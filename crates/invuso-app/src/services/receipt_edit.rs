//! Turning, cropping and straightening a receipt photo before recognition
//! (RCP-05, OCR-04; idee.md 7.2 step 1).
//!
//! The user turns the photo in quarter turns and drags four corners onto
//! the receipt's corners; [`ImageEdit::apply`] maps that quadrilateral onto
//! an upright rectangle (a perspective transform from four points), which
//! also crops away everything outside. [`detect_corners`] suggests the
//! corners from the paper's edge; two optional filters (stronger contrast,
//! sharpening) help recognition of faint or blurred print. The original
//! file stays untouched (AGENTS.md 7.4): [`save_edit`] writes the result
//! as a new file next to it. Image maths uses `f64`; no money is involved.

use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::{Rgb, RgbImage};

use super::ocr::preprocess::{luma, stretch_contrast};
use super::receipts::{RECEIPTS_DIR, ReceiptError, decode_upright, thumbnail_of};
use crate::storage::{Db, RECEIPT_AUTO_CORNERS, RECEIPT_CORNER_METHOD, ReceiptFiles, StorageError};

/// Longest side of a corrected image, as for recognition: phone photos are
/// larger, but a receipt line needs no more detail.
const MAX_SIDE: u32 = 4000;

/// Smallest share of the photo the corners may enclose; less is almost
/// certainly a slip of the finger.
const MIN_AREA: f64 = 0.01;

/// A point in the turned photo: `0.0..=1.0` from its left and top edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// The same spot after turning the photo a quarter clockwise.
    fn turned_clockwise(self) -> Self {
        Self::new(1.0 - self.y, self.x)
    }
}

/// How the user corrected a photo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageEdit {
    /// Clockwise quarter turns, `0..4`.
    pub quarter_turns: u8,
    /// The receipt's corners in the turned photo: top left, top right,
    /// bottom right, bottom left.
    pub corners: [Point; 4],
    /// Evens out shadows and lifts faint print (see [`flatten_contrast`]).
    pub contrast: bool,
    /// Sharpens slightly blurred print (unsharp mask).
    pub sharpen: bool,
}

impl Default for ImageEdit {
    /// Upright, uncropped, unfiltered: the photo as it is.
    fn default() -> Self {
        Self {
            quarter_turns: 0,
            corners: FULL,
            contrast: false,
            sharpen: false,
        }
    }
}

const FULL: [Point; 4] = [
    Point::new(0.0, 0.0),
    Point::new(1.0, 0.0),
    Point::new(1.0, 1.0),
    Point::new(0.0, 1.0),
];

impl ImageEdit {
    /// Whether the edit leaves the photo as it is.
    pub fn is_unchanged(&self) -> bool {
        *self == Self::default()
    }

    /// Turns the photo a quarter clockwise; the corners stay on the same
    /// spots of the receipt.
    pub fn turned_clockwise(self) -> Self {
        let [a, b, c, d] = self.corners.map(Point::turned_clockwise);
        // The former bottom left corner is now the top left one.
        Self {
            quarter_turns: (self.quarter_turns + 1) % 4,
            corners: [d, a, b, c],
            ..self
        }
    }

    /// The same turn and filters with corners found in the upright photo
    /// (by [`detect_corners`]), carried into the turned one.
    pub fn with_upright_corners(self, corners: [Point; 4]) -> Self {
        let mut edit = Self {
            quarter_turns: 0,
            corners,
            ..self
        };
        for _ in 0..self.quarter_turns % 4 {
            edit = edit.turned_clockwise();
        }
        edit
    }

    /// The same turn and filters with the corners back at the photo's
    /// edges.
    pub fn uncropped(self) -> Self {
        Self {
            corners: FULL,
            ..self
        }
    }

    /// Turns the photo a quarter counter-clockwise.
    pub fn turned_counter_clockwise(self) -> Self {
        self.turned_clockwise()
            .turned_clockwise()
            .turned_clockwise()
    }

    /// Whether the corners enclose a usable area: a convex shape, in
    /// order, not too small. Crossed or folded corners cannot be
    /// straightened.
    pub fn is_valid(&self) -> bool {
        let c = self.corners;
        let convex = (0..4).all(|i| {
            let (a, b, next) = (c[i], c[(i + 1) % 4], c[(i + 2) % 4]);
            let cross = (b.x - a.x) * (next.y - b.y) - (b.y - a.y) * (next.x - b.x);
            cross > 0.0
        });
        convex && area(&c) >= MIN_AREA
    }

    /// Applies the edit to an upright photo. `None` if the corners are not
    /// [valid](Self::is_valid).
    pub fn apply(&self, image: &RgbImage) -> Option<RgbImage> {
        if !self.is_valid() {
            return None;
        }
        let turned = match self.quarter_turns % 4 {
            1 => image::imageops::rotate90(image),
            2 => image::imageops::rotate180(image),
            3 => image::imageops::rotate270(image),
            _ => image.clone(),
        };
        let mut result = if self.corners == FULL {
            turned
        } else {
            straighten(&turned, &self.corners)?
        };
        if self.contrast {
            flatten_contrast(&mut result);
        }
        if self.sharpen {
            // A small radius: receipt print is a few pixels wide; the
            // threshold leaves paper grain alone.
            result = image::imageops::unsharpen(&result, SHARPEN_SIGMA, SHARPEN_THRESHOLD);
        }
        Some(result)
    }
}

/// Unsharp mask of [`ImageEdit::sharpen`].
const SHARPEN_SIGMA: f32 = 1.2;
const SHARPEN_THRESHOLD: i32 = 4;

/// Maps the quadrilateral `corners` (relative to `turned`) onto an upright
/// rectangle.
fn straighten(turned: &RgbImage, corners: &[Point; 4]) -> Option<RgbImage> {
    {
        let (w, h) = (f64::from(turned.width()), f64::from(turned.height()));
        let source = corners.map(|p| (p.x * w, p.y * h));
        let distance = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1);
        // The longer of two opposite edges, so no detail is lost.
        let mut width = distance(source[0], source[1]).max(distance(source[3], source[2]));
        let mut height = distance(source[0], source[3]).max(distance(source[1], source[2]));
        let longest = width.max(height);
        if longest > f64::from(MAX_SIDE) {
            let scale = f64::from(MAX_SIDE) / longest;
            width *= scale;
            height *= scale;
        }
        let (width, height) = (width.round().max(1.0), height.round().max(1.0));
        let target = [(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)];
        let homography = Homography::from_points(&target, &source)?;
        Some(warp(turned, &homography, width as u32, height as u32))
    }
}

/// Divides every pixel by the brightness of the paper around it, so
/// shadows and uneven light vanish and the print stands out evenly; then
/// spreads the result over the full range.
pub(crate) fn flatten_contrast(image: &mut RgbImage) {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return;
    }
    // Wide enough to see past a line of print to the paper, narrow enough
    // to follow the edge of a shadow.
    let radius = (width.max(height) / 40).max(4);
    let brightness: Vec<u8> = image.pixels().map(|p| luma(p.0)).collect();
    let background = local_mean(
        &brightness,
        width as usize,
        height as usize,
        radius as usize,
    );
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let paper = u32::from(background[(y * width + x) as usize]).max(1);
        for channel in pixel.0.iter_mut() {
            *channel = (u32::from(*channel) * 255 / paper).min(255) as u8;
        }
    }
    stretch_contrast(image);
}

