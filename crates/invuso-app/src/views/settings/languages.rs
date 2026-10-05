use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCheck, LdCircleAlert, LdDownload, LdLanguages, LdTrash2, LdX},
};

use crate::components::{Button, ButtonVariant, ConfirmSheet, ErrorBanner, TopBar};
use crate::platform;
use crate::preferences::language_name;
use crate::services::translation::TranslationConfidence;
use crate::services::translation::downloads::{PackDownload, PackDownloads};
use crate::services::translation::packs::{self, PACKS, Pack};
use crate::state::DataRevision;
use crate::storage::Db;

/// `/settings/languages`: translation packs to download and delete
/// (SET-07, decision 10.2) and how sure they must be to show a
/// translation. Nothing is downloaded without a tap here.
#[component]
pub fn SettingsLanguages() -> Element {
    let revision = use_context::<DataRevision>();
    let data_dir = use_hook(platform::data_dir);
    let installed = use_memo({
        let data_dir = data_dir.clone();
        move || {
            revision.track();
            PACKS
                .iter()
                .map(|pack| {
                    data_dir
                        .as_ref()
                        .is_ok_and(|dir| packs::is_installed(dir, pack))
                })
                .collect::<Vec<_>>()
        }
    });
    let mut deleting = use_signal(|| None::<&'static Pack>);
    let mut delete_error = use_signal(|| None::<String>);

    let delete_dir = data_dir.clone();
    let confirm_delete = move |_| {
        let Some(pack) = deleting() else {
            return;
        };
        let result = match &delete_dir {
            Ok(dir) => packs::delete(dir, pack).map_err(|e| e.to_string()),
            Err(message) => Err(message.clone()),
        };
        match result {
            Ok(()) => {
                deleting.set(None);
                let mut revision = revision;
                revision.bump();
            }
            Err(message) => delete_error.set(Some(message)),
        }
    };

    rsx! {
        TopBar { title: t!("page.settings_languages").to_string(), show_back: true }
        div { class: "mx-4 flex flex-col gap-4 pt-4 safe-area-x",
            p { class: "px-1 text-sm text-floral-white-400", {t!("packs.intro").to_string()} }
            if let Err(message) = &data_dir {
                ErrorBanner { error: Some(message.clone()) }
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                for (pack, installed) in PACKS.iter().zip(installed()) {
                    PackRow {
                        key: "{pack.id()}",
                        pack,
                        installed,
                        on_delete: move |pack| {
                            delete_error.set(None);
                            deleting.set(Some(pack));
                        },
                    }
                }
            }
            ConfidenceSection {}
            p { class: "px-1 text-xs text-floral-white-500", {t!("packs.source").to_string()} }
        }
        if let Some(pack) = deleting() {
            ConfirmSheet {
                title: t!("packs.delete_title").to_string(),
                text: t!("packs.delete_text", pair = pair_name(pack)).to_string(),
                confirm_label: t!("common.delete").to_string(),
                error: delete_error(),
                on_confirm: confirm_delete,
                on_close: move |_| deleting.set(None),
            }
        }
    }
}

/// How sure a pack must be before its translation is shown.
#[component]
fn ConfidenceSection() -> Element {
    let db = use_context::<Db>();
    let read_db = db.clone();
    let mut level = use_signal(move || TranslationConfidence::current(&read_db));
    let mut error = use_signal(|| None::<String>);

    rsx! {
        section { class: "flex flex-col gap-2",
            h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                {t!("packs.confidence_title").to_string()}
            }
            div {
                class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                role: "radiogroup",
                aria_label: t!("packs.confidence_title").to_string(),
                for option in TranslationConfidence::ALL {
                    button {
                        key: "{option.code()}",
                        class: "flex min-h-14 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
                        r#type: "button",
                        role: "radio",
                        aria_checked: if level() == option { "true" } else { "false" },
                        onclick: {
                            let db = db.clone();
                            move |_| match option.save(&db) {
                                Ok(()) => {
                                    level.set(option);
                                    error.set(None);
                                }
                                Err(e) => error.set(Some(e.to_string())),
                            }
                        },
                        span { class: "flex min-w-0 flex-1 flex-col",
                            span { class: "text-base text-floral-white-50", {confidence_label(option)} }
                            span { class: "text-sm text-floral-white-400", {confidence_hint(option)} }
                        }
                        if level() == option {
                            Icon { icon: LdCheck, class: "h-5 w-5 shrink-0 text-cerulean-300" }
                        }
                    }
                }
            }
            ErrorBanner { error: error() }
            p { class: "px-1 text-xs text-floral-white-500", {t!("packs.confidence_note").to_string()} }
        }
    }
}

