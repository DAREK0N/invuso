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

/// Full-screen image with pinch zoom, panning and double-tap zoom (UI-17,
/// RCP-04).
///
/// The close button carries the id `invuso-viewer-close`: `MainActivity.kt`
/// clicks it on Android back, so back closes the viewer first.
#[component]
pub fn ImageViewer(src: String, alt: String, on_close: EventHandler<()>) -> Element {
    let mut view = use_signal(|| View::FIT);
    let mut pointers = use_signal(BTreeMap::<i32, (f64, f64)>::new);
    let mut gesture = use_signal(|| None::<Gesture>);
    let mut failed = use_signal(|| false);

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
            onpointerdown: move |event| {
                let point = event.client_coordinates();
                pointers.write().insert(event.pointer_id(), (point.x, point.y));
                restart();
            },
            onpointermove: move |event| {
                let id = event.pointer_id();
                if !pointers.read().contains_key(&id) {
                    return;
                }
                let point = event.client_coordinates();
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
                img {
                    class: "max-h-full max-w-full object-contain will-change-transform",
                    style: "{transform}",
                    src: "{src}",
                    alt: "{alt}",
                    draggable: "false",
                    onerror: move |_| failed.set(true),
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

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn middle(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}
