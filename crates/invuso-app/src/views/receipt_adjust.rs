use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdCheck, LdCircleAlert, LdContrast, LdFocus, LdMaximize, LdRotateCcw, LdRotateCw,
        LdWandSparkles, LdX,
    },
};

use crate::components::Button;
use crate::platform;
use crate::services::receipt_edit::{self, ImageEdit, Point};
use crate::services::receipts;
use crate::storage::{Db, ReceiptFiles};

/// Diameter of the magnifier in pixels and how much it enlarges.
const LOUPE_SIZE: f64 = 120.0;
const LOUPE_ZOOM: f64 = 2.5;
/// How far above the finger the magnifier sits, so the finger does not
/// cover it.
const LOUPE_OFFSET: f64 = 110.0;
/// Padding of the stage (`p-6`): handles and magnifier may reach into it.
const STAGE_PADDING: f64 = 24.0;

/// Preview of the contrast filter in the WebView; the stored image gets
/// the real one (`receipt_edit::flatten_contrast`).
const CONTRAST_PREVIEW: &str = "filter: contrast(1.45) brightness(1.08);";
/// Written out when the filter is off: Dioxus updates the `style`
/// attribute property by property, so a property that is merely left out
/// would stay set (seen on the phone in AP-34).
const NO_PREVIEW: &str = "filter: none;";

/// Label of a corner handle, in the order of [`ImageEdit::corners`].
fn corner_label(index: usize) -> String {
    match index {
        0 => t!("adjust.corner_top_left"),
        1 => t!("adjust.corner_top_right"),
        2 => t!("adjust.corner_bottom_right"),
        _ => t!("adjust.corner_bottom_left"),
    }
    .to_string()
}

/// A corner being dragged: where the finger went down and where the
/// corner was then. Moves are applied as a distance from there, so the
/// corner does not jump under the finger.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    corner: usize,
    pointer: i32,
    from: (f64, f64),
    start: Point,
}

/// What the automatic corner search is doing.
#[derive(Debug, Clone, PartialEq)]
enum Detection {
    Idle,
    Running,
    /// No paper edge stood out; the corners stay where they were.
    NotFound,
    Failed(String),
}

