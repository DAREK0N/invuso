use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronRight, LdCircleAlert, LdPencil, LdTrash2, LdUserX},
};
use invuso_core::domain::PersonId;

use super::people::{DeletePersonSheet, PersonFormSheet};
use crate::Route;
use crate::components::{
    Avatar, AvatarSize, Button, ButtonVariant, CardSection, EmptyState, ErrorBanner, GroupIcon,
    MoneyText, TopBar, color_classes,
};
use crate::preferences::color_name;
use crate::services::summary::{PersonGroupBalance, person_balances, total_by_currency};
use crate::state::DataRevision;
use crate::storage::Db;

/// `/settings/people/:id`: name, color and note of a person, the groups with
/// the person's balance in each and in total, edit and delete (PER-01,
/// PER-03 without expenses and preferred payment methods).
#[component]
pub fn PersonDetail(id: String) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut editing = use_signal(|| false);
    let mut confirm_delete = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    let person_id = use_memo(use_reactive!(|id| PersonId::new(id)));
    let balances_db = db.clone();
    let person = use_memo(move || {
        revision.track();
        db.person(&person_id()).map_err(|e| e.to_string())
    });
    let balances = use_memo(move || {
        revision.track();
        person_balances(&balances_db, &person_id())
            .map_err(|e| format!("{} {e}", t!("people.balances_error_title")))
    });

    rsx! {
        TopBar { title: t!("page.person_detail").to_string(), show_back: true }
        match &*person.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("people.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(None) => rsx! {
                EmptyState {
                    title: t!("people.not_found_title").to_string(),
                    text: t!("people.not_found_text").to_string(),
                    Icon { icon: LdUserX, class: "h-8 w-8" }
                }
            },
            Ok(Some(person)) => rsx! {
                div { class: "mx-4 flex flex-col gap-4 pt-6 safe-area-x",
                    div { class: "flex flex-col items-center gap-3 text-center",
                        Avatar { name: person.name.clone(), color: person.color.clone(), size: AvatarSize::Lg }
                        h2 { class: "text-2xl font-semibold break-words text-floral-white-50", "{person.name}" }
                        if person.is_me {
                            span { class: "rounded-full bg-cerulean-800 px-3 py-1 text-sm font-medium text-cerulean-200",
                                {t!("people.me_badge").to_string()}
                            }
                        }
                    }
                    div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                        DetailRow { label: t!("people.color").to_string(),
                            span { class: "flex items-center gap-2",
                                span { class: "h-4 w-4 rounded-full {color_classes(&person.color)}" }
                                {color_name(&person.color)}
                            }
                        }
                        DetailRow { label: t!("people.note").to_string(),
                            match &person.note {
                                Some(note) => rsx! { span { class: "whitespace-pre-line break-words", "{note}" } },
                                None => rsx! { span { class: "text-floral-white-500", {t!("people.no_note").to_string()} } },
                            }
                        }
                    }
                    match &*balances.read() {
                        Ok(balances) => rsx! { GroupBalances { balances: balances.clone() } },
                        Err(message) => rsx! { ErrorBanner { error: Some(message.clone()) } },
                    }
                    div { class: "flex flex-col gap-3 pt-2",
                        Button {
                            variant: ButtonVariant::Secondary,
                            class: "w-full",
                            onclick: move |_| editing.set(true),
                            Icon { icon: LdPencil, class: "h-5 w-5" }
                            {t!("common.edit").to_string()}
                        }
                        if person.is_me {
                            p { class: "px-1 text-center text-sm text-floral-white-500",
                                {t!("people.me_not_deletable").to_string()}
                            }
                        } else {
                            Button {
                                variant: ButtonVariant::Danger,
                                class: "w-full",
                                onclick: move |_| {
                                    delete_error.set(None);
                                    confirm_delete.set(true);
                                },
                                Icon { icon: LdTrash2, class: "h-5 w-5" }
                                {t!("common.delete").to_string()}
                            }
                        }
                    }
                }
                if editing() {
                    PersonFormSheet {
                        person: Some(person.clone()),
                        on_saved: move |_| editing.set(false),
                        on_close: move |_| editing.set(false),
                    }
                }
                if confirm_delete() {
                    DeletePersonSheet {
                        person: person.clone(),
                        error: delete_error(),
                        on_deleted: move |_| {
                            confirm_delete.set(false);
                            // The undo toast stays visible on the list.
                            if nav.can_go_back() {
                                nav.go_back();
                            } else {
                                nav.replace(Route::SettingsPeople {});
                            }
                        },
                        on_error: move |message| delete_error.set(Some(message)),
                        on_close: move |_| confirm_delete.set(false),
                    }
                }
            },
        }
    }
}

/// Label above a value inside the detail card; `children` is the value.
#[component]
fn DetailRow(label: String, children: Element) -> Element {
    rsx! {
        div { class: "flex min-h-14 flex-col justify-center gap-0.5 border-b border-jet-black-800 px-4 py-2 last:border-b-0",
            span { class: "text-sm text-floral-white-400", "{label}" }
            span { class: "text-base text-floral-white-50", {children} }
        }
    }
}

/// The person's balance in each group and summed per currency (PER-03).
#[component]
fn GroupBalances(balances: Vec<PersonGroupBalance>) -> Element {
    let nav = use_navigator();
    let totals = total_by_currency(&balances);

    rsx! {
        CardSection { title: t!("people.groups").to_string(),
            if balances.is_empty() {
                p { class: "px-4 py-4 text-base text-floral-white-300", {t!("people.no_groups").to_string()} }
            }
            for entry in balances {
                button {
                    key: "{entry.group.id.as_str()}",
                    class: "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
                    r#type: "button",
                    onclick: {
                        let id = entry.group.id.as_str().to_string();
                        move |_| {
                            nav.push(Route::GroupOverview { id: id.clone() });
                        }
                    },
                    GroupIcon { icon: entry.group.icon.clone(), color: entry.group.color.clone() }
                    span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{entry.group.name}" }
                    MoneyText { amount: entry.balance(), signed: true, class: "text-base font-semibold" }
                    Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
                }
            }
        }
        if !totals.is_empty() {
            div { class: "flex min-h-14 items-center gap-3 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-4 py-2",
                span { class: "flex-1 text-base text-floral-white-50", {t!("people.total_balance").to_string()} }
                span { class: "flex flex-col items-end",
                    for total in totals {
                        MoneyText {
                            key: "{total.currency().code()}",
                            amount: total,
                            signed: true,
                            class: "text-base font-semibold",
                        }
                    }
                }
            }
        }
    }
}
