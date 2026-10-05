//! Row reconstruction (OCR-10): OCR engines report text in separate boxes,
//! e.g. the article name on the left and its price on the right. Boxes
//! whose vertical centres lie close together form one printed row.

use super::{BoundingBox, RecognizedText};

/// One printed row: its fragments left to right.
pub(super) struct Row {
    pub bbox: BoundingBox,
    pub text: String,
}

/// Groups fragments into rows, top to bottom.
///
/// A fragment joins the row above when its centre is less than half a
/// fragment height away from the row's mean centre; the smaller of the two
/// heights counts, so one tall box cannot swallow its neighbours. Integer
/// arithmetic only, so every platform groups identically.
pub(super) fn group_rows(fragments: &[RecognizedText]) -> Vec<Row> {
    let mut sorted: Vec<&RecognizedText> = fragments
        .iter()
        .filter(|f| !f.text.trim().is_empty())
        .collect();
    sorted.sort_by_key(|f| (f.bbox.center_y2(), f.bbox.left));

    let mut rows: Vec<Vec<&RecognizedText>> = Vec::new();
    for fragment in sorted {
        if let Some(row) = rows.last_mut()
            && belongs_to(row, fragment)
        {
            row.push(fragment);
            continue;
        }
        rows.push(vec![fragment]);
    }

    rows.into_iter()
        .map(|mut row| {
            row.sort_by_key(|f| f.bbox.left);
            let bbox = row
                .iter()
                .map(|f| f.bbox)
                .reduce(BoundingBox::union)
                .unwrap_or_default();
            let text = row
                .iter()
                .map(|f| f.text.trim())
                .collect::<Vec<_>>()
                .join(" ");
            Row { bbox, text }
        })
        .collect()
}

fn belongs_to(row: &[&RecognizedText], fragment: &RecognizedText) -> bool {
    let n = row.len() as i64;
    let top_sum: i64 = row.iter().map(|f| i64::from(f.bbox.top)).sum();
    let bottom_sum: i64 = row.iter().map(|f| i64::from(f.bbox.bottom)).sum();
    // Everything scaled by 2n to stay in integers:
    // |centre − mean centre| < min(height, mean height) / 2
    let distance = (n * fragment.bbox.center_y2() - (top_sum + bottom_sum)).abs();
    let limit = (n * fragment.bbox.height()).min(bottom_sum - top_sum);
    distance < limit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(left: i32, top: i32, right: i32, bottom: i32, text: &str) -> RecognizedText {
        RecognizedText {
            text: text.into(),
            bbox: BoundingBox {
                left,
                top,
                right,
                bottom,
            },
        }
    }

    fn texts(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn name_and_price_boxes_form_one_row() {
        // Taken from the Lidl fixture: padded boxes overlap the next row.
        let rows = group_rows(&[
            frag(1837, 1195, 2085, 1292, "3,99 A"),
            frag(511, 1110, 1236, 1204, "Dattelcherrytomaten"),
            frag(1841, 1112, 2081, 1205, "1,49 A"),
            frag(511, 1196, 1271, 1290, "Grüne Oliven o. Kern"),
        ]);
        assert_eq!(
            texts(&rows),
            ["Dattelcherrytomaten 1,49 A", "Grüne Oliven o. Kern 3,99 A"]
        );
        assert_eq!(
            rows[0].bbox,
            BoundingBox {
                left: 511,
                top: 1110,
                right: 2081,
                bottom: 1205
            }
        );
    }

    #[test]
    fn slightly_tilted_row_stays_together() {
        let rows = group_rows(&[
            frag(0, 100, 300, 140, "Brot"),
            frag(600, 112, 700, 152, "2,49"),
            frag(0, 150, 300, 190, "Milch"),
        ]);
        assert_eq!(texts(&rows), ["Brot 2,49", "Milch"]);
    }

    #[test]
    fn empty_fragments_are_ignored() {
        let rows = group_rows(&[frag(0, 0, 10, 10, "  "), frag(0, 20, 10, 30, "A")]);
        assert_eq!(texts(&rows), ["A"]);
        assert!(group_rows(&[]).is_empty());
    }

    #[test]
    fn flat_boxes_never_merge() {
        let rows = group_rows(&[frag(0, 10, 5, 10, "a"), frag(6, 10, 9, 10, "b")]);
        assert_eq!(rows.len(), 2);
    }
}
