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
/// A fragment joins a row when its centre is less than half a fragment
/// height away from the row's mean centre; the smaller of the two heights
/// counts, so one tall box cannot swallow its neighbours. A row never takes
/// a fragment that lies in the same column as one it already has: on
/// curved paper the price of the next row may sort in before, and two
/// prices side by side would ruin both rows. Of the last [`OPEN_ROWS`]
/// rows the closest fitting one wins. Integer arithmetic only, so every
/// platform groups identically.
pub(super) fn group_rows(fragments: &[RecognizedText]) -> Vec<Row> {
    let mut sorted: Vec<&RecognizedText> = fragments
        .iter()
        .filter(|f| !f.text.trim().is_empty())
        .collect();
    sorted.sort_by_key(|f| (f.bbox.center_y2(), f.bbox.left));

    let mut rows: Vec<Vec<&RecognizedText>> = Vec::new();
    for fragment in sorted {
        let open = rows.len().saturating_sub(OPEN_ROWS);
        let best = (open..rows.len())
            .filter(|&i| !shares_column(&rows[i], fragment))
            .filter_map(|i| distance(&rows[i], fragment).map(|d| (d, i)))
            .min();
        match best {
            Some((_, i)) => rows[i].push(fragment),
            None => rows.push(vec![fragment]),
        }
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

/// How many of the newest rows a fragment may still join.
const OPEN_ROWS: usize = 3;

/// How far the fragment's centre is from the row's mean centre, relative
/// to the row (scaled by 2n), if it is close enough to join.
fn distance(row: &[&RecognizedText], fragment: &RecognizedText) -> Option<i64> {
    let n = row.len() as i64;
    let top_sum: i64 = row.iter().map(|f| i64::from(f.bbox.top)).sum();
    let bottom_sum: i64 = row.iter().map(|f| i64::from(f.bbox.bottom)).sum();
    // Everything scaled by 2n to stay in integers:
    // |centre − mean centre| < min(height, mean height) / 2
    let distance = (n * fragment.bbox.center_y2() - (top_sum + bottom_sum)).abs();
    let limit = (n * fragment.bbox.height()).min(bottom_sum - top_sum);
    // Compared across rows of different sizes, so per fragment.
    (distance < limit).then(|| distance / n)
}

/// Whether the fragment overlaps a fragment of the row horizontally by
/// more than a third of the narrower one.
fn shares_column(row: &[&RecognizedText], fragment: &RecognizedText) -> bool {
    row.iter().any(|other| {
        let overlap =
            fragment.bbox.right.min(other.bbox.right) - fragment.bbox.left.max(other.bbox.left);
        let narrower = (fragment.bbox.right - fragment.bbox.left)
            .min(other.bbox.right - other.bbox.left)
            .max(1);
        overlap * 3 > narrower
    })
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
    fn two_prices_of_one_column_never_share_a_row() {
        // Crumpled paper: the second price sits higher than its name and
        // sorts in before it.
        let rows = group_rows(&[
            frag(0, 100, 300, 150, "Leergut A"),
            frag(600, 108, 700, 158, "-1,25"),
            frag(600, 140, 700, 190, "-9,83"),
            frag(0, 152, 300, 202, "Leergut B"),
        ]);
        assert_eq!(texts(&rows), ["Leergut A -1,25", "Leergut B -9,83"]);
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
