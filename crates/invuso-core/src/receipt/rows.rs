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
    let fragments: Vec<&RecognizedText> = fragments
        .iter()
        .filter(|f| !f.text.trim().is_empty())
        .collect();
    let in_price_column = price_column(&fragments);
    // Curved or tilted paper lifts or lowers the right-aligned price column
    // against the names: names and prices then pair up off-centre, or with
    // the neighbouring row. Only then are other offsets tried; the one that
    // pairs the most names with prices, most closely, wins.
    let height = typical_height(&fragments);
    let unit = height / OFFSET_STEPS;
    let mut rows = group_shifted(&fragments, &in_price_column, 0);
    let (_, misfit) = pairing(&rows, &fragments, &in_price_column, 0);
    if unit > 0 && misfit.is_some_and(|misfit| misfit * 4 > height) {
        // (rank, rows) of the best offset so far.
        let mut best: Option<(Rank, Vec<Vec<&RecognizedText>>)> = None;
        for step in -OFFSET_STEPS..=OFFSET_STEPS {
            let offset = step * unit;
            let shifted = group_shifted(&fragments, &in_price_column, offset);
            let (score, misfit) = pairing(&shifted, &fragments, &in_price_column, offset);
            let rank = (score, -misfit.unwrap_or(height), -step.abs());
            if best.as_ref().is_none_or(|(top, _)| rank > *top) {
                best = Some((rank, shifted));
            }
        }
        if let Some((_, shifted)) = best {
            rows = shifted;
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

/// Pairs, closeness of the pairs and nearness to no offset, compared in
/// this order.
type Rank = (i64, i64, i64);

/// How many of the newest rows a fragment may still join.
const OPEN_ROWS: usize = 3;

/// Offsets of the price column tried, in eighths of a typical box height,
/// up and down.
const OFFSET_STEPS: i64 = 8;

/// Groups the fragments with the price column moved down by `offset`
/// pixels (up if negative); see [`group_rows`].
fn group_shifted<'a>(
    fragments: &[&'a RecognizedText],
    in_price_column: &[bool],
    offset: i64,
) -> Vec<Vec<&'a RecognizedText>> {
    let shift = |index: usize| if in_price_column[index] { offset } else { 0 };
    let mut order: Vec<usize> = (0..fragments.len()).collect();
    order.sort_by_key(|&i| {
        (
            fragments[i].bbox.center_y2() + 2 * shift(i),
            fragments[i].bbox.left,
        )
    });

    // Row members as indices, so the shifted position stays known.
    let mut rows: Vec<Vec<usize>> = Vec::new();
    for index in order {
        let open = rows.len().saturating_sub(OPEN_ROWS);
        let best = (open..rows.len())
            .filter(|&r| !shares_column(rows[r].iter().map(|&i| fragments[i]), fragments[index]))
            .filter_map(|r| {
                let members = rows[r].iter().map(|&i| (fragments[i], shift(i)));
                distance(members, fragments[index], shift(index)).map(|d| (d, r))
            })
            .min();
        match best {
            Some((_, r)) => rows[r].push(index),
            None => rows.push(vec![index]),
        }
    }
    rows.into_iter()
        .map(|row| row.into_iter().map(|i| fragments[i]).collect())
        .collect()
}

/// How well names and prices pair up when the price column is moved by
/// `offset`: rows with a name and a price count, rows with a price alone
/// count against; and the median vertical distance between a row's name
/// and its price (`None` without such rows).
fn pairing(
    rows: &[Vec<&RecognizedText>],
    fragments: &[&RecognizedText],
    in_price_column: &[bool],
    offset: i64,
) -> (i64, Option<i64>) {
    let is_price = |fragment: &RecognizedText| {
        fragments
            .iter()
            .position(|f| std::ptr::eq(*f, fragment))
            .is_some_and(|i| in_price_column[i])
    };
    let mut score = 0;
    let mut misfits: Vec<i64> = Vec::new();
    for row in rows {
        let prices: Vec<&&RecognizedText> = row.iter().filter(|f| is_price(f)).collect();
        let names: Vec<&&RecognizedText> = row
            .iter()
            .filter(|f| !is_price(f) && f.text.chars().any(char::is_alphabetic))
            .collect();
        match (names.first(), prices.first()) {
            (Some(name), Some(price)) => {
                score += 2;
                // Centres are doubled; halve the difference.
                misfits
                    .push((name.bbox.center_y2() - price.bbox.center_y2() - 2 * offset).abs() / 2);
            }
            (None, Some(_)) => score -= 1,
            _ => {}
        }
    }
    misfits.sort_unstable();
    (score, misfits.get(misfits.len() / 2).copied())
}

/// Fragments ending at the right edge of the text with a digit in them:
/// the price column, which tills align right.
fn price_column(fragments: &[&RecognizedText]) -> Vec<bool> {
    let left = fragments.iter().map(|f| f.bbox.left).min().unwrap_or(0);
    let right = fragments.iter().map(|f| f.bbox.right).max().unwrap_or(0);
    let width = i64::from(right) - i64::from(left);
    fragments
        .iter()
        .map(|f| {
            let flush_right =
                (i64::from(right) - i64::from(f.bbox.right)) * PRICE_COLUMN_SHARE < width;
            let right_half = (i64::from(f.bbox.left) - i64::from(left)) * 2 > width;
            flush_right && right_half && f.text.chars().any(|c| c.is_ascii_digit())
        })
        .collect()
}

/// A fragment ending within 1/8 of the text width from its right edge is
/// in the price column.
const PRICE_COLUMN_SHARE: i64 = 8;

/// Median height of the fragments.
fn typical_height(fragments: &[&RecognizedText]) -> i64 {
    let mut heights: Vec<i64> = fragments.iter().map(|f| f.bbox.height()).collect();
    heights.sort_unstable();
    heights.get(heights.len() / 2).copied().unwrap_or(0)
}

/// How far the fragment's centre is from the row's mean centre, relative
/// to the row (scaled by 2n), if it is close enough to join. Each box is
/// taken as moved down by its shift.
fn distance<'a>(
    row: impl Iterator<Item = (&'a RecognizedText, i64)>,
    fragment: &RecognizedText,
    shift: i64,
) -> Option<i64> {
    let (mut n, mut top_sum, mut bottom_sum) = (0_i64, 0_i64, 0_i64);
    for (member, member_shift) in row {
        n += 1;
        top_sum += i64::from(member.bbox.top) + member_shift;
        bottom_sum += i64::from(member.bbox.bottom) + member_shift;
    }
    // Everything scaled by 2n to stay in integers:
    // |centre − mean centre| < min(height, mean height) / 2
    let distance = (n * (fragment.bbox.center_y2() + 2 * shift) - (top_sum + bottom_sum)).abs();
    let limit = (n * fragment.bbox.height()).min(bottom_sum - top_sum);
    // Compared across rows of different sizes, so per fragment.
    (distance < limit).then(|| distance / n)
}

/// Whether the fragment overlaps a fragment of the row horizontally by
/// more than a third of the narrower one.
fn shares_column<'a>(
    mut row: impl Iterator<Item = &'a RecognizedText>,
    fragment: &RecognizedText,
) -> bool {
    row.any(|other| {
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