/// Full-screen step right after a photo is taken or chosen (RCP-05,
/// idee.md 7.2 step 1): turn it in quarter turns, drag four corners onto
/// the receipt's corners (with a magnifier under the finger) or let them
/// be found, and optionally strengthen contrast or sharpen. "Apply"
/// stores the corrected copy next to the untouched original and hands
/// back the receipt; unchanged, it is handed back as it is. Recognition
/// runs afterwards on what was applied.
///
/// The close button carries the id `invuso-viewer-close`: `MainActivity.kt`
/// clicks it on Android back.
#[component]
pub fn ReceiptAdjuster(
    receipt: ReceiptFiles,
    on_done: EventHandler<ReceiptFiles>,
    on_cancel: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut edit = use_signal(ImageEdit::default);
    // Content size of the stage the photo is fitted into.
    let mut stage = use_signal(|| None::<(f64, f64)>);
    let mut drag = use_signal(|| None::<Drag>);
    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| None::<String>);
    let mut detection = use_signal(|| Detection::Idle);

    // The photo's upright size, to fit it and to turn the corner positions
    // into pixels; reading the header is quick but still file I/O.
    let size = use_resource({
        let receipt = receipt.clone();
        move || {
            let receipt = receipt.clone();
            async move {
                let data_dir = platform::data_dir()?;
                tokio::task::spawn_blocking(move || receipt_edit::page_size(&data_dir, &receipt))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())
            }
        }
    });

    // `requested`: the button was tapped. Found on its own after opening,
    // the corners only replace untouched ones, never the user's.
    let detect = use_callback({
        let (db, receipt) = (db.clone(), receipt.clone());
        move |requested: bool| {
            detection.set(Detection::Running);
            let receipt = receipt.clone();
            // A setting that cannot be read counts as its default.
            let method = receipt_edit::corner_method(&db).unwrap_or_default();
            spawn(async move {
                let found = match platform::data_dir() {
                    Ok(data_dir) => tokio::task::spawn_blocking(move || {
                        receipt_edit::detect_page_corners(&data_dir, &receipt, method)
                            .map_err(|e| e.to_string())
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string())),
                    Err(message) => Err(message),
                };
                let current = *edit.peek();
                match found {
                    Ok(Some(corners)) => {
                        if requested || current.uncropped() == current {
                            edit.set(current.with_upright_corners(corners));
                        }
                        detection.set(Detection::Idle);
                    }
                    Ok(None) => detection.set(Detection::NotFound),
                    Err(message) => detection.set(Detection::Failed(message)),
                }
            });
        }
    });
    use_hook({
        let db = db.clone();
        move || {
            // A setting that cannot be read counts as its default: on.
            if receipt_edit::auto_corners(&db).unwrap_or(true) {
                detect.call(false);
            }
        }
    });

    let source = receipt
        .image_paths
        .first()
        .map(|path| receipts::file_url(path));
    let current = edit();
    let valid = current.is_valid();
    let busy = saving() || detection() == Detection::Running;

    let apply = {
        let (db, receipt) = (db.clone(), receipt.clone());
        move |_| {
            let chosen = edit();
            if chosen.is_unchanged() {
                on_done.call(receipt.clone());
                return;
            }
            saving.set(true);
            save_error.set(None);
            let (db, receipt) = (db.clone(), receipt.clone());
            spawn(async move {
                let outcome = match platform::data_dir() {
                    Ok(data_dir) => tokio::task::spawn_blocking(move || {
                        receipt_edit::save_edit(&db, &data_dir, &receipt, &chosen)
                            .map_err(|e| e.to_string())
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string())),
                    Err(message) => Err(message),
                };
                saving.set(false);
                match outcome {
                    Ok(files) => on_done.call(files),
                    Err(message) => save_error.set(Some(message)),
                }
            });
        }
    };

    let body = match &*size.read() {
        None => rsx! { Spinner { text: t!("adjust.loading").to_string() } },
        Some(Err(message)) => {
            let receipt = receipt.clone();
            rsx! {
                div { class: "flex flex-1 flex-col items-center justify-center gap-4 px-8 text-center", role: "alert",
                    Icon { icon: LdCircleAlert, class: "h-8 w-8 text-watermelon-300" }
                    p { class: "text-base text-floral-white-50", {t!("adjust.error_title").to_string()} }
                    p { class: "text-sm break-words text-floral-white-400", "{message}" }
                    Button {
                        class: "w-full",
                        onclick: move |_| on_done.call(receipt.clone()),
                        {t!("adjust.use_original").to_string()}
                    }
                }
            }
        }
        Some(Ok((width, height))) => {
            let turned = if current.quarter_turns % 2 == 1 {
                (f64::from(*height), f64::from(*width))
            } else {
                (f64::from(*width), f64::from(*height))
            };
            rsx! {
                div {
                    class: "relative min-h-0 flex-1 p-6",
                    onresize: move |event: Event<ResizeData>| {
                        if let Ok(size) = event.get_content_box_size() {
                            stage.set(Some((size.width, size.height)));
                        }
                    },
                    if let (Some(area), Some(src)) = (stage(), source.clone()) {
                        Stage { area, turned, src, edit, drag, valid }
                    }
                }
            }
        }
    };

    let status = match detection() {
        Detection::Running => Some((false, t!("adjust.detecting").to_string())),
        Detection::NotFound => Some((false, t!("adjust.not_found").to_string())),
        Detection::Failed(message) => {
            Some((true, t!("adjust.detect_error", error = message).to_string()))
        }
        Detection::Idle => None,
    };

    rsx! {
        div {
            class: "fixed inset-0 z-[1150] flex flex-col bg-jet-black-950 animate-fade-in touch-none select-none",
            style: "padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);",
            role: "dialog",
            aria_modal: "true",
            aria_label: t!("adjust.title").to_string(),
            // On the whole screen, so a corner keeps following a finger
            // that slides past the photo.
            onpointermove: move |event| {
                let Some(active) = drag() else { return };
                let Some((width, height)) = shown_size(stage(), size.read().as_ref(), edit().quarter_turns) else {
                    return;
                };
                if event.pointer_id() != active.pointer {
                    return;
                }
                let point = event.client_coordinates();
                let moved = Point::new(
                    (active.start.x + (point.x - active.from.0) / width).clamp(0.0, 1.0),
                    (active.start.y + (point.y - active.from.1) / height).clamp(0.0, 1.0),
                );
                edit.write().corners[active.corner] = moved;
            },
            onpointerup: move |_| drag.set(None),
            onpointercancel: move |_| drag.set(None),
            div { class: "flex min-h-14 items-center gap-2 px-3",
                button {
                    id: "invuso-viewer-close",
                    class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-floral-white-50 active:bg-jet-black-800 transition-colors",
                    r#type: "button",
                    aria_label: t!("common.close").to_string(),
                    disabled: saving(),
                    onclick: move |_| on_cancel.call(()),
                    Icon { icon: LdX, class: "h-6 w-6" }
                }
                h1 { class: "min-w-0 flex-1 truncate text-lg font-semibold text-floral-white-50", {t!("adjust.title").to_string()} }
            }
            p { class: "px-4 text-sm text-floral-white-300",
                if !valid {
                    span { class: "text-watermelon-300", role: "alert", {t!("adjust.invalid").to_string()} }
                } else if let Some((alert, text)) = status {
                    span {
                        class: if alert { "text-watermelon-300" } else { "text-floral-white-300" },
                        role: if alert { "alert" } else { "status" },
                        "{text}"
                    }
                } else {
                    {t!("adjust.hint").to_string()}
                }
            }
            {body}
            if let Some(message) = save_error() {
                p { class: "px-4 text-sm break-words text-watermelon-300", role: "alert",
                    {t!("adjust.save_error", error = message).to_string()}
                }
            }
            if matches!(*size.read(), Some(Ok(_))) {
                div { class: "flex flex-wrap items-center gap-2 px-4 pt-2",
                    Toggle {
                        label: t!("adjust.auto").to_string(),
                        pressed: false,
                        disabled: busy,
                        onclick: move |_| detect.call(true),
                        if detection() == Detection::Running {
                            div { class: "h-4 w-4 animate-spin rounded-full border-2 border-floral-white-300 border-t-transparent" }
                        } else {
                            Icon { icon: LdWandSparkles, class: "h-4 w-4" }
                        }
                    }
                    Toggle {
                        label: t!("adjust.contrast").to_string(),
                        pressed: current.contrast,
                        disabled: saving(),
                        onclick: move |_| edit.with_mut(|e| e.contrast = !e.contrast),
                        Icon { icon: LdContrast, class: "h-4 w-4" }
                    }
                    Toggle {
                        label: t!("adjust.sharpen").to_string(),
                        pressed: current.sharpen,
                        disabled: saving(),
                        onclick: move |_| edit.with_mut(|e| e.sharpen = !e.sharpen),
                        Icon { icon: LdFocus, class: "h-4 w-4" }
                    }
                }
                div { class: "flex items-center gap-2 px-4 pb-4 pt-3",
                    ToolButton {
                        label: t!("adjust.rotate_left").to_string(),
                        disabled: busy,
                        onclick: move |_| edit.set(edit().turned_counter_clockwise()),
                        Icon { icon: LdRotateCcw, class: "h-5 w-5" }
                    }
                    ToolButton {
                        label: t!("adjust.rotate_right").to_string(),
                        disabled: busy,
                        onclick: move |_| edit.set(edit().turned_clockwise()),
                        Icon { icon: LdRotateCw, class: "h-5 w-5" }
                    }
                    ToolButton {
                        label: t!("adjust.reset").to_string(),
                        disabled: busy,
                        // Keeps turn and filters; only the corners go back out.
                        onclick: move |_| edit.set(edit().uncropped()),
                        Icon { icon: LdMaximize, class: "h-5 w-5" }
                    }
                    Button {
                        class: "flex-1",
                        disabled: !valid || busy,
                        onclick: apply,
                        if saving() {
                            div { class: "h-5 w-5 animate-spin rounded-full border-2 border-cerulean-200 border-t-transparent" }
                            {t!("adjust.saving").to_string()}
                        } else {
                            Icon { icon: LdCheck, class: "h-5 w-5" }
                            {t!("adjust.apply").to_string()}
                        }
                    }
                }
            }
        }
    }
}

