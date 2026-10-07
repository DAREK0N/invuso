use std::collections::BTreeMap;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdImageOff, LdX},
};

/// Largest zoom factor; enough to read small print on a receipt photo.
const MAX_SCALE: f64 = 6.0;
/// Zoom a double tap jumps to.
const DOUBLE_TAP_SCALE: f64 = 2.5;

/// Zoom and pan of the image; scale 1 shows it whole.
#[derive(Debug, Clone, Copy, PartialEq)]
struct View {
    scale: f64,
    x: f64,
    y: f64,
}

impl View {
    const FIT: Self = Self {
        scale: 1.0,
        x: 0.0,
        y: 0.0,
    };
}

/// Where a gesture started; moves are measured against it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    Pan {
        from: (f64, f64),
        view: View,
    },
    Pinch {
        distance: f64,
        middle: (f64, f64),
        view: View,
    },
}

/// An outlined part of the image that can be tapped, e.g. where a line of
/// a receipt was printed (OCR-37).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageMark {
    pub key: u64,
    /// SVG `points` of each outline, in pixels of the viewer's `mark_size`.
    pub outlines: Vec<String>,
    /// Upright rectangle around all outlines: left, top, right, bottom.
    pub bounds: (f64, f64, f64, f64),
    /// Drawn in the warning color (`pale-oak`).
    pub warning: bool,
    /// Read out instead of the outline.
    pub label: String,
}

/// Movement in pixels after which a touch is a pan, not a tap on a mark.
const TAP_SLOP: f64 = 10.0;

