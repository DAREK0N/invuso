//! PaddleOCR PP-OCRv6 small, run by `rten` in pure Rust (decision 10.1).
//!
//! Two ONNX models: a DBNet detector marks where text is, a CTC recognizer
//! reads each marked line. Pre- and post-processing follow PaddleOCR's
//! `DBPostProcess` and `CTCLabelDecode`, ported from the AP-S1 spike
//! (`spikes/ocr/`), with rotated boxes so slightly tilted photos still read.
//! Detection runs twice: over the whole photo to find the receipt's text,
//! then over that area alone, closer up (see [`PaddleOcr::detect`]).

use image::RgbImage;
use rten::Model;
use rten_imageproc::{PointF, RetrievalMode, RotatedRect, Vec2, find_contours, min_area_rect};
use rten_tensor::prelude::*;
use rten_tensor::{NdTensor, Tensor};

use super::{OcrEngine, OcrError, OcrProgress, Recognition, preprocess};
use crate::storage::OcrFragment;
use invuso_core::receipt::BoundingBox;

/// Engine name stored with every result (`receipt.ocr_engine`).
pub const ENGINE_NAME: &str = "pp-ocrv6-small";

/// Longest side of the overview fed to the detector. AP-S1 found nothing
/// more above 1920 px, but long, densely printed own receipts (AP-19) lose
/// fewer prices at 2560 px; the detail pass then looks closer still.
const DETECT_MAX_SIDE: f32 = 2560.0;
/// Detail pass ([`PaddleOcr::detect`]): thickness of a found line region
/// in detector pixels it aims for, …
const DETAIL_LINE_HEIGHT: f32 = 80.0;
/// … the most it scales the photo up, …
const DETAIL_MAX_SCALE: f32 = 2.5;
/// … the most detector pixels it spends on all bands together, …
const DETAIL_MAX_PIXELS: f32 = 8_000_000.0;
/// … the height of one band in detector pixels, …
const DETAIL_BAND_HEIGHT: f32 = 1280.0;
/// … and how many line heights the bands overlap and the text area
/// reaches beyond the found text.
const DETAIL_OVERLAP_LINES: f32 = 3.0;
const DETAIL_MARGIN_LINES: f32 = 2.0;
/// The detail pass runs only when it looks this much closer than the
/// overview, and when the overview found this many regions.
const DETAIL_MIN_GAIN: f32 = 1.25;
/// An overview region the detail pass covers less than this much of was
/// missed by it and is kept.
const MISSED_COVER: f32 = 0.5;
/// Share of a region's pixels that must be brighter than the paper
/// threshold ([`on_paper`]).
const PAPER_SHARE: f32 = 0.5;
/// Regions this many times thicker or thinner than the median line are not
/// counted towards the text area.
const DETAIL_LINE_SPREAD: f32 = 2.0;
const DETAIL_MIN_REGIONS: usize = 3;
/// Shortest side fed to the detector; small images are scaled up.
const DETECT_MIN_SIDE: f32 = 736.0;
/// Detector pixels above this probability count as text.
const TEXT_THRESHOLD: f32 = 0.3;
/// Regions whose mean probability is lower are dropped.
const BOX_THRESHOLD: f32 = 0.45;
/// How far a region is grown, relative to area / perimeter (Paddle's
/// `unclip_ratio`); DBNet marks only the core of each line.
const UNCLIP_RATIO: f32 = 1.4;
/// Steepest tilt that is levelled, in degrees; regions are read upright
/// only up to 45°.
const MAX_SKEW: f32 = 30.0;
/// Regions whose angles differ by at most this many degrees from a
/// group's centre belong to it ([`skew_angle`]).
const SKEW_GROUP: f32 = 3.0;
/// Pieces of one line ([`join_fragments`]): how much of the lower piece
/// they must share vertically, how far apart they may be horizontally and
/// how different in height, all relative to the lower piece's height.
const JOIN_OVERLAP: f32 = 0.6;
const JOIN_GAP: f32 = 0.5;
const JOIN_HEIGHT_RATIO: f32 = 1.6;
/// Regions thinner than this (detector pixels) are noise.
const MIN_REGION_SIDE: f32 = 3.0;
/// Input height of the recognizer.
const LINE_HEIGHT: u32 = 48;
/// Upper bound for the recognizer's input width.
const MAX_LINE_WIDTH: u32 = 3200;
/// Slope (sine of the angle) below which a line counts as level: half a
/// pixel over 50 pixels of text.
const LEVEL_TOLERANCE: f32 = 0.01;
/// A region counts towards the skew estimate when it is at least this many
/// times longer than high; short ones (single digits) have no clear
/// direction.
const SKEW_MIN_ELONGATION: f32 = 3.0;
/// Fewer elongated regions than this give no skew estimate.
const SKEW_MIN_REGIONS: usize = 3;

// ImageNet statistics the detector was trained with, in its BGR order.
const DETECT_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const DETECT_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// The loaded PP-OCRv6 models. Runs share it: `rten` models are immutable
/// while running.
pub struct PaddleOcr {
    detector: Model,
    recognizer: Model,
    /// Recognizer classes: CTC blank, the dictionary, then a space.
    chars: Vec<String>,
}