/// Mean of the `(2r+1)²` square around each value of a row-major grid,
/// through a summed-area table. On a photo of paper the paper dominates
/// it; print is thin.
fn local_mean(values: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    let stride = width + 1;
    let mut sums = vec![0u64; stride * (height + 1)];
    for y in 0..height {
        let mut row = 0u64;
        for x in 0..width {
            row += u64::from(values[y * width + x]);
            sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row;
        }
    }
    let r = radius;
    let mut means = vec![0u8; width * height];
    for y in 0..height {
        let (top, bottom) = (y.saturating_sub(r), (y + r + 1).min(height));
        for x in 0..width {
            let (left, right) = (x.saturating_sub(r), (x + r + 1).min(width));
            let total = sums[bottom * stride + right] + sums[top * stride + left]
                - sums[top * stride + right]
                - sums[bottom * stride + left];
            let count = ((bottom - top) * (right - left)) as u64;
            means[y * width + x] = (total / count) as u8;
        }
    }
    means
}

/// Longest side the corners are searched at; the paper's edge needs no
/// more detail, and it keeps the search quick.
const DETECT_SIDE: u32 = 600;

/// Suggests the receipt's corners in an upright photo: the largest bright
/// area (paper on a darker background) and its outermost points towards
/// each corner. `None` if no such area stands out, e.g. on a white table
/// or when the receipt fills the photo.
pub fn detect_corners(photo: &RgbImage) -> Option<[Point; 4]> {
    let small = downscaled(photo);
    let (width, height) = small.dimensions();
    let (w, h) = (width as usize, height as usize);
    // Paper is bright in every channel; light wood or skin is bright too,
    // but not in blue, and a light beige desk is bright but tinted. So the
    // darkest channel counts, less twice the tint. Blurred, so the print
    // does not cut holes into it.
    let whiteness: Vec<u8> = small
        .pixels()
        .map(|p| {
            let low = i32::from(p.0.into_iter().min().unwrap_or(0));
            let high = i32::from(p.0.into_iter().max().unwrap_or(0));
            (low - TINT_WEIGHT * (high - low)).clamp(0, 255) as u8
        })
        .collect();
    let whiteness = local_mean(&whiteness, w, h, BLUR_RADIUS);
    let threshold = otsu(&whiteness);
    let bright: Vec<bool> = whiteness.iter().map(|&v| v > threshold).collect();
    // Shrunk first, so a sheet of paper or a bright spot that only touches
    // the receipt is not taken for part of it (seen on the phone, AP-34).
    let shrunk = erode(&bright, w, h, ERODE_RADIUS);
    let paper = largest_region(&shrunk, w, h);
    let share = paper.len() as f64 / f64::from(width * height);
    if !(0.05..=0.95).contains(&share) {
        return None;
    }
    // Outermost along the diagonals: for a receipt turned less than 45°
    // these are its corners.
    // Each corner is moved back out by what the shrinking took away.
    let r = ERODE_RADIUS as f64;
    let normalized = |&(x, y): &(usize, usize), (dx, dy): (f64, f64)| {
        Point::new(
            ((x as f64 + 0.5 + dx * r) / f64::from(width)).clamp(0.0, 1.0),
            ((y as f64 + 0.5 + dy * r) / f64::from(height)).clamp(0.0, 1.0),
        )
    };
    let extreme = |score: fn(&(usize, usize)) -> i64, largest: bool, outward: (f64, f64)| {
        let found = if largest {
            paper.iter().max_by_key(|p| score(p))
        } else {
            paper.iter().min_by_key(|p| score(p))
        };
        found.map(|p| normalized(p, outward))
    };
    let sum = |&(x, y): &(usize, usize)| x as i64 + y as i64;
    let difference = |&(x, y): &(usize, usize)| x as i64 - y as i64;
    let corners = [
        extreme(sum, false, (-1.0, -1.0))?,
        extreme(difference, true, (1.0, -1.0))?,
        extreme(sum, true, (1.0, 1.0))?,
        extreme(difference, false, (-1.0, 1.0))?,
    ];
    with_margin(squared_ends(corners, width, height))
}

/// Sets an end of the receipt (top or bottom) that slants by more than
/// [`TORN_SLANT_DEGREES`] at right angles to the sides, moved out to the
/// farther of its two corners. Receipts are often torn off at a slant;
/// straightened onto the tear, every printed line would slant as well
/// (fuel receipt: 11°, lines ran into each other). Less slant is left
/// alone: on crumpled paper the sides are no better guide than the ends
/// (Netto receipt: 7°, squaring lost four items).
fn squared_ends(corners: [Point; 4], width: u32, height: u32) -> [Point; 4] {
    let (w, h) = (f64::from(width), f64::from(height));
    let [tl, tr, br, bl] = corners.map(|p| (p.x * w, p.y * h));
    let sub = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0, a.1 - b.1);
    let dot = |a: (f64, f64), b: (f64, f64)| a.0 * b.0 + a.1 * b.1;
    let (left, right) = (sub(bl, tl), sub(br, tr));
    let down = (left.0 + right.0, left.1 + right.1);
    let length = down.0.hypot(down.1);
    if length < 1.0 || dot(left, down) <= 0.0 || dot(right, down) <= 0.0 {
        return corners;
    }
    let down = (down.0 / length, down.1 / length);
    // A point of a side where it reaches `level` along `down`.
    let at_level = |from: (f64, f64), side: (f64, f64), level: f64| {
        let s = (level - dot(from, down)) / dot(side, down);
        (from.0 + side.0 * s, from.1 + side.1 * s)
    };
    // Sine of the angle between an end and the perpendicular of the sides.
    let slant = |a: (f64, f64), b: (f64, f64)| {
        let end = sub(b, a);
        (dot(end, down) / end.0.hypot(end.1)).abs()
    };
    let torn = TORN_SLANT_DEGREES.to_radians().sin();
    let [mut tl, mut tr, mut br, mut bl] = [tl, tr, br, bl];
    if slant(tl, tr) > torn {
        let top = dot(tl, down).min(dot(tr, down));
        (tl, tr) = (at_level(tl, left, top), at_level(tr, right, top));
    }
    if slant(bl, br) > torn {
        let bottom = dot(bl, down).max(dot(br, down));
        (bl, br) = (at_level(bl, left, bottom), at_level(br, right, bottom));
    }
    [tl, tr, br, bl].map(|(x, y)| Point::new((x / w).clamp(0.0, 1.0), (y / h).clamp(0.0, 1.0)))
}

/// Slant from which an end counts as torn, not cut (see [`squared_ends`]).
const TORN_SLANT_DEGREES: f64 = 10.0;

/// Moves detected corners outward by [`CORNER_MARGIN`]; `None` if they do
/// not enclose a [valid](ImageEdit::is_valid) area.
fn with_margin(corners: [Point; 4]) -> Option<[Point; 4]> {
    // A little desk in the picture costs nothing, a price cut off at the
    // edge costs an item: measured on the synthetic and jawildtext photos
    // (AP-38), tight corners lost more than they gained.
    let outward = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let corners = std::array::from_fn(|i| {
        let (dx, dy) = outward[i];
        Point::new(
            (corners[i].x + dx * CORNER_MARGIN).clamp(0.0, 1.0),
            (corners[i].y + dy * CORNER_MARGIN).clamp(0.0, 1.0),
        )
    });
    let edit = ImageEdit {
        corners,
        ..ImageEdit::default()
    };
    edit.is_valid().then_some(corners)
}

/// The photo with its longest side at most [`DETECT_SIDE`].
fn downscaled(photo: &RgbImage) -> RgbImage {
    let longest = photo.width().max(photo.height());
    if longest > DETECT_SIDE {
        image::imageops::thumbnail(
            photo,
            (photo.width() * DETECT_SIDE / longest).max(1),
            (photo.height() * DETECT_SIDE / longest).max(1),
        )
    } else {
        photo.clone()
    }
}

/// How much a pixel's tint (brightest minus darkest channel) lowers its
/// whiteness: receipts are neutral white, desks and wood are tinted.
const TINT_WEIGHT: i32 = 2;

/// Margin added around detected corners, as a share of the photo.
const CORNER_MARGIN: f64 = 0.03;

/// Blur of the whiteness before thresholding, in pixels of the
/// [`DETECT_SIDE`] image: about the height of a printed line there.
const BLUR_RADIUS: usize = 3;

