//! Image preparation before recognition (OCR-04): upright by EXIF, bounded
//! in size, contrast stretched.
//!
//! No binarization or grayscale conversion: the PP-OCRv6 models were
//! trained on colour photos, and thresholding loses the faint strokes of
//! thermal paper. Straightening a skewed photo is left to the detector's
//! rotated boxes; perspective correction comes with RCP-05.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage};

use super::OcrError;

/// Longest side kept for recognition. Phone photos are 4000–8000 px; more
/// than this adds no detail a receipt line needs, only memory and time.
const MAX_SIDE: u32 = 4000;

/// Share of darkest and brightest pixels ignored when stretching, so a
/// shadow or a glare spot does not decide the range.
const CLIP_PERCENT: u64 = 1;

/// Decodes an archived receipt image and prepares it for the engine.
pub fn prepare(bytes: &[u8]) -> Result<RgbImage, OcrError> {
    let mut rgb = decode(bytes)?;
    stretch_contrast(&mut rgb);
    Ok(rgb)
}

/// Decodes an image upright and bounded in size.
pub(super) fn decode(bytes: &[u8]) -> Result<RgbImage, OcrError> {
    let image_error = |e: image::ImageError| OcrError::Image(e.to_string());
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| OcrError::Image(e.to_string()))?
        .into_decoder()
        .map_err(image_error)?;
    let orientation = decoder.orientation().map_err(image_error)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(image_error)?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > MAX_SIDE {
        image = image.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    Ok(image.into_rgb8())
}

/// Spreads the brightness range of a dull photo (grey paper, dim light)
/// over the full scale; every channel gets the same mapping, so colours
/// keep their hue.
pub(super) fn stretch_contrast(image: &mut RgbImage) {
    let mut histogram = [0u64; 256];
    for pixel in image.pixels() {
        histogram[luma(pixel.0) as usize] += 1;
    }
    let total: u64 = histogram.iter().sum();
    let clip = total * CLIP_PERCENT / 100;
    let low = percentile(&histogram, clip);
    let high = 255 - percentile_from_top(&histogram, clip);
    if high <= low {
        return;
    }
    let (low, high) = (i32::from(low), i32::from(high));
    let range = high - low;
    if range >= 250 {
        return;
    }
    let map: Vec<u8> = (0..=255i32)
        .map(|v| ((v - low) * 255 / range).clamp(0, 255) as u8)
        .collect();
    for pixel in image.pixels_mut() {
        for channel in pixel.0.iter_mut() {
            *channel = map[*channel as usize];
        }
    }
}

/// Integer luma (ITU-R BT.601).
fn luma([r, g, b]: [u8; 3]) -> u8 {
    ((299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000) as u8
}

/// First brightness value with more than `skip` pixels at or below it.
fn percentile(histogram: &[u64; 256], skip: u64) -> u8 {
    let mut seen = 0;
    for (value, count) in histogram.iter().enumerate() {
        seen += count;
        if seen > skip {
            return value as u8;
        }
    }
    255
}

/// Like [`percentile`], counted from the bright end; 0 means 255.
fn percentile_from_top(histogram: &[u64; 256], skip: u64) -> u8 {
    let mut seen = 0;
    for (offset, count) in histogram.iter().rev().enumerate() {
        seen += count;
        if seen > skip {
            return offset as u8;
        }
    }
    255
}

#[cfg(test)]
mod tests {
    use image::{ImageFormat, Rgb};

    use super::*;

    #[test]
    fn dull_image_is_spread_to_the_full_range() {
        // Grey "ink" (90) on grey "paper" (170).
        let mut image = RgbImage::from_fn(20, 20, |x, _| {
            if x < 10 {
                Rgb([90, 90, 90])
            } else {
                Rgb([170, 170, 170])
            }
        });
        stretch_contrast(&mut image);
        assert_eq!(image.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(image.get_pixel(19, 0).0, [255, 255, 255]);
    }

    #[test]
    fn full_range_and_flat_images_stay_unchanged() {
        let original = RgbImage::from_fn(16, 16, |x, y| {
            let v = (x * 16 + y) as u8;
            Rgb([v, v, v])
        });
        let mut image = original.clone();
        stretch_contrast(&mut image);
        assert_eq!(image, original);

        let flat = RgbImage::from_pixel(8, 8, Rgb([200, 200, 200]));
        let mut image = flat.clone();
        stretch_contrast(&mut image);
        assert_eq!(image, flat);
    }

    #[test]
    fn large_photos_are_scaled_down_keeping_the_aspect_ratio() {
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::new(4400, 2200))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        assert_eq!(prepare(&png).unwrap().dimensions(), (4000, 2000));
        assert!(matches!(prepare(b"no image"), Err(OcrError::Image(_))));
    }
}