impl PaddleOcr {
    /// Loads the models from their ONNX bytes and the recognizer's
    /// character list (one character per line).
    pub fn load(
        detector: Vec<u8>,
        recognizer: Vec<u8>,
        dictionary: &str,
    ) -> Result<Self, OcrError> {
        let mut chars = vec![String::new()];
        chars.extend(
            dictionary
                .lines()
                .map(|line| line.trim_end_matches('\r').to_string()),
        );
        chars.push(" ".to_string());
        Ok(Self {
            detector: Model::load(detector).map_err(|e| OcrError::Model(e.to_string()))?,
            recognizer: Model::load(recognizer).map_err(|e| OcrError::Model(e.to_string()))?,
            chars,
        })
    }

    /// Text regions of the whole photo as rotated rectangles in image
    /// pixels, in two passes: the first, on the photo scaled into the
    /// detector's bounds, finds where the receipt's text is and how large
    /// it is printed; the second cuts that area out and searches it again
    /// in bands, scaled so a printed line has [`DETAIL_LINE_HEIGHT`]
    /// detector pixels. A long receipt photographed whole otherwise gets
    /// only a few pixels per line in the first pass.
    fn detect(&self, image: &RgbImage) -> Result<Vec<RotatedRect>, OcrError> {
        let (width, height) = image.dimensions();
        let (input_w, input_h) = detector_size(width, height);
        let overview = self.detect_scaled(image, input_w, input_h)?;
        // Text on a patterned or printed background is not the receipt's.
        let paper = preprocess::paper_threshold(image);
        let on_paper: Vec<RotatedRect> = overview
            .iter()
            .filter(|rect| on_paper(image, rect, paper))
            .copied()
            .collect();
        let Some(plan) = detail_plan(&on_paper, width, height, input_w as f32 / width as f32)
        else {
            return Ok(overview);
        };
        let mut rects = Vec::new();
        for band in &plan.bands {
            let view = image::imageops::crop_imm(
                image,
                plan.left,
                band.top,
                plan.width,
                band.bottom - band.top,
            )
            .to_image();
            let round =
                |side: u32| ((side as f32 * plan.scale / 32.0).round().max(1.0) * 32.0) as u32;
            let found = self.detect_scaled(&view, round(view.width()), round(view.height()))?;
            let offset = Vec2::from_yx(band.top as f32, plan.left as f32);
            rects.extend(found.into_iter().filter_map(|rect| {
                let center = rect.center().to_vec() + offset;
                // Each line belongs to the band whose own part holds its
                // centre; the overlap only makes sure it is seen whole.
                (center.y >= band.own_top as f32 && center.y < band.own_bottom as f32)
                    .then(|| move_rect(&rect, offset))
            }));
        }
        // Up close the detector now and then misses a line it saw from
        // afar; the overview's region stands in for it.
        let found: Vec<BoundingBox> = rects
            .iter()
            .map(|rect| bounding_box(rect, width, height))
            .collect();
        rects.extend(
            on_paper
                .into_iter()
                .filter(|rect| covered(&bounding_box(rect, width, height), &found) < MISSED_COVER),
        );
        Ok(rects)
    }

    /// Runs the detector on `image` scaled to `input_w` × `input_h`
    /// (multiples of 32); regions come back in `image` pixels.
    fn detect_scaled(
        &self,
        image: &RgbImage,
        input_w: u32,
        input_h: u32,
    ) -> Result<Vec<RotatedRect>, OcrError> {
        let (width, height) = image.dimensions();
        let resized = image::imageops::resize(
            image,
            input_w,
            input_h,
            image::imageops::FilterType::Triangle,
        );
        let plane = (input_w * input_h) as usize;
        let mut data = vec![0f32; 3 * plane];
        for (i, pixel) in resized.pixels().enumerate() {
            let bgr = [pixel[2], pixel[1], pixel[0]];
            for c in 0..3 {
                data[c * plane + i] = (f32::from(bgr[c]) / 255.0 - DETECT_MEAN[c]) / DETECT_STD[c];
            }
        }
        let (shape, probability) = run(
            &self.detector,
            [1, 3, input_h as usize, input_w as usize],
            data,
        )?;
        let [.., map_h, map_w] = shape[..] else {
            return Err(OcrError::Model("unexpected detector output".to_string()));
        };
        let scale_x = width as f32 / map_w as f32;
        let scale_y = height as f32 / map_h as f32;
        Ok(regions(&probability, map_w, map_h)
            .into_iter()
            .map(|rect| scale_rect(&rect, scale_x, scale_y))
            .collect())
    }

    /// Reads one line; returns the text and the mean probability of its
    /// characters.
    fn recognize(&self, image: &RgbImage, rect: &RotatedRect) -> Result<(String, f32), OcrError> {
        let (line_w, data) = line_input(image, rect);
        let (shape, output) = run(
            &self.recognizer,
            [1, 3, LINE_HEIGHT as usize, line_w as usize],
            data,
        )?;
        let [_, steps, classes] = shape[..] else {
            return Err(OcrError::Model("unexpected recognizer output".to_string()));
        };
        if classes != self.chars.len() {
            return Err(OcrError::Model(format!(
                "recognizer has {classes} classes, dictionary {}",
                self.chars.len()
            )));
        }
        Ok(ctc_decode(&output, steps, classes, &self.chars))
    }
}