/// Value separating the two groups of a grid best (Otsu's method).
fn otsu(values: &[u8]) -> u8 {
    let mut histogram = [0u64; 256];
    for &v in values {
        histogram[usize::from(v)] += 1;
    }
    let total = values.len() as f64;
    let weighted: f64 = histogram
        .iter()
        .enumerate()
        .map(|(v, &n)| v as f64 * n as f64)
        .sum();
    let (mut below, mut below_weighted) = (0f64, 0f64);
    let (mut best, mut best_variance) = (0u8, 0f64);
    for (value, &count) in histogram.iter().enumerate() {
        below += count as f64;
        below_weighted += value as f64 * count as f64;
        let above = total - below;
        if below == 0.0 || above == 0.0 {
            continue;
        }
        let difference = below_weighted / below - (weighted - below_weighted) / above;
        let variance = below * above * difference * difference;
        if variance > best_variance {
            (best, best_variance) = (value as u8, variance);
        }
    }
    best
}

/// How far the bright mask is shrunk before the paper is picked, in
/// pixels of the [`DETECT_SIDE`] image.
const ERODE_RADIUS: usize = 4;

/// Keeps only pixels whose whole `(2r+1)²` square is `true`; done as a
/// row pass and a column pass.
fn erode(mask: &[bool], width: usize, height: usize, radius: usize) -> Vec<bool> {
    let pass = |source: &[bool], along_rows: bool| {
        let (lines, length) = if along_rows {
            (height, width)
        } else {
            (width, height)
        };
        let index = |line: usize, i: usize| {
            if along_rows {
                line * width + i
            } else {
                i * width + line
            }
        };
        let mut out = vec![false; source.len()];
        for line in 0..lines {
            // Length of the run of `true` ending at each position.
            let mut run = vec![0usize; length];
            for i in 0..length {
                run[i] = if source[index(line, i)] {
                    if i > 0 { run[i - 1] + 1 } else { 1 }
                } else {
                    0
                };
            }
            for i in 0..length {
                let end = (i + radius).min(length - 1);
                let start = i.saturating_sub(radius);
                out[index(line, i)] = run[end] > end - start;
            }
        }
        out
    };
    pass(&pass(mask, true), false)
}

