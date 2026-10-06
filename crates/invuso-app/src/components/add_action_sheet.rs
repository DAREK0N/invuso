use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdBanknote, LdCamera, LdHandCoins, LdImage, LdPencil},
};

use invuso_core::domain::GroupId;

use crate::Route;
use crate::components::BottomSheet;
use crate::platform::ImageKind;
use crate::services::receipts;
use crate::storage::{Db, LAST_EXPENSE_GROUP, StorageError};
use crate::views::{SOURCE_CAMERA, SOURCE_GALLERY};

/// Action sheet behind the central plus button (idee.md 7.1).
#[component]
pub fn AddActionSheet(on_close: EventHandler<()>) -> Element {
    // Photos need Android 10+ and a camera app; see `platform::images`.
    let can_take_photo = use_hook(|| receipts::supports(ImageKind::Camera));
    let db = use_context::<Db>();
    // Without any group there is nobody to settle with.
    let settle_group = use_hook(|| settle_target(&db).ok().flatten());

    rsx! {
        BottomSheet { title: t!("add_sheet.title").to_string(), on_close,
            div { class: "flex flex-col gap-1 px-3 pt-2",
                if can_take_photo {
                    ActionRow {
                        label: t!("add_sheet.scan").to_string(),
                        to: Route::Scan {
                            source: SOURCE_CAMERA.to_string(),
                        },
                        on_close,
                        Icon { icon: LdCamera, class: "h-5 w-5" }
                    }
                }
                ActionRow {
                    label: t!("add_sheet.gallery").to_string(),
                    to: Route::Scan {
                        source: SOURCE_GALLERY.to_string(),
                    },
                    on_close,
                    Icon { icon: LdImage, class: "h-5 w-5" }
                }
                ActionRow {
                    label: t!("add_sheet.manual").to_string(),
                    to: Route::ExpenseNew {
                        group: String::new(),
                        receipt: String::new(),
                        copy: String::new(),
                    },
                    on_close,
                    Icon { icon: LdPencil, class: "h-5 w-5" }
                }
                ActionRow {
                    label: t!("add_sheet.cash").to_string(),
                    to: Route::Cash { person: String::new() },
                    on_close,
                    Icon { icon: LdBanknote, class: "h-5 w-5" }
                }
                if let Some(group) = settle_group {
                    ActionRow {
                        label: t!("add_sheet.settle").to_string(),
                        to: Route::GroupSettle {
                            id: group.as_str().to_string(),
                            record: true,
                        },
                        on_close,
                        Icon { icon: LdHandCoins, class: "h-5 w-5" }
                    }
                }
            }
        }
    }
}

/// One tappable entry of the action sheet; `children` is its icon.
#[component]
fn ActionRow(label: String, to: Route, on_close: EventHandler<()>, children: Element) -> Element {
    let nav = use_navigator();

    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-4 rounded-2xl px-3 text-left text-floral-white-100 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| {
                on_close.call(());
                nav.push(to.clone());
            },
            span { class: "flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-cerulean-800 text-cerulean-200",
                {children}
            }
            span { class: "text-base font-medium", "{label}" }
        }
    }
}

/// Group a settlement from the plus button goes to (SPL-06): the active
/// group, else the group of the last expense, else the newest group – the
/// same order the expense form preselects in.
fn settle_target(db: &Db) -> Result<Option<GroupId>, StorageError> {
    if let Some(active) = db.active_group()? {
        return Ok(Some(active.id));
    }
    let groups = db.groups()?;
    let last = db.setting(LAST_EXPENSE_GROUP)?;
    Ok(last
        .and_then(|id| groups.iter().find(|g| g.id.as_str() == id))
        .or_else(|| groups.first())
        .map(|g| g.id.clone()))
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::Currency;

    use super::*;
    use crate::storage::{NewGroup, Profile};

    fn group(db: &Db, name: &str) -> GroupId {
        db.create_group(NewGroup {
            name: name.into(),
            icon: "plane".into(),
            color: "cerulean".into(),
            base_currency: Currency::from_code("EUR").unwrap(),
            start_date: None,
            end_date: None,
            target_language: None,
        })
        .unwrap()
        .id
    }

    #[test]
    fn settle_target_prefers_active_then_last_then_newest() {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: Currency::from_code("EUR").unwrap(),
            target_language: "de".into(),
        })
        .unwrap();
        assert_eq!(settle_target(&db).unwrap(), None);
        let trip = group(&db, "Japan");
        let flat = group(&db, "WG");
        assert_eq!(settle_target(&db).unwrap(), Some(flat.clone()));
        db.set_setting(LAST_EXPENSE_GROUP, trip.as_str()).unwrap();
        assert_eq!(settle_target(&db).unwrap(), Some(trip.clone()));
        // A personal expense last: fall back to the newest group.
        db.set_setting(LAST_EXPENSE_GROUP, "").unwrap();
        assert_eq!(settle_target(&db).unwrap(), Some(flat.clone()));
        db.set_active_group(Some(&trip)).unwrap();
        assert_eq!(settle_target(&db).unwrap(), Some(trip));
    }
}