impl OcrEngine for PaddleOcr {
    fn name(&self) -> &'static str {
        ENGINE_NAME
    }

    fn recognize(
        &self,
        image: &RgbImage,
        progress: &mut dyn FnMut(OcrProgress),
    ) -> Result<Recognition, OcrError> {
        progress(OcrProgress::Detecting);
        let rects = self.detect(image)?;
        let skew = skew_angle(&rects);
        let center = PointF::from_yx(image.height() as f32 / 2.0, image.width() as f32 / 2.0);
        let rects = join_fragments(rects, skew, center);
        let total = rects.len();
        let mut fragments = Vec::new();
        for (done, rect) in rects.iter().enumerate() {
            progress(OcrProgress::Reading { done, total });
            let (text, confidence) = PaddleOcr::recognize(self, image, rect)?;
            let text = text.trim();
            if !text.is_empty() {
                let bbox = if skew == 0.0 {
                    bounding_box(rect, image.width(), image.height())
                } else {
                    level_box(rect, skew, center)
                };
                fragments.push(OcrFragment {
                    text: text.to_string(),
                    bbox,
                    confidence,
                });
            }
        }
        Ok(Recognition {
            fragments,
            skew_degrees: skew.to_degrees(),
        })
    }
}

/// Runs a model with one float input; returns shape and data of its first
/// output.
fn run(
    model: &Model,
    shape: [usize; 4],
    data: Vec<f32>,
) -> Result<(Vec<usize>, Vec<f32>), OcrError> {
    let input = Tensor::from_data(&shape, data);
    let output = model
        .run_one(input.into(), None)
        .map_err(|e| OcrError::Model(e.to_string()))?;
    let output: Tensor<f32> = output
        .try_into()
        .map_err(|_| OcrError::Model("unexpected output type".to_string()))?;
    Ok((output.shape().to_vec(), output.to_vec()))
}

/// Detector input size: scaled into [`DETECT_MIN_SIDE`, `DETECT_MAX_SIDE`]
/// and rounded to multiples of 32, as DBNet requires.
fn detector_size(width: u32, height: u32) -> (u32, u32) {
    let min_side = width.min(height) as f32;
    let mut scale = if min_side < DETECT_MIN_SIDE {
        DETECT_MIN_SIDE / min_side
    } else {
        1.0
    };
    let max_side = width.max(height) as f32 * scale;
    if max_side > DETECT_MAX_SIDE {
        scale *= DETECT_MAX_SIDE / max_side;
    }
    let round = |side: u32| (((side as f32 * scale) / 32.0).round().max(1.0) * 32.0) as u32;
    (round(width), round(height))
}

/// Where and how large the second detector pass looks (image pixels).
#[derive(Debug, PartialEq)]
struct DetailPlan {
    left: u32,
    width: u32,
    /// Detector pixels per image pixel.
    scale: f32,
    bands: Vec<Band>,
}

/// One horizontal strip of the text area. It reaches `overlap` beyond the
/// part it owns, so lines on the border are seen whole by one of two
/// neighbours.
#[derive(Debug, PartialEq)]
struct Band {
    top: u32,
    bottom: u32,
    own_top: u32,
    own_bottom: u32,
}