/// Pixels of the largest 4-connected area of `true` in a row-major mask.
fn largest_region(mask: &[bool], width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut seen = vec![false; mask.len()];
    let mut best = Vec::new();
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        let mut region = Vec::new();
        seen[start] = true;
        stack.push(start);
        while let Some(index) = stack.pop() {
            let (x, y) = (index % width, index / width);
            region.push((x, y));
            let neighbours = [
                (x > 0).then(|| index - 1),
                (x + 1 < width).then(|| index + 1),
                (y > 0).then(|| index - width),
                (y + 1 < height).then(|| index + width),
            ];
            for next in neighbours.into_iter().flatten() {
                if mask[next] && !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        if region.len() > best.len() {
            best = region;
        }
    }
    best
}

/// Suggests the receipt's corners from its straight edges. Candidate lines
/// come from a Hough transform of the brightness gradient; of all shapes
/// of two lines across and two along whose sides are paper edges (brighter
/// inside, see [`step_share`]), the one covering the paper best wins (see
/// [`PaperMap`]). Unlike [`detect_corners`] it needs no clearly darker
/// background, only a visible edge, e.g. paper on a bright desk lit from
/// the side (seen on the phone, AP-38). Where the receipt runs off the
/// photo, the photo's edge stands in. `None` if no four edges enclose a
/// plausible receipt.
pub fn detect_corners_by_edges(photo: &RgbImage) -> Option<[Point; 4]> {
    let small = downscaled(photo);
    let (width, height) = small.dimensions();
    let (w, h) = (width as usize, height as usize);
    if w < 8 || h < 8 {
        return None;
    }
    let brightness: Vec<u8> = small.pixels().map(|p| luma(p.0)).collect();
    // Blurred, so print turns into grey bands and paper grain vanishes;
    // the paper's edge stays a step.
    let blurred = local_mean(&brightness, w, h, EDGE_BLUR_RADIUS);
    let gradient = Gradient::of(&blurred, w, h);
    let smooth = Smooth {
        values: &blurred,
        width: w,
        height: h,
    };
    let (fw, fh) = (f64::from(width), f64::from(height));

    let mut across = vec![Line::border(0.5, 0.0), Line::border(0.5, fh)];
    let mut along = vec![Line::border(0.0, 0.0), Line::border(0.0, fw)];
    for line in hough_lines(&gradient) {
        let list = if line.is_across() {
            &mut across
        } else {
            &mut along
        };
        if list.len() < LINES_PER_DIRECTION + 2 {
            list.push(line);
        }
    }
    // Top to bottom, left to right, through the middle of the photo.
    across.sort_by(|a, b| a.y_at(fw / 2.0).total_cmp(&b.y_at(fw / 2.0)));
    along.sort_by(|a, b| a.x_at(fh / 2.0).total_cmp(&b.x_at(fh / 2.0)));

    let tolerance = OUTSIDE_TOLERANCE * fw.max(fh);
    let inside = |&(x, y): &(f64, f64)| {
        (-tolerance..=fw + tolerance).contains(&x) && (-tolerance..=fh + tolerance).contains(&y)
    };
    let paper = PaperMap::of(&blurred, w, h);
    let mut best: Option<(i64, [Point; 4])> = None;
    for (i, top) in across.iter().enumerate() {
        for bottom in &across[i + 1..] {
            for (j, left) in along.iter().enumerate() {
                for right in &along[j + 1..] {
                    let sides = [top, right, bottom, left];
                    // The whole photo or a strip of it is no suggestion.
                    if sides.iter().filter(|l| l.border).count() > 2 {
                        continue;
                    }
                    let (Some(a), Some(b), Some(c), Some(d)) = (
                        top.meet(left),
                        top.meet(right),
                        bottom.meet(right),
                        bottom.meet(left),
                    ) else {
                        continue;
                    };
                    let corners = [a, b, c, d];
                    if !corners.iter().all(inside) {
                        continue;
                    }
                    let corners = corners.map(|(x, y)| (x.clamp(0.0, fw), y.clamp(0.0, fh)));
                    let points = corners.map(|(x, y)| Point::new(x / fw, y / fh));
                    let edit = ImageEdit {
                        corners: points,
                        ..ImageEdit::default()
                    };
                    if !edit.is_valid() || area(&points) < MIN_RECEIPT_SHARE {
                        continue;
                    }
                    // A desk or a laptop around the receipt has straight
                    // edges too, a block of print inside it as well: the
                    // receipt is the shape that covers the paper best.
                    let score = paper.score(&corners);
                    if best.is_some_and(|(b, _)| score <= b) {
                        continue;
                    }
                    let centre = (
                        corners.iter().map(|c| c.0).sum::<f64>() / 4.0,
                        corners.iter().map(|c| c.1).sum::<f64>() / 4.0,
                    );
                    let on_edges = sides.iter().enumerate().all(|(k, side)| {
                        side.border
                            || step_share(&smooth, corners[k], corners[(k + 1) % 4], centre)
                                >= MIN_SUPPORT
                    });
                    if on_edges {
                        best = Some((score, points));
                    }
                }
            }
        }
    }
    with_margin(squared_ends(best?.1, width, height))
}

/// Blur before the gradient, in pixels of the [`DETECT_SIDE`] image.
const EDGE_BLUR_RADIUS: usize = 2;

/// Smallest brightness step per pixel that counts as an edge: a paper edge
/// on a desk of nearly the same brightness still steps faster than shadows
/// or uneven light.
const MIN_EDGE: f32 = 1.5;

/// Angle steps of the Hough transform: one per degree.
const ANGLE_STEPS: usize = 180;

/// How many steps a line may differ from a pixel's gradient direction and
/// still get its vote.
const VOTE_SPREAD: usize = 5;

/// Lines tried per direction (across, along) besides the photo's edges.
const LINES_PER_DIRECTION: usize = 10;

/// Most a receipt's edge may be tilted from the photo's edges, in degrees;
/// beyond, across and along can no longer be told apart.
const MAX_TILT: usize = 40;

/// How far a corner may lie outside the photo, as a share of its longest
/// side, before the lines are taken for something else.
const OUTSIDE_TOLERANCE: f64 = 0.03;

/// Smallest share of the photo a receipt found by its edges may cover.
const MIN_RECEIPT_SHARE: f64 = 0.05;

/// Share of a side along which the paper must be brighter than outside
/// (see [`step_share`]).
const MIN_SUPPORT: f64 = 0.6;

/// Brightness gradient of a row-major grid (central differences).
struct Gradient {
    width: usize,
    height: usize,
    dx: Vec<f32>,
    dy: Vec<f32>,
}

impl Gradient {
    fn of(values: &[u8], width: usize, height: usize) -> Self {
        let mut dx = vec![0f32; values.len()];
        let mut dy = vec![0f32; values.len()];
        let at = |x: usize, y: usize| f32::from(values[y * width + x]);
        for y in 1..height.saturating_sub(1) {
            for x in 1..width.saturating_sub(1) {
                dx[y * width + x] = (at(x + 1, y) - at(x - 1, y)) / 2.0;
                dy[y * width + x] = (at(x, y + 1) - at(x, y - 1)) / 2.0;
            }
        }
        Self {
            width,
            height,
            dx,
            dy,
        }
    }
}

/// A straight line `x·cos θ + y·sin θ = distance` in pixels of the
/// [`DETECT_SIDE`] image; `angle` θ in half turns (`0.0..1.0`).
#[derive(Debug, Clone, Copy)]
struct Line {
    angle: f64,
    distance: f64,
    /// One of the photo's own edges.
    border: bool,
}

impl Line {
    fn border(angle: f64, distance: f64) -> Self {
        Self {
            angle,
            distance,
            border: true,
        }
    }

    fn normal(&self) -> (f64, f64) {
        let radians = self.angle * std::f64::consts::PI;
        (radians.cos(), radians.sin())
    }

    /// Runs across the photo: its normal points rather up or down.
    fn is_across(&self) -> bool {
        let (c, s) = self.normal();
        s.abs() > c.abs()
    }

    fn y_at(&self, x: f64) -> f64 {
        let (c, s) = self.normal();
        (self.distance - x * c) / s
    }

    fn x_at(&self, y: f64) -> f64 {
        let (c, s) = self.normal();
        (self.distance - y * s) / c
    }

    /// Where the two lines cross; `None` if they run side by side.
    fn meet(&self, other: &Self) -> Option<(f64, f64)> {
        let ((c1, s1), (c2, s2)) = (self.normal(), other.normal());
        let determinant = c1 * s2 - s1 * c2;
        if determinant.abs() < 1e-6 {
            return None;
        }
        Some((
            (self.distance * s2 - other.distance * s1) / determinant,
            (c1 * other.distance - c2 * self.distance) / determinant,
        ))
    }
}

/// Straight lines along which many pixels step in brightness, strongest
/// first; each pixel votes only for lines at right angles to its gradient.
/// Lines tilted more than [`MAX_TILT`] from the photo's edges are left out.
fn hough_lines(gradient: &Gradient) -> Vec<Line> {
    use std::f64::consts::PI;

    let (w, h) = (gradient.width, gradient.height);
    let diagonal = (w as f64).hypot(h as f64).ceil() as usize;
    let distances = 2 * diagonal + 1;
    let trig: Vec<(f64, f64)> = (0..ANGLE_STEPS)
        .map(|a| {
            let radians = a as f64 / ANGLE_STEPS as f64 * PI;
            (radians.cos(), radians.sin())
        })
        .collect();
    let mut votes = vec![0u32; ANGLE_STEPS * distances];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let (gx, gy) = (gradient.dx[i], gradient.dy[i]);
            if gx.hypot(gy) < MIN_EDGE {
                continue;
            }
            // The line's normal is the gradient, pointing either way.
            let direction = f64::from(gy).atan2(f64::from(gx)).rem_euclid(PI);
            let step = (direction / PI * ANGLE_STEPS as f64).round() as usize;
            for offset in 0..=2 * VOTE_SPREAD {
                let a = (step + ANGLE_STEPS + offset - VOTE_SPREAD) % ANGLE_STEPS;
                let (c, s) = trig[a];
                let distance = (x as f64 + 0.5) * c + (y as f64 + 0.5) * s;
                let d = (distance.round() as i64 + diagonal as i64) as usize;
                votes[a * distances + d] += 1;
            }
        }
    }
    // A real edge runs along a good part of the shorter side.
    let min_votes = (w.min(h) / 8).max(10) as u32;
    let near_an_edge = |a: usize| {
        let degrees = a * 180 / ANGLE_STEPS;
        degrees <= MAX_TILT || degrees >= 180 - MAX_TILT || degrees.abs_diff(90) <= MAX_TILT
    };
    let mut peaks = Vec::new();
    for a in (0..ANGLE_STEPS).filter(|&a| near_an_edge(a)) {
        for d in 0..distances {
            let v = votes[a * distances + d];
            if v < min_votes {
                continue;
            }
            // Strongest within a few degrees and pixels; of equal ones the
            // first, so a flat top gives one peak.
            let strongest = (-3i64..=3).all(|da| {
                let na = (a as i64 + da).rem_euclid(ANGLE_STEPS as i64) as usize;
                (-4i64..=4).all(|dd| {
                    let nd = d as i64 + dd;
                    if !(0..distances as i64).contains(&nd) || (da, dd) == (0, 0) {
                        return true;
                    }
                    let other = votes[na * distances + nd as usize];
                    other < v || (other == v && (na, nd as usize) > (a, d))
                })
            });
            if strongest {
                peaks.push((v, a, d));
            }
        }
    }
    peaks.sort_by_key(|&(votes, _, _)| std::cmp::Reverse(votes));
    peaks
        .into_iter()
        .map(|(_, a, d)| Line {
            angle: a as f64 / ANGLE_STEPS as f64,
            distance: d as f64 - diagonal as f64,
            border: false,
        })
        .collect()
}

/// Share of the side from `a` to `b` where the paper (towards `centre`)
/// is clearly brighter than what lies outside, compared in bands a few
/// pixels to either side. A line of print or a fold has paper on both
/// sides and reaches little; a paper edge reaches most of its length.
fn step_share(smooth: &Smooth, a: (f64, f64), b: (f64, f64), centre: (f64, f64)) -> f64 {
    let length = (b.0 - a.0).hypot(b.1 - a.1);
    if length < 1.0 {
        return 0.0;
    }
    let mut normal = (-(b.1 - a.1) / length, (b.0 - a.0) / length);
    let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    if (centre.0 - middle.0) * normal.0 + (centre.1 - middle.1) * normal.1 < 0.0 {
        normal = (-normal.0, -normal.1);
    }
    let band = |x: f64, y: f64, sign: f64| {
        let total = BAND
            .iter()
            .map(|d| smooth.at(x + sign * normal.0 * d, y + sign * normal.1 * d))
            .sum::<Option<f64>>()?;
        Some(total / BAND.len() as f64)
    };
    let samples = length.ceil() as usize;
    let (mut compared, mut stepping) = (0usize, 0usize);
    for k in 0..samples {
        let t = (k as f64 + 0.5) / samples as f64;
        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        // Close to the photo's edge there is nothing outside to compare.
        let (Some(inside), Some(outside)) = (band(x, y, 1.0), band(x, y, -1.0)) else {
            continue;
        };
        compared += 1;
        stepping += usize::from(inside - outside >= MIN_STEP);
    }
    if compared * 2 < samples {
        return 0.0;
    }
    stepping as f64 / compared as f64
}

