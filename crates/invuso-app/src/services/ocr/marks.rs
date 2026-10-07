//! Where each line read from a receipt sits in the photo and how sure the
//! engine was of it (OCR-18, OCR-37).
//!
//! The engine stores its boxes in the levelled image (rows horizontal);
//! [`PhotoQuad::of`] turns them back by `skew_degrees`, so a mark lies on
//! the print even when the receipt was photographed askew.

use std::collections::BTreeSet;

use invuso_core::receipt::{BoundingBox, ParsedReceipt};

use crate::storage::ReceiptText;

/// Below this confidence a line counts as unsure and is marked (user
/// decision in AP-36: about one box in ten of the sample receipts).
pub const UNSURE_BELOW: f32 = 0.9;

/// Whether a line read with this confidence is marked as unsure; lines
/// typed by hand have none.
pub fn is_unsure(confidence: Option<f32>) -> bool {
    confidence.is_some_and(|c| c < UNSURE_BELOW)
}

/// Four corners of a text box in pixels of the image the engine read,
/// clockwise from the top left as printed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhotoQuad(pub [(f32, f32); 4]);

impl PhotoQuad {
    /// The box of a fragment in the photo: its levelled box turned back by
    /// `skew_degrees` around the centre of an image of `size`, the inverse
    /// of the levelling in `paddle::level_box`.
    pub fn of(bbox: BoundingBox, skew_degrees: f32, size: (u32, u32)) -> Self {
        let (sin, cos) = skew_degrees.to_radians().sin_cos();
        let (cx, cy) = (size.0 as f32 / 2.0, size.1 as f32 / 2.0);
        let turn = |x: i32, y: i32| {
            let (ex, ey) = (x as f32 - cx, y as f32 - cy);
            (cx + ex * cos - ey * sin, cy + ex * sin + ey * cos)
        };
        Self([
            turn(bbox.left, bbox.top),
            turn(bbox.right, bbox.top),
            turn(bbox.right, bbox.bottom),
            turn(bbox.left, bbox.bottom),
        ])
    }