/// Plans the detail pass from the overview's regions; `None` when it would
/// not look any closer than the overview did.
fn detail_plan(
    overview: &[RotatedRect],
    width: u32,
    height: u32,
    overview_scale: f32,
) -> Option<DetailPlan> {
    let mut thickness: Vec<f32> = overview.iter().map(|rect| line_axes(rect).3).collect();
    if thickness.len() < DETAIL_MIN_REGIONS {
        return None;
    }
    thickness.sort_by(f32::total_cmp);
    let line = thickness[thickness.len() / 2];

    // Blobs much thicker or thinner than a line are background (a
    // patterned table cloth, a shadow), not part of the receipt.
    let corners = overview
        .iter()
        .filter(|rect| {
            let t = line_axes(rect).3;
            t >= line / DETAIL_LINE_SPREAD && t <= line * DETAIL_LINE_SPREAD
        })
        .flat_map(|rect| rect.corners());
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in corners {
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    let margin = DETAIL_MARGIN_LINES * line;
    let clamp = |v: f32, max: u32| v.clamp(0.0, max as f32) as u32;
    let (left, top) = (clamp(x0 - margin, width), clamp(y0 - margin, height));
    let (right, bottom) = (clamp(x1 + margin, width), clamp(y1 + margin, height));
    let (area_w, area_h) = (right.saturating_sub(left), bottom.saturating_sub(top));
    if area_w == 0 || area_h == 0 {
        return None;
    }

    let mut scale = (DETAIL_LINE_HEIGHT / line).min(DETAIL_MAX_SCALE);
    let pixels = area_w as f32 * area_h as f32 * scale * scale;
    if pixels > DETAIL_MAX_PIXELS {
        scale *= (DETAIL_MAX_PIXELS / pixels).sqrt();
    }
    if scale < DETAIL_MIN_GAIN * overview_scale {
        return None;
    }

    let band_h = DETAIL_BAND_HEIGHT / scale;
    let overlap = (DETAIL_OVERLAP_LINES * line).ceil() as u32;
    let count = (area_h as f32 / band_h).ceil().max(1.0) as u32;
    let bands = (0..count)
        .map(|i| {
            let own_top = top + area_h * i / count;
            let own_bottom = top + area_h * (i + 1) / count;
            Band {
                top: own_top.saturating_sub(overlap).max(top),
                bottom: (own_bottom + overlap).min(bottom),
                own_top: if i == 0 { 0 } else { own_top },
                own_bottom: if i + 1 == count { height } else { own_bottom },
            }
        })
        .collect();
    Some(DetailPlan {
        left,
        width: area_w,
        scale,
        bands,
    })
}

/// Whether most of a region is paper: receipt text is dark ink on a
/// bright slip, so a line's box is mostly brighter than `paper`.
fn on_paper(image: &RgbImage, rect: &RotatedRect, paper: u8) -> bool {
    let bbox = bounding_box(rect, image.width(), image.height());
    let (width, height) = (bbox.right - bbox.left, bbox.bottom - bbox.top);
    if width <= 0 || height <= 0 {
        return false;
    }
    // A coarse grid is enough to tell paper from background.
    let step = (width.min(height) / 8).max(1) as usize;
    let (mut bright, mut total) = (0usize, 0usize);
    for y in (bbox.top..bbox.bottom).step_by(step) {
        for x in (bbox.left..bbox.right).step_by(step) {
            total += 1;
            if preprocess::luma(image.get_pixel(x as u32, y as u32).0) > paper {
                bright += 1;
            }
        }
    }
    bright as f32 >= PAPER_SHARE * total as f32
}

/// Share of `target`'s area that `boxes` cover.
fn covered(target: &BoundingBox, boxes: &[BoundingBox]) -> f32 {
    let area =
        |b: &BoundingBox| ((b.right - b.left).max(0) as f32) * ((b.bottom - b.top).max(0) as f32);
    let total = area(target);
    if total == 0.0 {
        return 1.0;
    }
    let overlap: f32 = boxes
        .iter()
        .map(|b| {
            area(&BoundingBox {
                left: b.left.max(target.left),
                top: b.top.max(target.top),
                right: b.right.min(target.right),
                bottom: b.bottom.min(target.bottom),
            })
        })
        .sum();
    overlap / total
}

/// A region shifted by `offset`.
fn move_rect(rect: &RotatedRect, offset: Vec2) -> RotatedRect {
    RotatedRect::new(
        PointF::from_yx(rect.center().y + offset.y, rect.center().x + offset.x),
        rect.up_axis(),
        rect.width(),
        rect.height(),
    )
}

/// Turns the detector's probability map into grown text regions, in map
/// pixels.
fn regions(probability: &[f32], map_w: usize, map_h: usize) -> Vec<RotatedRect> {
    let mask: Vec<bool> = probability.iter().map(|&p| p > TEXT_THRESHOLD).collect();
    let mask = NdTensor::from_data([map_h, map_w], mask);
    let contours = find_contours(mask.view(), RetrievalMode::External);
    contours
        .iter()
        .filter_map(|contour| {
            let points: Vec<PointF> = contour.iter().map(|p| p.to_f32()).collect();
            let rect = min_area_rect(&points)?;
            if rect.width().min(rect.height()) < MIN_REGION_SIDE
                || region_score(probability, map_w, map_h, &rect) < BOX_THRESHOLD
            {
                return None;
            }
            // Paddle's unclip: offset = area * ratio / perimeter.
            let (w, h) = (rect.width(), rect.height());
            let offset = w * h * UNCLIP_RATIO / (2.0 * (w + h));
            Some(rect.expanded(2.0 * offset, 2.0 * offset))
        })
        .collect()
}

/// Mean probability inside a region (Paddle's `box_score_fast`, with the
/// rotated rectangle instead of the polygon).
fn region_score(probability: &[f32], map_w: usize, map_h: usize, rect: &RotatedRect) -> f32 {
    let corners = rect.corners();
    let clamp = |v: f32, max: usize| (v.max(0.0) as usize).min(max - 1);
    let x0 = clamp(
        corners.iter().map(|p| p.x).fold(f32::MAX, f32::min).floor(),
        map_w,
    );
    let x1 = clamp(
        corners.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil(),
        map_w,
    );
    let y0 = clamp(
        corners.iter().map(|p| p.y).fold(f32::MAX, f32::min).floor(),
        map_h,
    );
    let y1 = clamp(
        corners.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil(),
        map_h,
    );
    let (mut sum, mut count) = (0.0f32, 0usize);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if rect.contains(PointF::from_yx(y as f32, x as f32)) {
                sum += probability[y * map_w + x];
                count += 1;
            }
        }
    }
    if count == 0 { 0.0 } else { sum / count as f32 }
}

/// Maps a region from detector map pixels to image pixels.
fn scale_rect(rect: &RotatedRect, scale_x: f32, scale_y: f32) -> RotatedRect {
    let corners = rect
        .corners()
        .map(|p| PointF::from_yx(p.y * scale_y, p.x * scale_x));
    // Four corners always give a rectangle.
    min_area_rect(&corners).unwrap_or(*rect)
}