/// Distances in pixels of the [`DETECT_SIDE`] image at which
/// [`step_share`] compares inside and outside: past the blur and the
/// uncertainty of the line, still close to the edge.
const BAND: [f64; 5] = [4.0, 6.0, 8.0, 10.0, 12.0];

/// How much brighter the paper must be than what lies outside it.
const MIN_STEP: f64 = 8.0;

/// Which pixels look like paper: within [`PAPER_RANGE`] of the brightest
/// ones (receipts are the brightest thing in a photo of one), as row-wise
/// running sums of +1 for paper and -1 for anything else.
struct PaperMap {
    width: usize,
    height: usize,
    sums: Vec<i64>,
}

impl PaperMap {
    fn of(values: &[u8], width: usize, height: usize) -> Self {
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let brightest = sorted[(sorted.len() - 1) * 98 / 100];
        let threshold = brightest.saturating_sub(PAPER_RANGE);
        let stride = width + 1;
        let mut sums = vec![0i64; stride * height];
        for y in 0..height {
            for x in 0..width {
                let value = if values[y * width + x] >= threshold {
                    1
                } else {
                    -1
                };
                sums[y * stride + x + 1] = sums[y * stride + x] + value;
            }
        }
        Self {
            width,
            height,
            sums,
        }
    }

    /// Paper pixels inside the convex `corners` less the other pixels
    /// inside.
    fn score(&self, corners: &[(f64, f64); 4]) -> i64 {
        let stride = self.width + 1;
        let mut total = 0;
        for y in 0..self.height {
            let centre = y as f64 + 0.5;
            // Where the row's centre line crosses the four sides.
            let (mut left, mut right) = (f64::MAX, f64::MIN);
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                if (a.1 - centre) * (b.1 - centre) > 0.0 || a.1 == b.1 {
                    continue;
                }
                let x = a.0 + (b.0 - a.0) * (centre - a.1) / (b.1 - a.1);
                (left, right) = (left.min(x), right.max(x));
            }
            if left > right {
                continue;
            }
            // Pixels whose centre lies in `left..right`.
            let from = (left - 0.5).ceil().clamp(0.0, self.width as f64) as usize;
            let to = ((right - 0.5).floor() + 1.0).clamp(0.0, self.width as f64) as usize;
            if from < to {
                total += self.sums[y * stride + to] - self.sums[y * stride + from];
            }
        }
        total
    }
}

/// How much darker than the brightest pixels paper may be: shade across
/// the receipt, not yet the desk under it.
const PAPER_RANGE: u8 = 20;

/// The blurred brightness [`detect_corners_by_edges`] works on.
struct Smooth<'a> {
    values: &'a [u8],
    width: usize,
    height: usize,
}

impl Smooth<'_> {
    /// Brightness at a point; `None` outside the image.
    fn at(&self, x: f64, y: f64) -> Option<f64> {
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let (x, y) = (x as usize, y as usize);
        (x < self.width && y < self.height).then(|| f64::from(self.values[y * self.width + x]))
    }
}

/// Area enclosed by the corners (shoelace formula).
fn area(corners: &[Point; 4]) -> f64 {
    let twice: f64 = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    twice.abs() / 2.0
}

/// A perspective transform of the plane: `(x, y)` maps to
/// `((h0·x + h1·y + h2) / d, (h3·x + h4·y + h5) / d)` with
/// `d = h6·x + h7·y + 1`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Homography([f64; 8]);

impl Homography {
    /// The transform taking each point of `from` to the same point of `to`;
    /// `None` if three of them lie on a line.
    fn from_points(from: &[(f64, f64); 4], to: &[(f64, f64); 4]) -> Option<Self> {
        // Two linear equations per point pair in the eight unknowns.
        let mut rows = [[0.0f64; 9]; 8];
        for (i, (&(x, y), &(u, v))) in from.iter().zip(to).enumerate() {
            rows[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            rows[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        let h = solve(rows)?;
        // A solution can still squash the plane onto a line; that has no
        // inverse and shows nothing.
        let determinant = h[0] * (h[4] - h[5] * h[7]) - h[1] * (h[3] - h[5] * h[6])
            + h[2] * (h[3] * h[7] - h[4] * h[6]);
        (determinant.abs() > 1e-12).then_some(Self(h))
    }

    fn map(&self, x: f64, y: f64) -> (f64, f64) {
        let h = &self.0;
        let d = h[6] * x + h[7] * y + 1.0;
        (
            (h[0] * x + h[1] * y + h[2]) / d,
            (h[3] * x + h[4] * y + h[5]) / d,
        )
    }
}

/// Solves eight linear equations (last column: right-hand side) by
/// Gaussian elimination with partial pivoting.
fn solve(mut rows: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for column in 0..8 {
        let pivot =
            (column..8).max_by(|&a, &b| rows[a][column].abs().total_cmp(&rows[b][column].abs()))?;
        // Coordinates are pixels, so anything this small is a degenerate
        // set of points, not rounding.
        if rows[pivot][column].abs() < 1e-9 {
            return None;
        }
        rows.swap(column, pivot);
        let pivot_row = rows[column];
        for (index, row) in rows.iter_mut().enumerate() {
            if index != column {
                let factor = row[column] / pivot_row[column];
                for (value, &pivot_value) in row.iter_mut().zip(&pivot_row).skip(column) {
                    *value -= factor * pivot_value;
                }
            }
        }
    }
    let mut result = [0.0; 8];
    for (i, value) in result.iter_mut().enumerate() {
        *value = rows[i][8] / rows[i][i];
    }
    Some(result)
}

/// Fills a `width` × `height` image with the colours `homography` points
/// to in `source`, smoothed between neighbouring pixels (bilinear).
fn warp(source: &RgbImage, homography: &Homography, width: u32, height: u32) -> RgbImage {
    let (max_x, max_y) = (source.width() - 1, source.height() - 1);
    RgbImage::from_fn(width, height, |x, y| {
        // Pixel centres: pixel 0 covers 0.0..1.0.
        let (u, v) = homography.map(f64::from(x) + 0.5, f64::from(y) + 0.5);
        let (u, v) = (
            (u - 0.5).clamp(0.0, f64::from(max_x)),
            (v - 0.5).clamp(0.0, f64::from(max_y)),
        );
        let (x0, y0) = (u.floor() as u32, v.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(max_x), (y0 + 1).min(max_y));
        let (fx, fy) = (u - f64::from(x0), v - f64::from(y0));
        let (a, b) = (source.get_pixel(x0, y0).0, source.get_pixel(x1, y0).0);
        let (c, d) = (source.get_pixel(x0, y1).0, source.get_pixel(x1, y1).0);
        let channel = |i: usize| {
            let top = f64::from(a[i]) * (1.0 - fx) + f64::from(b[i]) * fx;
            let bottom = f64::from(c[i]) * (1.0 - fx) + f64::from(d[i]) * fx;
            (top * (1.0 - fy) + bottom * fy).round() as u8
        };
        Rgb([channel(0), channel(1), channel(2)])
    })
}

/// Width and height of a receipt's first page as it is shown, turned
/// upright by its EXIF orientation. Blocking: reads the file's header.
pub fn page_size(data_dir: &Path, receipt: &ReceiptFiles) -> Result<(u32, u32), ReceiptError> {
    let path = receipt
        .image_paths
        .first()
        .ok_or_else(|| ReceiptError::Image("the receipt has no image".to_string()))?;
    let bytes = std::fs::read(data_dir.join(path))?;
    let image = decode_upright(&bytes)?;
    Ok((image.width(), image.height()))
}

/// Whether the corners are suggested as soon as a photo is adjusted
/// (setting `receipt_auto_corners`, on unless switched off).
pub fn auto_corners(db: &Db) -> Result<bool, StorageError> {
    Ok(db.setting(RECEIPT_AUTO_CORNERS)?.as_deref() != Some("off"))
}

pub fn set_auto_corners(db: &Db, on: bool) -> Result<(), StorageError> {
    db.set_setting(RECEIPT_AUTO_CORNERS, if on { "on" } else { "off" })
}

/// How the corners are found in a photo (setting `receipt_corner_method`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CornerMethod {
    /// The largest bright area and its outermost points
    /// ([`detect_corners`]); needs a background darker than the paper.
    #[default]
    Corners,
    /// The paper's straight edges ([`detect_corners_by_edges`]); also on a
    /// bright desk.
    PaperEdges,
}

impl CornerMethod {
    pub const ALL: [Self; 2] = [Self::Corners, Self::PaperEdges];

    pub fn code(self) -> &'static str {
        match self {
            Self::Corners => "corners",
            Self::PaperEdges => "paper_edges",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.code() == code)
    }

    /// Suggested corners of the receipt in an upright photo.
    pub fn detect(self, photo: &RgbImage) -> Option<[Point; 4]> {
        match self {
            Self::Corners => detect_corners(photo),
            Self::PaperEdges => detect_corners_by_edges(photo),
        }
    }
}

/// The chosen [`CornerMethod`]; unset or unknown counts as the default.
pub fn corner_method(db: &Db) -> Result<CornerMethod, StorageError> {
    Ok(db
        .setting(RECEIPT_CORNER_METHOD)?
        .as_deref()
        .and_then(CornerMethod::from_code)
        .unwrap_or_default())
}

pub fn set_corner_method(db: &Db, method: CornerMethod) -> Result<(), StorageError> {
    db.set_setting(RECEIPT_CORNER_METHOD, method.code())
}

/// Suggested corners of the receipt's first page in the upright photo,
/// found by `method`. Blocking: decodes the full photo.
pub fn detect_page_corners(
    data_dir: &Path,
    receipt: &ReceiptFiles,
    method: CornerMethod,
) -> Result<Option<[Point; 4]>, ReceiptError> {
    let path = receipt
        .image_paths
        .first()
        .ok_or_else(|| ReceiptError::Image("the receipt has no image".to_string()))?;
    let bytes = std::fs::read(data_dir.join(path))?;
    Ok(method.detect(&decode_upright(&bytes)?.into_rgb8()))
}

/// Applies `edit` to the original of the receipt's first page and stores
/// the result with a new thumbnail (RCP-05). An unchanged edit stores
/// nothing. Blocking: decodes and encodes a full photo.
pub fn save_edit(
    db: &Db,
    data_dir: &Path,
    receipt: &ReceiptFiles,
    edit: &ImageEdit,
) -> Result<ReceiptFiles, ReceiptError> {
    if edit.is_unchanged() {
        return Ok(receipt.clone());
    }
    // Always from the original, so a correction never stacks on another.
    let original = receipt
        .image_paths
        .first()
        .ok_or_else(|| ReceiptError::Image("the receipt has no image".to_string()))?;
    let bytes = std::fs::read(data_dir.join(original))?;
    let image = decode_upright(&bytes)?.into_rgb8();
    let edited = edit
        .apply(&image)
        .ok_or_else(|| ReceiptError::Image("the corners enclose no area".to_string()))?;

    let stem = Path::new(original)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&receipt.id);
    let edited_path = format!("{RECEIPTS_DIR}/{stem}_edited.jpg");
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 90).encode_image(&edited)?;
    std::fs::write(data_dir.join(&edited_path), jpeg)?;

    let thumbnail_path = format!("{RECEIPTS_DIR}/{stem}_edited_thumb.jpg");
    std::fs::write(
        data_dir.join(&thumbnail_path),
        thumbnail_of(&image::DynamicImage::ImageRgb8(edited))?,
    )?;
    Ok(db.save_receipt_edit(&receipt.id, &edited_path, Some(&thumbnail_path))?)
}