    /// Smallest upright rectangle around it: left, top, right, bottom.
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        self.0.iter().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(l, t, r, b), &(x, y)| (l.min(x), t.min(y), r.max(x), b.max(y)),
        )
    }

    /// The corners as an SVG `points` attribute.
    pub fn svg_points(&self) -> String {
        self.0
            .iter()
            .map(|(x, y)| format!("{x:.1},{y:.1}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Upright rectangle around all quads: left, top, right, bottom; `None`
/// without quads.
pub fn bounds_of(quads: &[PhotoQuad]) -> Option<(f32, f32, f32, f32)> {
    quads
        .iter()
        .map(PhotoQuad::bounds)
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
}

/// What the photo says about one parsed item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemMark {
    /// The lowest confidence of the text it was read from: one misread
    /// price makes the whole line doubtful. `None` if no text is known.
    pub confidence: Option<f32>,
    /// Where it was printed; empty if the image size is unknown.
    pub quads: Vec<PhotoQuad>,
}

/// Confidence and place in the photo of each item of `parsed`, which must
/// have been read from `text` (`parse_receipt(&text.recognized(), …)`).
pub fn item_marks(text: &ReceiptText, parsed: &ParsedReceipt) -> Vec<ItemMark> {
    parsed
        .items
        .iter()
        .map(|item| {
            let fragments: BTreeSet<usize> = item
                .rows
                .iter()
                .filter_map(|&row| parsed.rows.get(row))
                .flat_map(|row| row.fragments.iter().copied())
                .collect();
            let fragments: Vec<_> = fragments
                .into_iter()
                .filter_map(|i| text.fragments.get(i))
                .collect();
            let confidence = fragments.iter().map(|f| f.confidence).reduce(f32::min);
            let quads = match text.image_size {
                Some(size) => fragments
                    .iter()
                    .map(|f| PhotoQuad::of(f.bbox, text.skew_degrees, size))
                    .collect(),
                None => Vec::new(),
            };
            ItemMark { confidence, quads }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::Currency;
    use invuso_core::receipt::parse_receipt;

    use super::*;
    use crate::storage::OcrFragment;

    fn bbox(left: i32, top: i32, right: i32, bottom: i32) -> BoundingBox {
        BoundingBox {
            left,
            top,
            right,
            bottom,
        }
    }

    fn fragment(text: &str, bbox: BoundingBox, confidence: f32) -> OcrFragment {
        OcrFragment {
            text: text.to_string(),
            bbox,
            confidence,
        }
    }

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        // Boxes are whole pixels.
        (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0
    }

    #[test]
    fn a_level_box_stays_where_it_is() {
        let quad = PhotoQuad::of(bbox(10, 20, 110, 40), 0.0, (400, 300));
        assert_eq!(
            quad.0,
            [(10.0, 20.0), (110.0, 20.0), (110.0, 40.0), (10.0, 40.0)]
        );
        assert_eq!(quad.bounds(), (10.0, 20.0, 110.0, 40.0));
        assert_eq!(
            quad.svg_points(),
            "10.0,20.0 110.0,20.0 110.0,40.0 10.0,40.0"
        );
    }

    #[test]
    fn a_box_of_a_skewed_photo_lands_on_the_print() {
        // Levelling as the engine does it (`paddle::level_box`): a point of
        // the photo turned by the skew around the centre.
        let (size, skew) = ((1000, 2000), 6.0_f32);
        let (sin, cos) = skew.to_radians().sin_cos();
        let level = |(x, y): (f32, f32)| {
            let (dx, dy) = (x - 500.0, y - 1000.0);
            (500.0 + dx * cos + dy * sin, 1000.0 - dx * sin + dy * cos)
        };
        // A printed word right of the centre slopes down to the right.
        let printed = (800.0, 1400.0);
        let (lx, ly) = level(printed);
        let (lx, ly) = (lx.round() as i32, ly.round() as i32);
        let quad = PhotoQuad::of(bbox(lx, ly, lx, ly), skew, size);
        assert!(close(quad.0[0], printed), "{:?}", quad.0[0]);

        // The right end of a level box lies lower in the photo.
        let quad = PhotoQuad::of(bbox(200, 990, 800, 1010), skew, size);
        let (left_top, right_top) = (quad.0[0], quad.0[1]);
        let drop = right_top.1 - left_top.1;
        assert!((drop - 600.0 * sin).abs() < 0.5, "{drop}");
        assert_eq!(bounds_of(&[]), None);
        let (l, t, r, b) = bounds_of(&[quad]).unwrap();
        assert_eq!((l, t, r, b), quad.bounds());
        assert!(
            quad.0
                .iter()
                .all(|&(x, y)| (l..=r).contains(&x) && (t..=b).contains(&y))
        );
        assert!(b - t > 20.0, "a tilted box is taller than the level one");
    }

    #[test]
    fn an_item_is_as_sure_as_its_weakest_text() {
        let text = ReceiptText::new(
            "test",
            vec![
                fragment("1,99", bbox(300, 102, 380, 122), 0.8),
                fragment("Milch", bbox(10, 100, 110, 120), 0.99),
                fragment("Brot", bbox(10, 140, 110, 160), 0.97),
                fragment("2,49", bbox(300, 142, 380, 162), 0.95),
                fragment("SUMME 4,48", bbox(10, 190, 380, 210), 0.5),
            ],
            0.0,
        )
        .with_image_size(400, 300);
        let parsed =
            parse_receipt(&text.recognized(), Currency::from_code("EUR").unwrap()).unwrap();
        assert_eq!(parsed.items.len(), 2);
        let marks = item_marks(&text, &parsed);
        assert_eq!(marks[0].confidence, Some(0.8));
        assert_eq!(marks[1].confidence, Some(0.95));
        assert!(is_unsure(marks[0].confidence));
        assert!(!is_unsure(marks[1].confidence));
        assert!(!is_unsure(None));
        assert_eq!(
            bounds_of(&marks[0].quads),
            Some((10.0, 100.0, 380.0, 122.0))
        );

        // Without the image size the confidence is still known.
        let unsized_text = ReceiptText {
            image_size: None,
            ..text
        };
        let marks = item_marks(&unsized_text, &parsed);
        assert_eq!(marks[0].confidence, Some(0.8));
        assert!(marks[0].quads.is_empty());
    }
}
