//! Simple charts drawn as SVG and plain elements from Rust (UI-19), no
//! chart library. Geometry uses whole thousandths of the total, so no
//! floating point is needed.

use dioxus::prelude::*;

/// Units a whole chart is divided into.
const SCALE: i64 = 1000;

/// One part of a [`DonutChart`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartSlice {
    pub key: String,
    /// Not negative; negative parts are drawn as empty.
    pub amount_minor: i64,
    /// Design-token color name, e.g. `"cerulean"`.
    pub color: String,
}

/// Ring of `slices` in their colors, starting at the top and going
/// clockwise; `children` sit in its middle (GRP-16).
#[component]
pub fn DonutChart(slices: Vec<ChartSlice>, label: String, children: Element) -> Element {
    let total: i64 = slices.iter().map(|s| s.amount_minor.max(0)).sum();
    let arcs = arcs(&slices, total);
    // A small gap tells neighbors of the same color apart.
    let gap = if arcs.len() > 1 { 6 } else { 0 };

    rsx! {
        div { class: "relative mx-auto aspect-square w-44",
            svg {
                class: "h-full w-full -rotate-90",
                view_box: "0 0 100 100",
                role: "img",
                "aria-label": "{label}",
                circle {
                    class: "fill-none stroke-jet-black-800",
                    cx: "50",
                    cy: "50",
                    r: "40",
                    stroke_width: "14",
                }
                for (key, color, start, length) in arcs {
                    circle {
                        key: "{key}",
                        class: "fill-none {stroke_class(&color)}",
                        cx: "50",
                        cy: "50",
                        r: "40",
                        stroke_width: "14",
                        path_length: "{SCALE}",
                        stroke_dasharray: "{(length - gap).max(1)} {SCALE}",
                        stroke_dashoffset: "{-start}",
                    }
                }
            }
            div { class: "absolute inset-0 flex flex-col items-center justify-center px-8 text-center",
                {children}
            }
        }
    }
}

/// Horizontal bar showing `amount_minor` as a share of `total_minor`.
#[component]
pub fn ShareBar(amount_minor: i64, total_minor: i64, color: String) -> Element {
    let permille = share(amount_minor, total_minor);
    rsx! {
        span { class: "block h-1.5 w-full overflow-hidden rounded-full bg-jet-black-800",
            span {
                class: "block h-full rounded-full {fill_class(&color)}",
                style: "width: {permille / 10}.{permille % 10}%",
            }
        }
    }
}

/// "12 %": the share in whole percent, rounded half up; "< 1 %" for a
/// share that rounds to nothing.
pub fn percent_text(amount_minor: i64, total_minor: i64) -> String {
    let permille = share(amount_minor, total_minor);
    if permille > 0 && permille < 5 {
        return t!("chart.below_one_percent").to_string();
    }
    t!("chart.percent", value = (permille + 5) / 10).to_string()
}

/// Key, color, start and length of each visible arc, in thousandths.
/// Starts and ends come from the running sum, so the arcs close the ring
/// exactly.
fn arcs(slices: &[ChartSlice], total: i64) -> Vec<(String, String, i64, i64)> {
    let mut arcs = Vec::new();
    let mut before = 0_i64;
    for slice in slices {
        let amount = slice.amount_minor.max(0);
        let start = share(before, total);
        before = before.saturating_add(amount);
        let length = share(before, total) - start;
        if length > 0 {
            arcs.push((slice.key.clone(), slice.color.clone(), start, length));
        }
    }
    arcs
}

/// `amount / total` in thousandths, rounded down, 0 for an empty total.
fn share(amount: i64, total: i64) -> i64 {
    if total <= 0 || amount <= 0 {
        return 0;
    }
    let permille = i128::from(amount) * i128::from(SCALE) / i128::from(total);
    i64::try_from(permille.min(i128::from(SCALE))).unwrap_or(SCALE)
}

/// Stroke class per color. Written out in full so Tailwind finds them;
/// the 500 tones read on dark and light surfaces alike.
fn stroke_class(color: &str) -> &'static str {
    match color {
        "muted-teal" => "stroke-muted-teal-500",
        "pale-oak" => "stroke-pale-oak-500",
        "thistle" => "stroke-thistle-500",
        "dusty-grape" => "stroke-dusty-grape-500",
        "ash-grey" => "stroke-ash-grey-500",
        "slate-grey" => "stroke-slate-grey-500",
        "floral-white" => "stroke-floral-white-500",
        // "cerulean" and colors this version does not know.
        _ => "stroke-cerulean-500",
    }
}

/// Fill class per color, like [`stroke_class`].
fn fill_class(color: &str) -> &'static str {
    match color {
        "muted-teal" => "bg-muted-teal-500",
        "pale-oak" => "bg-pale-oak-500",
        "thistle" => "bg-thistle-500",
        "dusty-grape" => "bg-dusty-grape-500",
        "ash-grey" => "bg-ash-grey-500",
        "slate-grey" => "bg-slate-grey-500",
        "floral-white" => "bg-floral-white-500",
        _ => "bg-cerulean-500",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(key: &str, amount_minor: i64) -> ChartSlice {
        ChartSlice {
            key: key.into(),
            amount_minor,
            color: "cerulean".into(),
        }
    }

    #[test]
    fn arcs_close_the_ring_exactly() {
        // Thirds: 333 + 333 + 334, not 333 × 3 = 999.
        let slices = [slice("a", 100), slice("b", 100), slice("c", 100)];
        let arcs = arcs(&slices, 300);
        let parts: Vec<(i64, i64)> = arcs.iter().map(|a| (a.2, a.3)).collect();
        assert_eq!(parts, [(0, 333), (333, 333), (666, 334)]);
    }

    #[test]
    fn tiny_parts_still_close_the_ring_and_negative_ones_are_left_out() {
        let slices = [
            slice("big", 1_000_000),
            slice("tiny", 1),
            slice("minus", -5),
        ];
        let arcs = arcs(&slices, 1_000_001);
        let parts: Vec<(&str, i64, i64)> = arcs.iter().map(|a| (a.0.as_str(), a.2, a.3)).collect();
        assert_eq!(parts, [("big", 0, 999), ("tiny", 999, 1)]);
    }

    #[test]
    fn share_handles_empty_and_huge_totals() {
        assert_eq!(share(5, 0), 0);
        assert_eq!(share(i64::MAX, i64::MAX), 1000);
        assert_eq!(share(1, 3), 333);
    }

    #[test]
    fn percent_rounds_half_up_and_marks_tiny_shares() {
        rust_i18n::set_locale("de");
        assert_eq!(percent_text(125, 1_000), "13 %");
        assert_eq!(percent_text(124, 1_000), "12 %");
        assert_eq!(percent_text(1, 1_000), "< 1 %");
        assert_eq!(percent_text(0, 1_000), "0 %");
        assert_eq!(percent_text(1_000, 1_000), "100 %");
    }
}