/// How far the receipt's lines are turned against the image (radians,
/// positive = sloping down to the right); 0 if they are level or too few
/// regions have a clear direction.
///
/// Other text in the picture (a second slip lying across) has its own
/// direction, so the elongated regions are grouped by angle and the group
/// with the most text wins, weighted by line height: the receipt being
/// photographed is the largest print in the picture. Its median angle is
/// the skew.
fn skew_angle(rects: &[RotatedRect]) -> f32 {
    let mut lines: Vec<(f32, f32)> = rects
        .iter()
        .filter_map(|rect| {
            let (along, length, _, thickness) = line_axes(rect);
            (length >= SKEW_MIN_ELONGATION * thickness)
                .then(|| (along.y.atan2(along.x), length * thickness * thickness))
        })
        .collect();
    if lines.len() < SKEW_MIN_REGIONS {
        return 0.0;
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    let spread = SKEW_GROUP.to_radians();
    let group = |center: f32| {
        lines
            .iter()
            .filter(move |(angle, _)| (angle - center).abs() <= spread)
    };
    let Some(&(center, _)) = lines.iter().max_by(|a, b| {
        let weight = |center: f32| group(center).map(|(_, w)| w).sum::<f32>();
        weight(a.0).total_cmp(&weight(b.0))
    }) else {
        return 0.0;
    };
    let angles: Vec<f32> = group(center).map(|(angle, _)| *angle).collect();
    let median = angles[angles.len() / 2];
    if median.sin().abs() < LEVEL_TOLERANCE || median.abs() > MAX_SKEW.to_radians() {
        0.0
    } else {
        median
    }
}

/// Joins regions that are pieces of one printed line. On widely spaced
/// till fonts the detector often splits a price at its comma (`0` | `99`),
/// and the comma in the gap is lost for good; read as one piece it stays.
/// Two regions join when, in the levelled receipt, they overlap vertically
/// by most of the lower one, are of similar height and are at most
/// [`JOIN_GAP`] line heights apart.
fn join_fragments(rects: Vec<RotatedRect>, skew: f32, center: PointF) -> Vec<RotatedRect> {
    let boxes: Vec<BoundingBox> = rects.iter().map(|r| level_box(r, skew, center)).collect();
    let height = |b: &BoundingBox| (b.bottom - b.top).max(1) as f32;
    let mut parent: Vec<usize> = (0..rects.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..boxes.len() {
        for j in i + 1..boxes.len() {
            let (a, b) = (&boxes[i], &boxes[j]);
            let (ha, hb) = (height(a), height(b));
            let low = ha.min(hb);
            if ha.max(hb) > JOIN_HEIGHT_RATIO * low {
                continue;
            }
            let overlap = (a.bottom.min(b.bottom) - a.top.max(b.top)) as f32;
            let gap = (a.left.max(b.left) - a.right.min(b.right)) as f32;
            if overlap >= JOIN_OVERLAP * low && gap <= JOIN_GAP * low {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<PointF>> =
        std::collections::BTreeMap::new();
    for (i, rect) in rects.iter().enumerate() {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().extend(rect.corners());
    }
    groups
        .into_values()
        .filter_map(|corners| min_area_rect(&corners))
        .collect()
}

/// Box of a region in the levelled receipt: its corners turned back by
/// `skew` around `center`. Rows of a tilted photo then share their height
/// again, which the parser's row reconstruction (OCR-10) relies on.
fn level_box(rect: &RotatedRect, skew: f32, center: PointF) -> BoundingBox {
    let (sin, cos) = skew.sin_cos();
    let corners = rect.corners().map(|p| {
        let (dx, dy) = (p.x - center.x, p.y - center.y);
        (
            center.x + dx * cos + dy * sin,
            center.y - dx * sin + dy * cos,
        )
    });
    let left = corners.iter().map(|c| c.0).fold(f32::MAX, f32::min);
    let right = corners.iter().map(|c| c.0).fold(f32::MIN, f32::max);
    let top = corners.iter().map(|c| c.1).fold(f32::MAX, f32::min);
    let bottom = corners.iter().map(|c| c.1).fold(f32::MIN, f32::max);
    BoundingBox {
        left: left.floor() as i32,
        top: top.floor() as i32,
        right: right.ceil() as i32,
        bottom: bottom.ceil() as i32,
    }
}

/// Axis-aligned box around a region, inside the image.
fn bounding_box(rect: &RotatedRect, width: u32, height: u32) -> BoundingBox {
    let corners = rect.corners();
    let xs = corners.iter().map(|p| p.x);
    let ys = corners.iter().map(|p| p.y);
    let clamp = |v: f32, max: u32| v.clamp(0.0, max as f32) as i32;
    BoundingBox {
        left: clamp(xs.clone().fold(f32::MAX, f32::min).floor(), width),
        top: clamp(ys.clone().fold(f32::MAX, f32::min).floor(), height),
        right: clamp(xs.fold(f32::MIN, f32::max).ceil(), width),
        bottom: clamp(ys.fold(f32::MIN, f32::max).ceil(), height),
    }
}

/// Reading direction and line extent of a region: `along` is the side
/// closer to horizontal, pointing right; `down` points down. Receipts are
/// photographed upright, so a region's text never runs more than 45° off.
fn line_axes(rect: &RotatedRect) -> (Vec2, f32, Vec2, f32) {
    let up = rect.up_axis();
    let across = up.perpendicular();
    // `width` runs along `across`, `height` along `up`.
    let (mut along, length, mut down, thickness) = if across.x.abs() >= across.y.abs() {
        (across, rect.width(), up, rect.height())
    } else {
        (up, rect.height(), across, rect.width())
    };
    if along.x < 0.0 {
        along = -along;
    }
    if down.y < 0.0 {
        down = -down;
    }
    (along, length.max(1.0), down, thickness.max(1.0))
}

/// Cuts a region out of the image, straightened and scaled to the
/// recognizer's height; returns its width and the normalized CHW data.
fn line_input(image: &RgbImage, rect: &RotatedRect) -> (u32, Vec<f32>) {
    let line = straighten(image, rect);
    let line_w = ((line.width() as f32 * LINE_HEIGHT as f32 / line.height() as f32).ceil() as u32)
        .clamp(16, MAX_LINE_WIDTH);
    // An area filter, not point sampling: lines on a 12-megapixel photo
    // are often three times the target height.
    let resized = image::imageops::resize(
        &line,
        line_w,
        LINE_HEIGHT,
        image::imageops::FilterType::Triangle,
    );
    let plane = (line_w * LINE_HEIGHT) as usize;
    let mut data = vec![0f32; 3 * plane];
    for (i, pixel) in resized.pixels().enumerate() {
        // BGR, scaled to [-1, 1] as the recognizer was trained.
        for (c, value) in [pixel[2], pixel[1], pixel[0]].into_iter().enumerate() {
            data[c * plane + i] = (f32::from(value) / 255.0 - 0.5) / 0.5;
        }
    }
    (line_w, data)
}

/// The region as an upright image at full resolution. Practically level
/// regions are cut out as they are, so their pixels stay unblurred.
fn straighten(image: &RgbImage, rect: &RotatedRect) -> RgbImage {
    let (along, length, down, thickness) = line_axes(rect);
    if along.y.abs() < LEVEL_TOLERANCE {
        let bbox = bounding_box(rect, image.width(), image.height());
        let width = (bbox.right - bbox.left).max(1) as u32;
        let height = (bbox.bottom - bbox.top).max(1) as u32;
        let (left, top) = (bbox.left.max(0) as u32, bbox.top.max(0) as u32);
        if left + width <= image.width() && top + height <= image.height() {
            return image::imageops::crop_imm(image, left, top, width, height).to_image();
        }
    }
    let width = length.ceil() as u32;
    let height = thickness.ceil() as u32;
    let origin = rect.center().to_vec() - along * (length / 2.0) - down * (thickness / 2.0);
    RgbImage::from_fn(width, height, |col, row| {
        let at = origin + along * (col as f32 + 0.5) + down * (row as f32 + 0.5);
        let [r, g, b] = sample(image, at.x - 0.5, at.y - 0.5);
        image::Rgb([r.round() as u8, g.round() as u8, b.round() as u8])
    })
}

/// Bilinear sample at a continuous pixel position; edges repeat.
fn sample(image: &RgbImage, x: f32, y: f32) -> [f32; 3] {
    let max_x = image.width() as i64 - 1;
    let max_y = image.height() as i64 - 1;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let pixel = |px: i64, py: i64| {
        image
            .get_pixel(px.clamp(0, max_x) as u32, py.clamp(0, max_y) as u32)
            .0
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    let corners = [
        (pixel(x0, y0), (1.0 - fx) * (1.0 - fy)),
        (pixel(x0 + 1, y0), fx * (1.0 - fy)),
        (pixel(x0, y0 + 1), (1.0 - fx) * fy),
        (pixel(x0 + 1, y0 + 1), fx * fy),
    ];
    let mut out = [0f32; 3];
    for (value, weight) in corners {
        for c in 0..3 {
            out[c] += f32::from(value[c]) * weight;
        }
    }
    out
}

/// Greedy CTC decoding: best class per step, repeats merged, blanks
/// dropped. Returns the text and the mean probability of its characters.
fn ctc_decode(output: &[f32], steps: usize, classes: usize, chars: &[String]) -> (String, f32) {
    let mut text = String::new();
    let mut last = 0usize;
    let (mut sum, mut count) = (0f32, 0usize);
    for step in output.chunks_exact(classes).take(steps) {
        let (best, probability) =
            step.iter()
                .copied()
                .enumerate()
                .fold(
                    (0, f32::MIN),
                    |best, (i, p)| if p > best.1 { (i, p) } else { best },
                );
        if best != 0 && best != last {
            text.push_str(&chars[best]);
            sum += probability;
            count += 1;
        }
        last = best;
    }
    (text, if count == 0 { 0.0 } else { sum / count as f32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars() -> Vec<String> {
        ["", "a", "b", " "].map(String::from).to_vec()
    }

    #[test]
    fn ctc_merges_repeats_and_drops_blanks() {
        // Steps: a a blank a b b
        let steps = [
            [0.1, 0.8, 0.05, 0.05],
            [0.1, 0.9, 0.0, 0.0],
            [0.9, 0.05, 0.05, 0.0],
            [0.1, 0.7, 0.1, 0.1],
            [0.0, 0.0, 1.0, 0.0],
            [0.2, 0.0, 0.8, 0.0],
        ];
        let output: Vec<f32> = steps.iter().flatten().copied().collect();
        let (text, confidence) = ctc_decode(&output, steps.len(), 4, &chars());
        assert_eq!(text, "aab");
        assert!((confidence - (0.8 + 0.7 + 1.0) / 3.0).abs() < 1e-6);
        assert_eq!(
            ctc_decode(&[1.0, 0.0, 0.0, 0.0], 1, 4, &chars()),
            (String::new(), 0.0)
        );
    }

    #[test]
    fn detector_input_stays_within_bounds_in_multiples_of_32() {
        // A 12-megapixel photo is scaled down to the longest side.
        assert_eq!(detector_size(3000, 4000), (1920, 2560));
        // A small scan is scaled up to the shortest side.
        assert_eq!(detector_size(368, 1000), (736, 2016));
        // A narrow scan is scaled up, then capped by the longest side.
        assert_eq!(detector_size(368, 1600), (576, 2560));
        assert_eq!(detector_size(500, 333), (1120, 736));
    }

    #[test]
    fn finds_a_tilted_line_and_grows_it() {
        // A 40 × 6 bar of text probability, tilted by about 5°.
        let (map_w, map_h) = (80usize, 40usize);
        let mut probability = vec![0f32; map_w * map_h];
        for i in 0..40 {
            let x = 20 + i;
            let y0 = 15 + i * 7 / 80;
            for y in y0..y0 + 6 {
                probability[y * map_w + x] = 0.9;
            }
        }
        let found = regions(&probability, map_w, map_h);
        assert_eq!(found.len(), 1);
        let (along, length, down, thickness) = line_axes(&found[0]);
        assert!(length > 40.0 && thickness > 6.0, "{length} × {thickness}");
        assert!(along.x > 0.99 && along.y > 0.0, "slopes down to the right");
        assert!(down.y > 0.99);
    }

    #[test]
    fn tilted_rows_are_levelled() {
        // Three lines sloping down by 6°, one short region without a
        // direction of its own.
        let skew = 6f32.to_radians();
        let up = Vec2::from_yx(-skew.cos(), skew.sin());
        let line =
            |x: f32, y: f32, width: f32| RotatedRect::new(PointF::from_yx(y, x), up, width, 20.0);
        let rects = [
            line(200.0, 100.0, 200.0),
            line(200.0, 200.0, 300.0),
            line(200.0, 300.0, 120.0),
            line(400.0, 50.0, 20.0),
        ];
        let angle = skew_angle(&rects);
        assert!((angle - skew).abs() < 1e-4, "{}", angle.to_degrees());

        // Name on the left and price on the right of one printed row lie
        // 0.1 × 400 px apart in the photo, level once turned back.
        let center = PointF::from_yx(0.0, 0.0);
        let name = line(-200.0 * skew.cos(), -200.0 * skew.sin(), 100.0);
        let price = line(200.0 * skew.cos(), 200.0 * skew.sin(), 60.0);
        let (a, b) = (
            level_box(&name, angle, center),
            level_box(&price, angle, center),
        );
        assert!(
            (a.top - b.top).abs() <= 1 && (a.bottom - b.bottom).abs() <= 1,
            "{a:?} {b:?}"
        );
        assert!(
            (a.left + 250).abs() <= 1 && (a.right + 150).abs() <= 1,
            "{a:?}"
        );

        // Level receipts keep their exact boxes.
        let level = [0.0, 100.0, 200.0].map(|y| {
            RotatedRect::new(
                PointF::from_yx(y, 100.0),
                Vec2::from_yx(-1.0, 0.0),
                150.0,
                20.0,
            )
        });
        assert_eq!(skew_angle(&level), 0.0);
        assert_eq!(skew_angle(&rects[..2]), 0.0);
    }

    fn level_line(x: f32, y: f32, width: f32, thickness: f32) -> RotatedRect {
        RotatedRect::new(
            PointF::from_yx(y, x),
            Vec2::from_yx(-1.0, 0.0),
            width,
            thickness,
        )
    }

    #[test]
    fn skew_follows_the_largest_print() {
        let tilted = |degrees: f32, y: f32, thickness: f32| {
            let angle = degrees.to_radians();
            RotatedRect::new(
                PointF::from_yx(y, 500.0),
                Vec2::from_yx(-angle.cos(), angle.sin()),
                400.0,
                thickness,
            )
        };
        // The receipt, level and printed large, under a second slip lying
        // across with more, but smaller lines.
        let mut rects: Vec<RotatedRect> = (0..4)
            .map(|i| tilted(0.0, 1000.0 + 100.0 * i as f32, 40.0))
            .collect();
        rects.extend((0..7).map(|i| tilted(-22.0 + 0.3 * i as f32, 100.0 * i as f32, 20.0)));
        assert_eq!(skew_angle(&rects), 0.0);

        // A receipt photographed 18° askew is levelled.
        let rects: Vec<RotatedRect> = (0..5)
            .map(|i| tilted(-18.0, 100.0 * i as f32, 30.0))
            .collect();
        assert!((skew_angle(&rects).to_degrees() + 18.0).abs() < 0.01);
    }

    #[test]
    fn detail_pass_looks_closer_at_the_text_only() {
        // A long receipt in the middle of a 3000 × 4000 photo: lines 20 px
        // thick from y = 200 to 3800, and a blob on the table cloth.
        let mut overview: Vec<RotatedRect> = (0..=36)
            .map(|i| level_line(1200.0, 200.0 + 100.0 * i as f32, 400.0, 20.0))
            .collect();
        overview.push(level_line(2500.0, 500.0, 300.0, 200.0));
        let plan = detail_plan(&overview, 3000, 4000, 0.64).unwrap();
        // Two line heights of margin around the text, nothing of the blob.
        assert_eq!((plan.left, plan.width), (960, 480));
        // 80 / 20 would be 4, capped at 2.5, then lowered to the budget.
        let pixels = 480.0 * 3700.0 * plan.scale * plan.scale;
        assert!(plan.scale < 2.5 && pixels <= DETAIL_MAX_PIXELS * 1.001);
        assert!(plan.scale > 2.0, "{}", plan.scale);

        // The bands own the whole photo height without gaps and reach three
        // line heights into their neighbours.
        let bands = &plan.bands;
        assert!(bands.len() > 1);
        assert_eq!(bands[0].own_top, 0);
        assert_eq!(bands.last().unwrap().own_bottom, 4000);
        for pair in bands.windows(2) {
            assert_eq!(pair[0].own_bottom, pair[1].own_top);
            assert_eq!(pair[0].bottom, pair[0].own_bottom + 60);
            assert_eq!(pair[1].top, pair[1].own_top - 60);
        }
        for band in bands {
            assert!(band.top >= 150 && band.bottom <= 3850);
            let detector_height = (band.bottom - band.top) as f32 * plan.scale;
            assert!(detector_height <= DETAIL_BAND_HEIGHT + 2.0 * 60.0 * plan.scale + 1.0);
        }

        // No closer look when the overview already saw the lines large
        // enough, or saw too little to judge.
        assert_eq!(detail_plan(&overview, 3000, 4000, 2.0), None);
        assert_eq!(detail_plan(&overview[..2], 3000, 4000, 0.64), None);
    }

    #[test]
    fn missed_lines_are_told_by_their_cover() {
        let target = BoundingBox {
            left: 0,
            top: 0,
            right: 100,
            bottom: 10,
        };
        let half = BoundingBox {
            left: 50,
            top: -5,
            right: 200,
            bottom: 20,
        };
        let apart = BoundingBox {
            left: 0,
            top: 50,
            right: 100,
            bottom: 60,
        };
        assert_eq!(covered(&target, &[]), 0.0);
        assert_eq!(covered(&target, &[apart]), 0.0);
        assert_eq!(covered(&target, &[half, apart]), 0.5);

        let moved = move_rect(&level_line(10.0, 20.0, 8.0, 4.0), Vec2::from_yx(100.0, 5.0));
        assert_eq!(
            bounding_box(&moved, 1000, 1000),
            BoundingBox {
                left: 11,
                top: 118,
                right: 19,
                bottom: 122
            }
        );
    }

    #[test]
    fn lines_on_the_dark_table_are_not_on_paper() {
        // Dark cloth on the left, white paper with a black stroke on the
        // right.
        let image = RgbImage::from_fn(200, 100, |x, y| {
            if x < 100 {
                image::Rgb([40, 40, 40])
            } else if y == 50 {
                image::Rgb([0, 0, 0])
            } else {
                image::Rgb([230, 230, 230])
            }
        });
        let paper = preprocess::paper_threshold(&image);
        assert!((40..230).contains(&paper), "{paper}");
        assert!(on_paper(
            &image,
            &level_line(150.0, 50.0, 80.0, 20.0),
            paper
        ));
        assert!(!on_paper(
            &image,
            &level_line(50.0, 50.0, 80.0, 20.0),
            paper
        ));
    }

    #[test]
    fn noise_and_weak_regions_are_dropped() {
        let (map_w, map_h) = (40usize, 20usize);
        let mut probability = vec![0f32; map_w * map_h];
        // Two pixels high: noise.
        for x in 5..30 {
            probability[3 * map_w + x] = 0.9;
            probability[4 * map_w + x] = 0.9;
        }
        // Above the text threshold but below the box threshold.
        for y in 10..16 {
            for x in 5..30 {
                probability[y * map_w + x] = 0.3;
            }
        }
        assert!(regions(&probability, map_w, map_h).is_empty());
    }

    #[test]
    fn upright_line_is_cut_out_unchanged() {
        // Left half black, right half white.
        let image = RgbImage::from_fn(96, 48, |x, _| {
            if x < 48 {
                image::Rgb([0, 0, 0])
            } else {
                image::Rgb([255, 255, 255])
            }
        });
        let rect = RotatedRect::new(
            PointF::from_yx(24.0, 48.0),
            Vec2::from_yx(-1.0, 0.0),
            96.0,
            48.0,
        );
        let (line_w, data) = line_input(&image, &rect);
        assert_eq!(line_w, 96);
        let plane = (96 * 48) as usize;
        assert_eq!(data.len(), 3 * plane);
        assert_eq!(data[10], -1.0);
        assert_eq!(data[90], 1.0);
        assert_eq!(
            bounding_box(&rect, 96, 48),
            BoundingBox {
                left: 0,
                top: 0,
                right: 96,
                bottom: 48
            }
        );
    }
}
