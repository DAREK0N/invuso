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
use crate::storage::{Db, RECEIPT_AUTO_CORNERS, ReceiptFiles, StorageError};

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
    let longest = photo.width().max(photo.height());
    let small = if longest > DETECT_SIDE {
        image::imageops::thumbnail(
            photo,
            (photo.width() * DETECT_SIDE / longest).max(1),
            (photo.height() * DETECT_SIDE / longest).max(1),
        )
    } else {
        photo.clone()
    };
    let (width, height) = small.dimensions();
    let (w, h) = (width as usize, height as usize);
    // Paper is bright in every channel; light wood or skin is bright too,
    // but not in blue. Blurred, so the print does not cut holes into it.
    let whiteness: Vec<u8> = small
        .pixels()
        .map(|p| p.0.into_iter().min().unwrap_or(0))
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
    let edit = ImageEdit {
        corners,
        ..ImageEdit::default()
    };
    edit.is_valid().then_some(corners)
}

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

/// Suggested corners of the receipt's first page in the upright photo
/// (see [`detect_corners`]). Blocking: decodes the full photo.
pub fn detect_page_corners(
    data_dir: &Path,
    receipt: &ReceiptFiles,
) -> Result<Option<[Point; 4]>, ReceiptError> {
    let path = receipt
        .image_paths
        .first()
        .ok_or_else(|| ReceiptError::Image("the receipt has no image".to_string()))?;
    let bytes = std::fs::read(data_dir.join(path))?;
    Ok(detect_corners(&decode_upright(&bytes)?.into_rgb8()))
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
    let (w, h) = (f64::from(width), f64::from(height));
    let (cw, ch) = (f64::from(card.width()), f64::from(card.height()));
    let in_photo = corners.map(|p| (p.x * w, p.y * h));
    let card_corners = [(0.0, 0.0), (cw, 0.0), (cw, ch), (0.0, ch)];
    let Some(to_card) = Homography::from_points(&in_photo, &card_corners) else {
        return RgbImage::new(width, height);
    };
    warp_onto(card, &to_card, width, height)
}

/// Like [`warp`], with a table colour wherever `homography` points
/// outside `source`.
#[cfg(test)]
fn warp_onto(source: &RgbImage, homography: &Homography, width: u32, height: u32) -> RgbImage {
    let (sw, sh) = (f64::from(source.width()), f64::from(source.height()));
    let inside = warp(source, homography, width, height);
    RgbImage::from_fn(width, height, |x, y| {
        let (u, v) = homography.map(f64::from(x) + 0.5, f64::from(y) + 0.5);
        if (0.0..sw).contains(&u) && (0.0..sh).contains(&v) {
            *inside.get_pixel(x, y)
        } else {
            Rgb([90, 60, 40])
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
            // The card's black frame and the downscaling move them a bit.
            assert!(
                (f.x - c.x).abs() < 0.03 && (f.y - c.y).abs() < 0.03,
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
    /// `INVUSO_PHOTOS`, separated by `;`), to check them by eye.
    #[test]
    #[ignore = "needs photos named in INVUSO_PHOTOS"]
    fn detects_corners_of_own_photos() {
        let paths = std::env::var("INVUSO_PHOTOS").unwrap();
        for path in paths.split(';') {
            let bytes = std::fs::read(path).unwrap();
            let photo = decode_upright(&bytes).unwrap().into_rgb8();
            println!("{path}: {:?}", detect_corners(&photo));
        }
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