/// Size in pixels the turned photo is shown at: as large as fits the
/// stage.
fn shown_size(
    stage: Option<(f64, f64)>,
    size: Option<&Result<(u32, u32), String>>,
    quarter_turns: u8,
) -> Option<(f64, f64)> {
    let (area_w, area_h) = stage?;
    let &(w, h) = size?.as_ref().ok()?;
    let (w, h) = if quarter_turns % 2 == 1 {
        (f64::from(h), f64::from(w))
    } else {
        (f64::from(w), f64::from(h))
    };
    fit((area_w, area_h), (w, h))
}

/// `image` scaled to fit `area`, keeping its proportions.
fn fit(area: (f64, f64), image: (f64, f64)) -> Option<(f64, f64)> {
    if image.0 <= 0.0 || image.1 <= 0.0 || area.0 <= 0.0 || area.1 <= 0.0 {
        return None;
    }
    let scale = (area.0 / image.0).min(area.1 / image.1);
    Some((image.0 * scale, image.1 * scale))
}

/// Centre of the magnifier for a corner at `(x, y)` on a stage of `size`:
/// above the finger, below it near the top edge, always on screen.
fn loupe_centre((x, y): (f64, f64), size: (f64, f64)) -> (f64, f64) {
    let half = LOUPE_SIZE / 2.0;
    let above = y - LOUPE_OFFSET;
    let cy = if above - half >= -STAGE_PADDING {
        above
    } else {
        y + LOUPE_OFFSET
    };
    let cx = x.clamp(half - STAGE_PADDING, size.0 - half + STAGE_PADDING);
    (cx, cy)
}