/// Full-screen image with pinch zoom, panning and double-tap zoom (UI-17,
/// RCP-04). With `marks` (in pixels of an image of `mark_size`) their
/// outlines lie on the image; tapping one calls `on_mark`, and the mark
/// `focus` is outlined strongly and zoomed to when the viewer opens.
///
/// The close button carries the id `invuso-viewer-close`: `MainActivity.kt`
/// clicks it on Android back, so back closes the viewer first.
#[component]
pub fn ImageViewer(
    src: String,
    alt: String,
    on_close: EventHandler<()>,
    #[props(default)] marks: Vec<ImageMark>,
    #[props(default)] mark_size: Option<(u32, u32)>,
    #[props(default)] focus: Option<u64>,
    #[props(default)] on_mark: Option<EventHandler<u64>>,
) -> Element {
    let mut view = use_signal(|| View::FIT);
    let mut pointers = use_signal(BTreeMap::<i32, (f64, f64)>::new);
    let mut gesture = use_signal(|| None::<Gesture>);
    let mut failed = use_signal(|| false);
    // Where the current touch started and whether it moved off since.
    let mut touch_start = use_signal(|| None::<(f64, f64)>);
    let mut moved = use_signal(|| false);
    let focus_bounds = focus
        .and_then(|key| marks.iter().find(|m| m.key == key))
        .map(|m| m.bounds);
    let marked = mark_size.filter(|_| !marks.is_empty());

    // A new finger, or one lifted, starts a new gesture from the current
    // view, so the image never jumps.
    let mut restart = move || {
        let current = view();
        let points: Vec<(f64, f64)> = pointers.read().values().copied().collect();
        gesture.set(match points.as_slice() {
            [one] => Some(Gesture::Pan {
                from: *one,
                view: current,
            }),
            [a, b, ..] => Some(Gesture::Pinch {
                distance: distance(*a, *b),
                middle: middle(*a, *b),
                view: current,
            }),
            [] => None,
        });
    };

    let current = view();
    let transform = format!(
        "transform: translate({}px, {}px) scale({});",
        current.x, current.y, current.scale
    );

    rsx! {
        div {
            class: "fixed inset-0 z-[1200] flex items-center justify-center overflow-hidden bg-black animate-fade-in touch-none select-none",
            role: "dialog",
            aria_modal: "true",
            aria_label: "{alt}",
            onmounted: move |event: MountedEvent| async move {
                // Zooms to the focused mark once the screen size is known.
                let (Some(size), Some(bounds)) = (mark_size, focus_bounds) else {
                    return;
                };
                if let Ok(rect) = event.data().get_client_rect().await {
                    view.set(focused(
                        (rect.size.width, rect.size.height),
                        (f64::from(size.0), f64::from(size.1)),
                        bounds,
                    ));
                }
            },
            onpointerdown: move |event| {
                let point = event.client_coordinates();
                if pointers.read().is_empty() {
                    touch_start.set(Some((point.x, point.y)));
                    moved.set(false);
                }
                pointers.write().insert(event.pointer_id(), (point.x, point.y));
                restart();
            },
            onpointermove: move |event| {
                let id = event.pointer_id();
                if !pointers.read().contains_key(&id) {
                    return;
                }
                let point = event.client_coordinates();
                if let Some(start) = touch_start()
                    && distance(start, (point.x, point.y)) > TAP_SLOP
                {
                    moved.set(true);
                }
                pointers.write().insert(id, (point.x, point.y));
                let points: Vec<(f64, f64)> = pointers.read().values().copied().collect();
                match (gesture(), points.as_slice()) {
                    (Some(Gesture::Pinch { distance: start, middle: from, view: base }), [a, b, ..]) => {
                        let scale = (base.scale * distance(*a, *b) / start.max(1.0)).clamp(1.0, MAX_SCALE);
                        let now = middle(*a, *b);
                        view.set(View { scale, x: base.x + now.0 - from.0, y: base.y + now.1 - from.1 });
                    }
                    (Some(Gesture::Pan { from, view: base }), [one]) if base.scale > 1.0 => {
                        view.set(View { scale: base.scale, x: base.x + one.0 - from.0, y: base.y + one.1 - from.1 });
                    }
                    _ => {}
                }
            },
            onpointerup: move |event| {
                pointers.write().remove(&event.pointer_id());
                if view().scale <= 1.0 {
                    view.set(View::FIT);
                }
                restart();
            },
            onpointercancel: move |event| {
                pointers.write().remove(&event.pointer_id());
                restart();
            },
            ondoubleclick: move |_| {
                view.set(if view().scale > 1.0 {
                    View::FIT
                } else {
                    View { scale: DOUBLE_TAP_SCALE, ..View::FIT }
                });
            },
            if failed() {
                div { class: "flex flex-col items-center gap-3 px-8 text-center text-floral-white-300",
                    Icon { icon: LdImageOff, class: "h-10 w-10" }
                    p { class: "text-sm", {t!("receipt.image_error").to_string()} }
                }
            } else {
                div { class: "relative will-change-transform", style: "{transform}",
                    img {
                        class: "block max-h-screen max-w-[100vw] object-contain",
                        src: "{src}",
                        alt: "{alt}",
                        draggable: "false",
                        onerror: move |_| failed.set(true),
                    }
                    if let Some((width, height)) = marked {
                        svg {
                            class: "absolute inset-0 h-full w-full",
                            view_box: "0 0 {width} {height}",
                            preserve_aspect_ratio: "none",
                            for mark in marks.iter().cloned() {
                                g {
                                    key: "{mark.key}",
                                    role: "button",
                                    "aria-label": "{mark.label}",
                                    onclick: move |_| {
                                        if let Some(on_mark) = on_mark
                                            && !moved()
                                        {
                                            on_mark.call(mark.key);
                                        }
                                    },
                                    for (index, points) in mark.outlines.iter().enumerate() {
                                        g { key: "{index}",
                                            // A slightly wider invisible edge makes thin rows easier to
                                            // hit, without covering the closely printed row above.
                                            polygon {
                                                class: "fill-transparent stroke-transparent",
                                                stroke_width: "8",
                                                vector_effect: "non-scaling-stroke",
                                                pointer_events: "all",
                                                points: "{points}",
                                            }
                                            polygon {
                                                class: mark_class(mark.warning, focus == Some(mark.key)),
                                                stroke_width: if focus == Some(mark.key) { "3" } else { "1.5" },
                                                vector_effect: "non-scaling-stroke",
                                                pointer_events: "none",
                                                points: "{points}",
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        button {
            id: "invuso-viewer-close",
            class: "fixed z-[1201] flex h-11 w-11 items-center justify-center rounded-full glass text-floral-white-50 active:bg-jet-black-800 transition-colors",
            style: "top: calc(0.75rem + var(--safe-area-top)); right: calc(0.75rem + var(--safe-area-right));",
            r#type: "button",
            aria_label: t!("common.close").to_string(),
            onclick: move |_| on_close.call(()),
            Icon { icon: LdX, class: "h-6 w-6" }
        }
    }
}

fn mark_class(warning: bool, focused: bool) -> &'static str {
    match (warning, focused) {
        (true, true) => "fill-pale-oak-400/45 stroke-pale-oak-600",
        (true, false) => "fill-pale-oak-300/10 stroke-pale-oak-500",
        (false, true) => "fill-cerulean-400/40 stroke-cerulean-600",
        (false, false) => "fill-cerulean-400/5 stroke-cerulean-400/70",
    }
}

/// Zoom and pan that show `bounds` (in pixels of an image of `size`)
/// centred on a screen of `screen` size: its width across most of the
/// screen, a row not taller than a quarter of it.
fn focused(screen: (f64, f64), size: (f64, f64), bounds: (f64, f64, f64, f64)) -> View {
    let (screen_w, screen_h) = screen;
    let (width, height) = size;
    if width <= 0.0 || height <= 0.0 || screen_w <= 0.0 || screen_h <= 0.0 {
        return View::FIT;
    }
    // The image fitted into the screen, as at scale 1.
    let shown_w = screen_w.min(screen_h * width / height);
    let shown_h = shown_w * height / width;
    let (left, top, right, bottom) = bounds;
    let box_w = ((right - left) / width * shown_w).max(1.0);
    let box_h = ((bottom - top) / height * shown_h).max(1.0);
    let scale = (0.9 * screen_w / box_w)
        .min(0.25 * screen_h / box_h)
        .clamp(1.0, MAX_SCALE);
    let (u, v) = ((left + right) / 2.0 / width, (top + bottom) / 2.0 / height);
    View {
        scale,
        x: -scale * (u - 0.5) * shown_w,
        y: -scale * (v - 0.5) * shown_h,
    }
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn middle(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focusing_centres_the_mark() {
        // A tall receipt (1000 × 4000) on a 400 × 800 screen is shown
        // 200 × 800; a row across most of its width, a quarter down.
        let view = focused(
            (400.0, 800.0),
            (1000.0, 4000.0),
            (100.0, 980.0, 900.0, 1020.0),
        );
        // The row is 160 px wide on screen: zoomed to 90 % of 400 px.
        assert!((view.scale - 2.25).abs() < 1e-9, "{}", view.scale);
        assert!(view.x.abs() < 1e-9);
        // Its centre (a quarter down, 200 px above the middle) moves to
        // the middle.
        assert!((view.y - 2.25 * 200.0).abs() < 1e-9, "{}", view.y);

        // Never smaller than the whole image, never beyond the limit.
        let whole = focused((400.0, 800.0), (1000.0, 4000.0), (0.0, 0.0, 1000.0, 4000.0));
        assert_eq!(whole, View::FIT);
        let tiny = focused(
            (400.0, 800.0),
            (1000.0, 4000.0),
            (500.0, 2000.0, 501.0, 2001.0),
        );
        assert_eq!(tiny.scale, MAX_SCALE);
        assert_eq!(
            focused((0.0, 0.0), (1000.0, 4000.0), (0.0, 0.0, 1.0, 1.0)),
            View::FIT
        );
    }
}