/// Test helper: a `width` × `height` photo of `card` lying on a brown
/// table, its corners at `corners`, as if taken at an angle.
#[cfg(test)]
pub(crate) fn photograph_at_an_angle(
    card: &RgbImage,
    corners: &[Point; 4],
    width: u32,
    height: u32,
) -> RgbImage {
    photograph_on(card, corners, width, height, |_, _| Rgb([90, 60, 40]))
}

/// Like [`photograph_at_an_angle`], on a table coloured by `table` at each
/// pixel of the photo.
#[cfg(test)]
fn photograph_on(
    card: &RgbImage,
    corners: &[Point; 4],
    width: u32,
    height: u32,
    table: impl Fn(u32, u32) -> Rgb<u8>,
) -> RgbImage {
    let (w, h) = (f64::from(width), f64::from(height));
    let (cw, ch) = (f64::from(card.width()), f64::from(card.height()));
    let in_photo = corners.map(|p| (p.x * w, p.y * h));
    let card_corners = [(0.0, 0.0), (cw, 0.0), (cw, ch), (0.0, ch)];
    let Some(to_card) = Homography::from_points(&in_photo, &card_corners) else {
        return RgbImage::new(width, height);
    };
    let inside = warp(card, &to_card, width, height);
    RgbImage::from_fn(width, height, |x, y| {
        let (u, v) = to_card.map(f64::from(x) + 0.5, f64::from(y) + 0.5);
        if (0.0..cw).contains(&u) && (0.0..ch).contains(&v) {
            *inside.get_pixel(x, y)
        } else {
            table(x, y)
        }
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat};

    use super::*;

    /// A test card: dark frame, white inside, a black bar near the top
    /// left, so position and orientation are both visible.
    fn card(width: u32, height: u32) -> RgbImage {
        RgbImage::from_fn(width, height, |x, y| {
            let frame = x < 4 || y < 4 || x >= width - 4 || y >= height - 4;
            let bar = (10..30).contains(&x) && (10..14).contains(&y);
            if frame || bar {
                Rgb([0, 0, 0])
            } else {
                Rgb([255, 255, 255])
            }
        })
    }

    fn mean_difference(a: &RgbImage, b: &RgbImage) -> f64 {
        assert_eq!(a.dimensions(), b.dimensions());
        let total: u64 = a
            .pixels()
            .zip(b.pixels())
            .map(|(p, q)| u64::from(p.0[0].abs_diff(q.0[0])))
            .sum();
        total as f64 / f64::from(a.width() * a.height())
    }

    #[test]
    fn homography_maps_each_corner_onto_its_target() {
        let from = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)];
        let to = [(12.0, 7.0), (90.0, 15.0), (80.0, 70.0), (5.0, 60.0)];
        let h = Homography::from_points(&from, &to).unwrap();
        for (&(x, y), &(u, v)) in from.iter().zip(&to) {
            let (mu, mv) = h.map(x, y);
            assert!((mu - u).abs() < 1e-6 && (mv - v).abs() < 1e-6, "{mu},{mv}");
        }
        // Three corners on one line give no transform.
        let line = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (0.0, 5.0)];
        assert_eq!(Homography::from_points(&from, &line), None);
    }

    #[test]
    fn unchanged_edit_returns_the_photo_as_it_is() {
        let photo = card(60, 40);
        let edit = ImageEdit::default();
        assert!(edit.is_unchanged());
        assert_eq!(edit.apply(&photo), Some(photo));
    }

    #[test]
    fn quarter_turns_turn_photo_and_corners_together() {
        let photo = card(60, 40);
        let turned = ImageEdit::default().turned_clockwise();
        assert_eq!(turned.quarter_turns, 1);
        assert_eq!(turned.corners, FULL);
        let result = turned.apply(&photo).unwrap();
        assert_eq!(result.dimensions(), (40, 60));
        // The bar near the top left is now near the top right.
        assert_eq!(result.get_pixel(40 - 12, 15).0, [0, 0, 0]);
        assert_eq!(result.get_pixel(12, 15).0, [255, 255, 255]);

        // A corner keeps marking the same spot of the receipt.
        let crop = ImageEdit {
            quarter_turns: 0,
            contrast: false,
            sharpen: false,
            corners: [
                Point::new(0.1, 0.2),
                Point::new(0.9, 0.2),
                Point::new(0.9, 0.8),
                Point::new(0.1, 0.8),
            ],
        };
        let close = |a: Point, b: Point| (a.x - b.x).abs() < 1e-12 && (a.y - b.y).abs() < 1e-12;
        let once = crop.turned_clockwise();
        assert!(close(once.corners[0], Point::new(0.2, 0.1)));
        assert!(close(once.corners[1], Point::new(0.8, 0.1)));
        let all_round = once
            .turned_clockwise()
            .turned_clockwise()
            .turned_clockwise();
        assert_eq!(all_round.quarter_turns, 0);
        for (&a, &b) in all_round.corners.iter().zip(&crop.corners) {
            assert!(close(a, b));
        }
        assert_eq!(once.turned_counter_clockwise().quarter_turns, 0);
    }

    #[test]
    fn rectangular_corners_crop() {
        let photo = card(100, 80);
        let edit = ImageEdit {
            quarter_turns: 0,
            contrast: false,
            sharpen: false,
            corners: [
                Point::new(0.0, 0.0),
                Point::new(0.5, 0.0),
                Point::new(0.5, 0.5),
                Point::new(0.0, 0.5),
            ],
        };
        let result = edit.apply(&photo).unwrap();
        assert_eq!(result.dimensions(), (50, 40));
        let expected = image::imageops::crop_imm(&photo, 0, 0, 50, 40).to_image();
        assert_eq!(result, expected);
    }

    #[test]
    fn a_photo_taken_at_an_angle_is_straightened() {
        // Put the card into a perspective (narrower at the top, as when
        // the phone is tilted), then straighten it with the four corners.
        let flat = card(200, 300);
        let (w, h) = (400.0, 400.0);
        let corners = [
            Point::new(0.30, 0.10),
            Point::new(0.70, 0.12),
            Point::new(0.85, 0.90),
            Point::new(0.12, 0.88),
        ];
        let photo = photograph_at_an_angle(&flat, &corners, w as u32, h as u32);

        let edit = ImageEdit {
            quarter_turns: 0,
            contrast: false,
            sharpen: false,
            corners,
        };
        assert!(edit.is_valid());
        let straightened = edit.apply(&photo).unwrap();
        let back = image::imageops::resize(
            &straightened,
            200,
            300,
            image::imageops::FilterType::Triangle,
        );
        // Without the correction the photo has nothing in common with the
        // card; with it only resampling blur remains.
        let distorted =
            image::imageops::resize(&photo, 200, 300, image::imageops::FilterType::Triangle);
        let before = mean_difference(&distorted, &flat);
        let after = mean_difference(&back, &flat);
        assert!(after < 15.0, "after {after}");
        assert!(after * 4.0 < before, "before {before}, after {after}");
        // The bar is back at the top left.
        assert!(back.get_pixel(20, 12).0[0] < 100);
    }

    #[test]
    fn crossed_or_tiny_corners_are_refused() {
        let photo = card(40, 40);
        let crossed = ImageEdit {
            quarter_turns: 0,
            contrast: false,
            sharpen: false,
            corners: [
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(1.0, 0.0),
                Point::new(0.0, 1.0),
            ],
        };
        assert!(!crossed.is_valid());
        assert_eq!(crossed.apply(&photo), None);
        let tiny = ImageEdit {
            quarter_turns: 0,
            contrast: false,
            sharpen: false,
            corners: [
                Point::new(0.5, 0.5),
                Point::new(0.55, 0.5),
                Point::new(0.55, 0.55),
                Point::new(0.5, 0.55),
            ],
        };
        assert!(!tiny.is_valid());
        // Counter-clockwise order is a mirrored receipt, not a valid one.
        let mut mirrored = ImageEdit::default();
        mirrored.corners.reverse();
        assert!(!mirrored.is_valid());
    }

    #[test]
    fn saving_an_edit_keeps_the_original_file() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("invuso-edit-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(dir.join(RECEIPTS_DIR)).unwrap();
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(card(80, 60))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        std::fs::write(dir.join("receipts/r1.png"), &png).unwrap();
        let receipt = db.create_receipt("receipts/r1.png", None).unwrap();
        assert_eq!(page_size(&dir, &receipt).unwrap(), (80, 60));

        // Nothing changed: nothing written.
        let same = save_edit(&db, &dir, &receipt, &ImageEdit::default()).unwrap();
        assert_eq!(same, receipt);
        assert!(!dir.join("receipts/r1_edited.jpg").exists());

        let edit = ImageEdit::default().turned_clockwise();
        let edited = save_edit(&db, &dir, &receipt, &edit).unwrap();
        assert_eq!(std::fs::read(dir.join("receipts/r1.png")).unwrap(), png);
        assert_eq!(edited.image_paths, ["receipts/r1.png"]);
        assert_eq!(edited.page(0), Some("receipts/r1_edited.jpg"));
        assert_eq!(
            edited.thumbnail_path.as_deref(),
            Some("receipts/r1_edited_thumb.jpg")
        );
        let stored = image::open(dir.join("receipts/r1_edited.jpg")).unwrap();
        assert_eq!((stored.width(), stored.height()), (60, 80));
        assert!(dir.join("receipts/r1_edited_thumb.jpg").exists());
        assert_eq!(db.receipt(&receipt.id).unwrap(), Some(edited));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corners_of_paper_on_a_darker_table_are_found() {
        let corners = [
            Point::new(0.30, 0.10),
            Point::new(0.70, 0.12),
            Point::new(0.85, 0.90),
            Point::new(0.12, 0.88),
        ];
        let photo = photograph_at_an_angle(&card(400, 600), &corners, 800, 800);
        let found = detect_corners(&photo).unwrap();
        for (f, c) in found.iter().zip(&corners) {
            // The card's black frame, the downscaling and the margin move
            // them a bit outward.
            assert!(
                (f.x - c.x).abs() < 0.06 && (f.y - c.y).abs() < 0.06,
                "{f:?} vs {c:?}"
            );
        }

        // No paper against a darker background: nothing to suggest.
        assert_eq!(
            detect_corners(&RgbImage::from_pixel(50, 50, Rgb([240, 240, 240]))),
            None
        );
    }

    /// Prints the corners found in the user's own photos (paths in
    /// `INVUSO_PHOTOS`, separated by `;`), to check them by eye; with
    /// `INVUSO_CORNERS_OUT` set to a folder, also draws them into a copy.
    /// `INVUSO_CORNER_METHOD` picks the method by its code.
    #[test]
    #[ignore = "needs photos named in INVUSO_PHOTOS"]
    fn detects_corners_of_own_photos() {
        let paths = std::env::var("INVUSO_PHOTOS").unwrap();
        let out = std::env::var("INVUSO_CORNERS_OUT").ok();
        let method = std::env::var("INVUSO_CORNER_METHOD")
            .ok()
            .and_then(|code| CornerMethod::from_code(&code))
            .unwrap_or_default();
        for path in paths.split(';') {
            let bytes = std::fs::read(path).unwrap();
            let photo = decode_upright(&bytes).unwrap().into_rgb8();
            let corners = method.detect(&photo);
            println!("{path}: {corners:?}");
            let (Some(out), Some(corners)) = (&out, corners) else {
                continue;
            };
            let mut drawn =
                image::imageops::thumbnail(&photo, photo.width() / 4, photo.height() / 4);
            let (w, h) = (f64::from(drawn.width()), f64::from(drawn.height()));
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                for step in 0..=400 {
                    let t = f64::from(step) / 400.0;
                    let (x, y) = ((a.x + (b.x - a.x) * t) * w, (a.y + (b.y - a.y) * t) * h);
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (px, py) = (x as u32 + dx, y as u32 + dy);
                        if px < drawn.width() && py < drawn.height() {
                            drawn.put_pixel(px, py, Rgb([255, 0, 0]));
                        }
                    }
                }
            }
            let name = Path::new(path).file_stem().unwrap().to_string_lossy();
            drawn
                .save(Path::new(out).join(format!("{name}.{}.png", method.code())))
                .unwrap();
        }
    }

    /// A receipt without a frame: off-white paper with rows of grey print.
    fn receipt_paper(width: u32, height: u32) -> RgbImage {
        RgbImage::from_fn(width, height, |x, y| {
            let row = y % 24 >= 10 && y % 24 < 18 && y > 20 && y < height - 20;
            let letter = x > 20 && x < width - 20 && (x / 6) % 5 != 0 && (x * 7 + y) % 11 < 7;
            if row && letter {
                Rgb([60, 60, 60])
            } else {
                Rgb([250, 250, 248])
            }
        })
    }

    fn assert_close(found: &[Point; 4], expected: &[Point; 4]) {
        for (f, c) in found.iter().zip(expected) {
            // The downscaling and the margin move them a bit outward.
            assert!(
                (f.x - c.x).abs() < 0.06 && (f.y - c.y).abs() < 0.06,
                "{found:?} vs {expected:?}"
            );
        }
    }

    const TILTED: [Point; 4] = [
        Point::new(0.30, 0.10),
        Point::new(0.70, 0.14),
        Point::new(0.66, 0.92),
        Point::new(0.24, 0.88),
    ];

    #[test]
    fn paper_edges_are_found_on_a_darker_table() {
        let photo = photograph_at_an_angle(&receipt_paper(400, 800), &TILTED, 800, 900);
        assert_close(&detect_corners_by_edges(&photo).unwrap(), &TILTED);
    }

    #[test]
    fn paper_edges_are_found_on_a_bright_desk_lit_from_the_side() {
        // Neutral grey desk, as bright as the paper towards the right: no
        // bright area stands out there, but the edge still steps.
        let photo = photograph_on(&receipt_paper(400, 800), &TILTED, 800, 900, |x, _| {
            let v = (200 + x * 50 / 800) as u8;
            Rgb([v, v, v])
        });
        assert_close(&detect_corners_by_edges(&photo).unwrap(), &TILTED);
    }

    #[test]
    fn paper_running_off_the_photo_ends_at_its_edge() {
        let long = [
            Point::new(0.30, -0.20),
            Point::new(0.68, -0.18),
            Point::new(0.70, 1.20),
            Point::new(0.32, 1.18),
        ];
        let photo = photograph_at_an_angle(&receipt_paper(400, 1200), &long, 800, 900);
        let found = detect_corners_by_edges(&photo).unwrap();
        let expected = [
            Point::new(0.30, 0.0),
            Point::new(0.68, 0.0),
            Point::new(0.70, 1.0),
            Point::new(0.32, 1.0),
        ];
        assert_close(&found, &expected);
    }

    #[test]
    fn no_paper_edge_suggests_nothing() {
        let plain = RgbImage::from_fn(300, 400, |x, _| {
            let v = (180 + x * 40 / 300) as u8;
            Rgb([v, v, v])
        });
        assert_eq!(detect_corners_by_edges(&plain), None);
    }

    #[test]
    fn a_torn_end_is_squared_outward_a_cut_one_kept() {
        // Upright sides; the top torn off at a slant (right end lower).
        let torn = [
            Point::new(0.2, 0.1),
            Point::new(0.6, 0.2),
            Point::new(0.6, 0.9),
            Point::new(0.2, 0.9),
        ];
        let squared = squared_ends(torn, 1000, 1000);
        let close = |a: Point, b: Point| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
        assert!(close(squared[0], Point::new(0.2, 0.1)));
        assert!(close(squared[1], Point::new(0.6, 0.1)), "{squared:?}");
        assert!(close(squared[2], torn[2]) && close(squared[3], torn[3]));

        // A few degrees off: crumpled, not torn; left as found.
        let cut = [
            Point::new(0.2, 0.1),
            Point::new(0.6, 0.12),
            Point::new(0.6, 0.9),
            Point::new(0.2, 0.9),
        ];
        assert_eq!(squared_ends(cut, 1000, 1000), cut);
    }

    #[test]
    fn corner_method_defaults_to_corner_detection() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(corner_method(&db).unwrap(), CornerMethod::Corners);
        set_corner_method(&db, CornerMethod::PaperEdges).unwrap();
        assert_eq!(corner_method(&db).unwrap(), CornerMethod::PaperEdges);
        db.set_setting(RECEIPT_CORNER_METHOD, "unknown").unwrap();
        assert_eq!(corner_method(&db).unwrap(), CornerMethod::Corners);
    }

    #[test]
    fn automatic_corners_are_on_until_switched_off() {
        let db = Db::open_in_memory().unwrap();
        assert!(auto_corners(&db).unwrap());
        set_auto_corners(&db, false).unwrap();
        assert!(!auto_corners(&db).unwrap());
        set_auto_corners(&db, true).unwrap();
        assert!(auto_corners(&db).unwrap());
    }

    #[test]
    fn corners_found_upright_follow_the_turn() {
        let upright = [
            Point::new(0.1, 0.2),
            Point::new(0.9, 0.2),
            Point::new(0.9, 0.8),
            Point::new(0.1, 0.8),
        ];
        let turned = ImageEdit {
            sharpen: true,
            ..ImageEdit::default()
        }
        .turned_clockwise();
        let edit = turned.with_upright_corners(upright);
        assert_eq!(edit.quarter_turns, 1);
        assert!(edit.sharpen);
        assert!((edit.corners[0].x - 0.2).abs() < 1e-12);
        assert!((edit.corners[0].y - 0.1).abs() < 1e-12);
        assert_eq!(edit.uncropped().corners, FULL);
        assert_eq!(edit.uncropped().quarter_turns, 1);
    }

    #[test]
    fn contrast_evens_out_a_shadow() {
        // Paper from dark (shadow, left) to bright, print at half its
        // brightness in every column.
        let photo = RgbImage::from_fn(200, 100, |x, y| {
            let paper = 110 + (x * 130 / 199) as u8;
            let v = if y % 10 == 5 { paper / 2 } else { paper };
            Rgb([v, v, v])
        });
        let mut flat = photo.clone();
        flatten_contrast(&mut flat);
        let (left, right) = (flat.get_pixel(20, 2).0[0], flat.get_pixel(180, 2).0[0]);
        assert!(left.abs_diff(right) < 20, "paper {left} vs {right}");
        let (ink_left, ink_right) = (flat.get_pixel(20, 5).0[0], flat.get_pixel(180, 5).0[0]);
        assert!(
            ink_left.abs_diff(ink_right) < 20,
            "print {ink_left} vs {ink_right}"
        );
        assert!(left > ink_left + 80);

        let unchanged = ImageEdit::default();
        let filtered = ImageEdit {
            contrast: true,
            ..unchanged
        };
        assert!(!filtered.is_unchanged());
        assert_eq!(filtered.apply(&photo), Some(flat));
    }

    #[test]
    fn sharpening_steepens_an_edge() {
        // A slightly soft step from grey to white.
        let photo = RgbImage::from_fn(40, 10, |x, _| {
            let v = match x {
                0..19 => 100,
                19 => 150,
                _ => 200,
            };
            Rgb([v, v, v])
        });
        let edit = ImageEdit {
            sharpen: true,
            ..ImageEdit::default()
        };
        let sharp = edit.apply(&photo).unwrap();
        assert_eq!(sharp.dimensions(), photo.dimensions());
        // Darker just before the step, brighter just after it.
        assert!(sharp.get_pixel(18, 5).0[0] < photo.get_pixel(18, 5).0[0]);
        assert!(sharp.get_pixel(20, 5).0[0] > photo.get_pixel(20, 5).0[0]);
    }
}
