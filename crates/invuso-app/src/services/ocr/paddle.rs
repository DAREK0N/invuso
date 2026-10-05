//! PaddleOCR PP-OCRv6 small, run by `rten` in pure Rust (decision 10.1).
//!
//! Two ONNX models: a DBNet detector marks where text is, a CTC recognizer
//! reads each marked line. Pre- and post-processing follow PaddleOCR's
//! `DBPostProcess` and `CTCLabelDecode`, ported from the AP-S1 spike
//! (`spikes/ocr/`), with rotated boxes so slightly tilted photos still read.

use image::RgbImage;
use rten::Model;
use rten_imageproc::{PointF, RetrievalMode, RotatedRect, Vec2, find_contours, min_area_rect};
use rten_tensor::prelude::*;
use rten_tensor::{NdTensor, Tensor};

use super::{OcrEngine, OcrError, OcrProgress, Recognition};
use crate::storage::OcrFragment;
use invuso_core::receipt::BoundingBox;

/// Engine name stored with every result (`receipt.ocr_engine`).
pub const ENGINE_NAME: &str = "pp-ocrv6-small";

/// Longest side fed to the detector. Larger inputs found no more text in
/// AP-S1 but doubled the time.
const DETECT_MAX_SIDE: f32 = 1920.0;
/// Shortest side fed to the detector; small images are scaled up.
const DETECT_MIN_SIDE: f32 = 736.0;
/// Detector pixels above this probability count as text.
const TEXT_THRESHOLD: f32 = 0.2;
/// Regions whose mean probability is lower are dropped.
const BOX_THRESHOLD: f32 = 0.45;
/// How far a region is grown, relative to area / perimeter (Paddle's
/// `unclip_ratio`); DBNet marks only the core of each line.
const UNCLIP_RATIO: f32 = 1.4;
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

    /// Text regions as rotated rectangles in image pixels.
    fn detect(&self, image: &RgbImage) -> Result<Vec<RotatedRect>, OcrError> {
        let (width, height) = image.dimensions();
        let (input_w, input_h) = detector_size(width, height);
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
/// positive = sloping down to the right): the median direction of the
/// elongated regions; 0 if they are level or too few.
fn skew_angle(rects: &[RotatedRect]) -> f32 {
    let mut angles: Vec<f32> = rects
        .iter()
        .filter_map(|rect| {
            let (along, length, _, thickness) = line_axes(rect);
            (length >= SKEW_MIN_ELONGATION * thickness).then(|| along.y.atan2(along.x))
        })
        .collect();
    if angles.len() < SKEW_MIN_REGIONS {
        return 0.0;
    }
    angles.sort_by(f32::total_cmp);
    let median = angles[angles.len() / 2];
    if median.sin().abs() < LEVEL_TOLERANCE {
        0.0
    } else {
        median
    }
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
        assert_eq!(detector_size(3000, 4000), (1440, 1920));
        // A small scan is scaled up to the shortest side.
        // A narrow scan is scaled up, then capped by the longest side.
        assert_eq!(detector_size(368, 1000), (704, 1920));
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