fn confidence_label(level: TranslationConfidence) -> String {
    match level {
        TranslationConfidence::Strict => t!("packs.confidence_strict"),
        TranslationConfidence::Balanced => t!("packs.confidence_balanced"),
        TranslationConfidence::All => t!("packs.confidence_all"),
    }
    .to_string()
}

fn confidence_hint(level: TranslationConfidence) -> String {
    match level {
        TranslationConfidence::Strict => t!("packs.confidence_strict_hint"),
        TranslationConfidence::Balanced => t!("packs.confidence_balanced_hint"),
        TranslationConfidence::All => t!("packs.confidence_all_hint"),
    }
    .to_string()
}

/// "Japanisch → Deutsch".
fn pair_name(pack: &Pack) -> String {
    format!(
        "{} → {}",
        language_name(pack.source),
        language_name(pack.target)
    )
}

/// Megabytes, rounded, as shown to the user.
fn megabytes(bytes: u64) -> String {
    t!("packs.megabytes", size = bytes.div_ceil(1_000_000)).to_string()
}

#[component]
fn PackRow(
    pack: &'static Pack,
    installed: bool,
    on_delete: EventHandler<&'static Pack>,
) -> Element {
    let revision = use_context::<DataRevision>();
    let downloads = use_context::<PackDownloads>();
    let state = downloads.get(pack);

    let status = match (&state, installed) {
        (Some(PackDownload::Running { done, total }), _) => t!(
            "packs.downloading",
            done = done / 1_000_000,
            total = total.div_ceil(1_000_000)
        )
        .to_string(),
        (_, true) => t!("packs.installed", size = megabytes(pack.size())).to_string(),
        _ => megabytes(pack.size()),
    };

    rsx! {
        div { class: "flex flex-col gap-3 border-b border-jet-black-800 px-4 py-3 last:border-b-0",
            div { class: "flex items-center gap-3",
                Icon { icon: LdLanguages, class: "h-5 w-5 shrink-0 text-cerulean-300" }
                div { class: "flex min-w-0 flex-1 flex-col",
                    span { class: "text-base text-floral-white-50", {pair_name(pack)} }
                    span { class: "text-sm tabular-nums text-floral-white-400", "{status}" }
                }
                match (&state, installed) {
                    (Some(PackDownload::Running { .. }), _) => rsx! {
                        button {
                            class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-floral-white-300 active:bg-jet-black-800 transition-colors",
                            r#type: "button",
                            aria_label: t!("packs.cancel").to_string(),
                            onclick: move |_| downloads.cancel(pack),
                            Icon { icon: LdX, class: "h-5 w-5" }
                        }
                    },
                    (_, true) => rsx! {
                        button {
                            class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-watermelon-300 active:bg-jet-black-800 transition-colors",
                            r#type: "button",
                            aria_label: t!("common.delete").to_string(),
                            onclick: move |_| on_delete.call(pack),
                            Icon { icon: LdTrash2, class: "h-5 w-5" }
                        }
                    },
                    _ => rsx! {
                        Button {
                            variant: ButtonVariant::Secondary,
                            onclick: move |_| {
                                downloads.clear(pack);
                                downloads.start(pack, revision);
                            },
                            Icon { icon: LdDownload, class: "h-4 w-4" }
                            {t!("packs.download").to_string()}
                        }
                    },
                }
            }
            if let Some(PackDownload::Running { done, total }) = state {
                div {
                    class: "h-1.5 w-full overflow-hidden rounded-full bg-jet-black-700",
                    role: "progressbar",
                    aria_valuemin: "0",
                    aria_valuemax: "100",
                    aria_valuenow: "{done * 100 / total.max(1)}",
                    div {
                        class: "h-full rounded-full bg-cerulean-400 transition-[width] ease-apple",
                        style: "width: {done * 100 / total.max(1)}%",
                    }
                }
            }
            if let Some(PackDownload::Failed(message)) = state {
                div { class: "flex items-start gap-2", role: "alert",
                    Icon { icon: LdCircleAlert, class: "mt-0.5 h-4 w-4 shrink-0 text-watermelon-300" }
                    p { class: "text-sm break-words text-watermelon-300",
                        {t!("packs.failed", message = message).to_string()}
                    }
                }
            }
        }
    }
}