/// The turned photo with the area outside the corners dimmed, the four
/// corner handles and, while one is dragged, a magnifier of its spot.
#[component]
fn Stage(
    area: (f64, f64),
    turned: (f64, f64),
    src: String,
    edit: Signal<ImageEdit>,
    drag: Signal<Option<Drag>>,
    valid: bool,
) -> Element {
    let Some((width, height)) = fit(area, turned) else {
        return rsx! {};
    };
    let current = edit();
    let points = current.corners.map(|p| (p.x * width, p.y * height));
    let outline = points
        .iter()
        .map(|(x, y)| format!("{x:.1},{y:.1}"))
        .collect::<Vec<_>>()
        .join(" ");
    let [a, b, c, d] = points;
    let shade = format!(
        "M0,0 H{width:.1} V{height:.1} H0 Z M{:.1},{:.1} L{:.1},{:.1} L{:.1},{:.1} L{:.1},{:.1} Z",
        a.0, a.1, b.0, b.1, c.0, c.1, d.0, d.1
    );
    let stroke = if valid {
        "fill-none stroke-cerulean-400"
    } else {
        "fill-none stroke-watermelon-400"
    };
    let active = drag().map(|d| d.corner);

    rsx! {
        div {
            class: "absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2",
            style: "width: {width}px; height: {height}px;",
            Photo { src: src.clone(), size: (width, height), edit: current, scale: 1.0 }
            svg {
                class: "absolute inset-0 h-full w-full overflow-visible",
                view_box: "0 0 {width} {height}",
                path { class: "fill-jet-black-950/60", fill_rule: "evenodd", d: "{shade}" }
                polygon { class: stroke, stroke_width: "2", points: "{outline}" }
            }
            for (index, (x, y)) in points.into_iter().enumerate() {
                button {
                    key: "{index}",
                    class: "absolute flex h-11 w-11 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full touch-none",
                    style: "left: {x}px; top: {y}px;",
                    r#type: "button",
                    aria_label: corner_label(index),
                    onpointerdown: move |event| {
                        let point = event.client_coordinates();
                        drag.set(Some(Drag {
                            corner: index,
                            pointer: event.pointer_id(),
                            from: (point.x, point.y),
                            start: edit.peek().corners[index],
                        }));
                    },
                    span {
                        class: "h-6 w-6 rounded-full border-2 border-floral-white-50 shadow-md",
                        class: if active == Some(index) { "bg-cerulean-300" } else { "bg-cerulean-500/70" },
                    }
                }
            }
            if let Some(index) = active {
                Loupe {
                    src,
                    size: (width, height),
                    edit: current,
                    corner: points[index],
                    outline: outline.clone(),
                    stroke,
                }
            }
        }
    }
}

/// The photo turned by the browser to fill a `size` box enlarged by
/// `scale`, with the contrast preview.
#[component]
fn Photo(src: String, size: (f64, f64), edit: ImageEdit, scale: f64) -> Element {
    let (width, height) = (size.0 * scale, size.1 * scale);
    // The browser shows the photo upright; turned by 90° it takes the
    // box's height as its width.
    let (image_w, image_h) = if edit.quarter_turns % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let degrees = u32::from(edit.quarter_turns) * 90;
    let filter = if edit.contrast {
        CONTRAST_PREVIEW
    } else {
        NO_PREVIEW
    };
    rsx! {
        img {
            class: "absolute left-1/2 top-1/2 max-w-none",
            style: "width: {image_w}px; height: {image_h}px; transform: translate(-50%, -50%) rotate({degrees}deg); {filter}",
            src: "{src}",
            alt: "",
            draggable: "false",
        }
    }
}

/// Round magnifier showing the photo around the dragged corner, with a
/// crosshair on the corner itself.
#[component]
fn Loupe(
    src: String,
    size: (f64, f64),
    edit: ImageEdit,
    corner: (f64, f64),
    outline: String,
    stroke: &'static str,
) -> Element {
    let (cx, cy) = loupe_centre(corner, size);
    let half = LOUPE_SIZE / 2.0;
    let (width, height) = (size.0 * LOUPE_ZOOM, size.1 * LOUPE_ZOOM);
    let left = half - corner.0 * LOUPE_ZOOM;
    let top = half - corner.1 * LOUPE_ZOOM;
    rsx! {
        div {
            class: "pointer-events-none absolute z-10 overflow-hidden rounded-full border-2 border-floral-white-50 bg-jet-black-950 shadow-lg",
            style: "width: {LOUPE_SIZE}px; height: {LOUPE_SIZE}px; left: {cx - half}px; top: {cy - half}px;",
            aria_hidden: "true",
            div {
                class: "absolute",
                style: "width: {width}px; height: {height}px; left: {left}px; top: {top}px;",
                Photo { src, size, edit, scale: LOUPE_ZOOM }
                svg {
                    class: "absolute inset-0 h-full w-full overflow-visible",
                    view_box: "0 0 {size.0} {size.1}",
                    polygon {
                        class: stroke,
                        stroke_width: "1.5",
                        vector_effect: "non-scaling-stroke",
                        points: "{outline}",
                    }
                }
            }
            span { class: "absolute left-1/2 top-1/2 h-5 w-px -translate-x-1/2 -translate-y-1/2 bg-cerulean-300" }
            span { class: "absolute left-1/2 top-1/2 h-px w-5 -translate-x-1/2 -translate-y-1/2 bg-cerulean-300" }
        }
    }
}

/// Pill that switches an option on and off.
#[component]
fn Toggle(
    label: String,
    pressed: bool,
    disabled: bool,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let colors = if pressed {
        "border-cerulean-500 bg-cerulean-800 text-floral-white-50"
    } else {
        "border-jet-black-700 bg-jet-black-900 text-floral-white-200 active:bg-jet-black-800"
    };
    rsx! {
        button {
            class: "flex min-h-11 items-center gap-2 rounded-full border px-4 text-sm font-medium transition-colors ease-apple disabled:opacity-50 {colors}",
            r#type: "button",
            aria_pressed: if pressed { "true" } else { "false" },
            disabled,
            onclick: move |event| onclick.call(event),
            {children}
            "{label}"
        }
    }
}

/// Round icon button of the tool row.
#[component]
fn ToolButton(
    label: String,
    disabled: bool,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    rsx! {
        button {
            class: "flex h-12 w-12 shrink-0 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-50 active:bg-jet-black-700 disabled:opacity-50 transition-colors",
            r#type: "button",
            aria_label: "{label}",
            title: "{label}",
            disabled,
            onclick: move |event| onclick.call(event),
            {children}
        }
    }
}

#[component]
fn Spinner(text: String) -> Element {
    rsx! {
        div { class: "flex flex-1 flex-col items-center justify-center gap-4 px-8 text-center", role: "status",
            div { class: "h-10 w-10 animate-spin rounded-full border-4 border-jet-black-700 border-t-cerulean-400" }
            p { class: "text-sm text-floral-white-300", "{text}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_fits_the_stage_keeping_its_proportions() {
        assert_eq!(fit((300.0, 600.0), (3000.0, 4000.0)), Some((300.0, 400.0)));
        assert_eq!(fit((800.0, 400.0), (3000.0, 4000.0)), Some((300.0, 400.0)));
        assert_eq!(fit((0.0, 400.0), (3000.0, 4000.0)), None);
        // Turned a quarter, a portrait photo is shown landscape.
        let size = Ok((3000, 4000));
        assert_eq!(
            shown_size(Some((400.0, 400.0)), Some(&size), 1),
            Some((400.0, 300.0))
        );
        assert_eq!(shown_size(None, Some(&size), 0), None);
    }

    #[test]
    fn magnifier_stays_clear_of_the_finger_and_on_screen() {
        let stage = (300.0, 500.0);
        // Above the finger in the middle …
        assert_eq!(loupe_centre((150.0, 300.0), stage), (150.0, 190.0));
        // … below it near the top, pulled in at the sides.
        assert_eq!(loupe_centre((0.0, 20.0), stage), (36.0, 130.0));
        assert_eq!(loupe_centre((300.0, 20.0), stage), (264.0, 130.0));
    }
}
