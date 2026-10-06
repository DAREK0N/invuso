use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdChevronDown, LdCircleAlert, LdEye, LdEyeOff, LdPencil, LdPlus, LdTag, LdTrash2,
    },
};
use invuso_core::domain::Category;

use crate::components::{
    BottomSheet, Button, CategoryIcon, ColorPicker, ConfirmSheet, EmptyState, ErrorBanner,
    IconPicker, IconSet, ListItem, MenuRow, TextField, TopBar,
};
use crate::preferences::{DEFAULT_CATEGORY_ICON, category_name, suggested_person_color};
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::{Db, NewCategory};

/// What the category form sheet is doing.
#[derive(Debug, Clone, PartialEq)]
enum Form {
    New,
    Edit(Category),
}

/// Everything the page lists.
#[derive(Debug, Clone, PartialEq)]
struct Overview {
    categories: Vec<Category>,
    /// Default categories the user has hidden.
    hidden: Vec<Category>,
}

/// `/settings/categories`: the default categories and the user's own ones
/// (EXP-09, SET-06). Tap edits, long-press opens a menu (UI-09). Default
/// categories are hidden instead of deleted and can be shown again from
/// the folded section at the bottom.
#[component]
pub fn SettingsCategories() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let mut form = use_signal(|| None::<Form>);
    let mut menu = use_signal(|| None::<Category>);
    let mut confirm_delete = use_signal(|| None::<Category>);
    let mut delete_error = use_signal(|| None::<String>);
    let mut show_hidden = use_signal(|| false);

    let menu_db = db.clone();
    let overview = use_memo(move || {
        revision.track();
        Ok::<_, String>(Overview {
            categories: db.categories().map_err(|e| e.to_string())?,
            hidden: db.hidden_categories().map_err(|e| e.to_string())?,
        })
    });

    rsx! {
        TopBar { title: t!("page.settings_categories").to_string(), show_back: true }
        match &*overview.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("categories.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(overview) => rsx! {
                div { class: "mx-4 flex flex-col gap-4 pt-4 safe-area-x",
                    Button { class: "w-full", onclick: move |_| form.set(Some(Form::New)),
                        Icon { icon: LdPlus, class: "h-5 w-5" }
                        {t!("categories.add").to_string()}
                    }
                    if overview.categories.is_empty() {
                        EmptyState {
                            title: t!("categories.empty_title").to_string(),
                            text: t!("categories.empty_text").to_string(),
                            Icon { icon: LdTag, class: "h-8 w-8" }
                        }
                    } else {
                        div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                            for category in overview.categories.iter().cloned() {
                                CategoryRow {
                                    key: "{category.id.as_str()}",
                                    category,
                                    on_open: move |category| form.set(Some(Form::Edit(category))),
                                    on_long_press: move |category| menu.set(Some(category)),
                                }
                            }
                        }
                        p { class: "px-1 text-sm text-floral-white-500", {t!("categories.long_press_hint").to_string()} }
                    }
                    if !overview.hidden.is_empty() {
                        HiddenSection {
                            categories: overview.hidden.clone(),
                            open: show_hidden(),
                            on_toggle: move |_| show_hidden.toggle(),
                            on_open: move |category| menu.set(Some(category)),
                        }
                    }
                }
            },
        }
        if let Some(target) = form() {
            CategoryFormSheet {
                category: match target {
                    Form::New => None,
                    Form::Edit(category) => Some(category),
                },
                on_saved: move |_| form.set(None),
                on_close: move |_| form.set(None),
            }
        }
        if let Some(category) = menu() {
            CategoryMenu {
                hidden: overview.read().as_ref().is_ok_and(|o| o.hidden.contains(&category)),
                category,
                on_edit: move |category| {
                    menu.set(None);
                    form.set(Some(Form::Edit(category)));
                },
                on_hide: {
                    let db = menu_db.clone();
                    move |(category, hide): (Category, bool)| {
                        menu.set(None);
                        set_hidden_with_undo(&db, &category, hide, revision, toaster);
                    }
                },
                on_delete: move |category| {
                    menu.set(None);
                    delete_error.set(None);
                    confirm_delete.set(Some(category));
                },
                on_close: move |_| menu.set(None),
            }
        }
        if let Some(category) = confirm_delete() {
            DeleteCategorySheet {
                category,
                error: delete_error(),
                on_deleted: move |_| confirm_delete.set(None),
                on_error: move |message| delete_error.set(Some(message)),
                on_close: move |_| confirm_delete.set(None),
            }
        }
    }
}

/// List row: icon on the category's color and its name; default
/// categories are marked as such.
#[component]
fn CategoryRow(
    category: Category,
    on_open: EventHandler<Category>,
    on_long_press: EventHandler<Category>,
) -> Element {
    let open_target = category.clone();
    let menu_target = category.clone();

    rsx! {
        ListItem {
            title: category_name(&category),
            subtitle: category.is_default.then(|| t!("categories.default").to_string()),
            onclick: move |_| on_open.call(open_target.clone()),
            on_long_press: move |_| on_long_press.call(menu_target.clone()),
            CategoryIcon { icon: category.icon.clone(), color: category.color.clone() }
        }
    }
}

/// Folded list of hidden default categories; tapping one offers to show
/// it again.
#[component]
fn HiddenSection(
    categories: Vec<Category>,
    open: bool,
    on_toggle: EventHandler<()>,
    on_open: EventHandler<Category>,
) -> Element {
    let count = categories.len();

    rsx! {
        section { class: "flex flex-col gap-2",
            button {
                class: "flex min-h-11 items-center gap-2 px-1 text-left text-sm font-semibold text-floral-white-400 active:text-floral-white-200",
                r#type: "button",
                aria_expanded: if open { "true" } else { "false" },
                onclick: move |_| on_toggle.call(()),
                Icon { icon: LdEyeOff, class: "h-4 w-4" }
                span { class: "flex-1", {t!("categories.hidden", count = count).to_string()} }
                Icon {
                    icon: LdChevronDown,
                    class: if open { "h-5 w-5 rotate-180 transition-transform" } else { "h-5 w-5 transition-transform" },
                }
            }
            if open {
                div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900 opacity-70",
                    for category in categories.iter().cloned() {
                        ListItem {
                            key: "{category.id.as_str()}",
                            title: category_name(&category),
                            onclick: {
                                let category = category.clone();
                                move |_| on_open.call(category.clone())
                            },
                            CategoryIcon { icon: category.icon.clone(), color: category.color.clone() }
                        }
                    }
                }
            }
        }
    }
}

/// Menu of a category: edit and delete for own ones, edit and hide for
/// default ones, "show again" for hidden ones.
#[component]
fn CategoryMenu(
    category: Category,
    hidden: bool,
    on_edit: EventHandler<Category>,
    on_hide: EventHandler<(Category, bool)>,
    on_delete: EventHandler<Category>,
    on_close: EventHandler<()>,
) -> Element {
    let edit_target = category.clone();
    let hide_target = category.clone();
    let delete_target = category.clone();

    rsx! {
        BottomSheet { title: category_name(&category), on_close,
            div { class: "flex flex-col gap-1 px-3 pt-2",
                if hidden {
                    MenuRow {
                        label: t!("categories.show").to_string(),
                        onclick: move |_| on_hide.call((hide_target.clone(), false)),
                        Icon { icon: LdEye, class: "h-5 w-5" }
                    }
                } else {
                    MenuRow {
                        label: t!("common.edit").to_string(),
                        onclick: move |_| on_edit.call(edit_target.clone()),
                        Icon { icon: LdPencil, class: "h-5 w-5" }
                    }
                    if category.is_default {
                        MenuRow {
                            label: t!("categories.hide").to_string(),
                            onclick: move |_| on_hide.call((hide_target.clone(), true)),
                            Icon { icon: LdEyeOff, class: "h-5 w-5" }
                        }
                    } else {
                        MenuRow {
                            label: t!("common.delete").to_string(),
                            danger: true,
                            onclick: move |_| on_delete.call(delete_target.clone()),
                            Icon { icon: LdTrash2, class: "h-5 w-5" }
                        }
                    }
                }
            }
        }
    }
}

/// Hides a default category or shows it again, with "Undo" in a toast.
fn set_hidden_with_undo(
    db: &Db,
    category: &Category,
    hide: bool,
    mut revision: DataRevision,
    mut toaster: Toaster,
) {
    let apply = |db: &Db, hide: bool| {
        if hide {
            db.delete_category(&category.id)
        } else {
            db.restore_category(&category.id)
        }
    };
    if let Err(e) = apply(db, hide) {
        toaster.show(format!("{} {e}", t!("profile.save_error")), None);
        return;
    }
    revision.bump();

    let db = db.clone();
    let id = category.id.clone();
    let undo = move || {
        let (mut revision, mut toaster) = (revision, toaster);
        let result = if hide {
            db.restore_category(&id)
        } else {
            db.delete_category(&id)
        };
        match result {
            Ok(()) => revision.bump(),
            Err(e) => toaster.show(format!("{} {e}", t!("profile.save_error")), None),
        }
    };
    let name = category_name(category);
    let message = if hide {
        t!("categories.hidden_toast", name = name)
    } else {
        t!("categories.shown_toast", name = name)
    };
    toaster.show(
        message.to_string(),
        Some(ToastAction {
            label: t!("common.undo").to_string(),
            run: Rc::new(undo),
        }),
    );
}

/// The category as the form saves it. A default category whose shown name
/// the user changed keeps the typed name from now on instead of the
/// translated one (user decision in AP-28).
fn edited(category: &Category, typed_name: &str, icon: String, color: String) -> Category {
    let renamed = category.is_default && typed_name.trim() != category_name(category);
    Category {
        name: if category.is_default && !renamed {
            category.name.clone()
        } else {
            typed_name.trim().to_string()
        },
        is_default: category.is_default && !renamed,
        icon,
        color,
        ..category.clone()
    }
}

/// Bottom sheet to create (`category: None`) or edit a category: name,
/// color and icon (EXP-09).
#[component]
fn CategoryFormSheet(
    category: Option<Category>,
    on_saved: EventHandler<Category>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let is_new = category.is_none();
    let is_default = category.as_ref().is_some_and(|c| c.is_default);

    let initial = category.clone();
    let mut name = use_signal(|| initial.as_ref().map(category_name).unwrap_or_default());
    let initial = category.clone();
    let suggest_db = db.clone();
    let mut color = use_signal(move || match &initial {
        Some(category) => category.color.clone(),
        None => {
            let categories = suggest_db.categories().unwrap_or_default();
            suggested_person_color(categories.iter().map(|c| c.color.as_str())).to_string()
        }
    });
    let initial = category.clone();
    let mut icon = use_signal(|| initial.map_or(DEFAULT_CATEGORY_ICON.to_string(), |c| c.icon));
    let mut name_error = use_signal(|| None::<String>);
    let mut save_error = use_signal(|| None::<String>);

    let title = if is_new {
        t!("categories.new_title").to_string()
    } else {
        t!("categories.edit_title").to_string()
    };

    let save = move |_| {
        if name.read().trim().is_empty() {
            name_error.set(Some(t!("categories.name_required").to_string()));
            return;
        }
        let result = match &category {
            None => db.create_category(NewCategory {
                name: name(),
                icon: icon(),
                color: color(),
            }),
            Some(existing) => {
                let updated = edited(existing, &name(), icon(), color());
                db.update_category(&updated).map(|()| updated)
            }
        };
        match result {
            Ok(saved) => {
                revision.bump();
                on_saved.call(saved);
            }
            Err(error) => save_error.set(Some(format!("{} {error}", t!("profile.save_error")))),
        }
    };

    rsx! {
        BottomSheet { title, on_close,
            div { class: "flex max-h-[75vh] flex-col gap-5 overflow-y-auto overscroll-contain px-5 pt-3",
                TextField {
                    id: "category-name",
                    label: t!("categories.name").to_string(),
                    value: name(),
                    placeholder: t!("categories.name_placeholder").to_string(),
                    error: name_error(),
                    oninput: move |value| {
                        name.set(value);
                        name_error.set(None);
                    },
                }
                if is_default {
                    p { class: "-mt-3 px-1 text-sm text-floral-white-500", {t!("categories.rename_hint").to_string()} }
                }
                ColorPicker {
                    label: t!("categories.color").to_string(),
                    selected: color(),
                    on_select: move |value| color.set(value),
                }
                IconPicker {
                    label: t!("categories.icon").to_string(),
                    selected: icon(),
                    color: color(),
                    set: IconSet::Category,
                    on_select: move |value| icon.set(value),
                }
                ErrorBanner { error: save_error() }
                Button { class: "w-full", onclick: save, {t!("common.save").to_string()} }
            }
        }
    }
}

/// Confirmation before deleting an own category. Deleting is soft: past
/// expenses keep showing it, and a toast offers "Undo" (UI-11).
#[component]
fn DeleteCategorySheet(
    category: Category,
    error: Option<String>,
    on_deleted: EventHandler<()>,
    on_error: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let name = category_name(&category);

    rsx! {
        ConfirmSheet {
            title: t!("categories.delete_title").to_string(),
            text: t!("categories.delete_text", name = name).to_string(),
            confirm_label: t!("common.delete").to_string(),
            error,
            on_confirm: move |_| match delete_with_undo(&db, &category, revision, toaster) {
                Ok(()) => on_deleted.call(()),
                Err(message) => on_error.call(message),
            },
            on_close,
        }
    }
}

/// Soft-deletes the category and offers to restore it from a toast.
fn delete_with_undo(
    db: &Db,
    category: &Category,
    mut revision: DataRevision,
    mut toaster: Toaster,
) -> Result<(), String> {
    db.delete_category(&category.id)
        .map_err(|e| format!("{} {e}", t!("categories.delete_error")))?;
    revision.bump();

    let db = db.clone();
    let id = category.id.clone();
    let undo = move || {
        let (mut revision, mut toaster) = (revision, toaster);
        match db.restore_category(&id) {
            Ok(()) => revision.bump(),
            Err(e) => toaster.show(format!("{} {e}", t!("categories.restore_error")), None),
        }
    };
    toaster.show(
        t!("categories.deleted", name = category_name(category)).to_string(),
        Some(ToastAction {
            label: t!("common.undo").to_string(),
            run: Rc::new(undo),
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::CategoryId;

    use super::*;

    fn category(name: &str, is_default: bool) -> Category {
        Category {
            id: CategoryId::new("c"),
            name: name.into(),
            icon: "tag".into(),
            color: "cerulean".into(),
            is_default,
        }
    }

    #[test]
    fn a_default_keeps_its_key_until_renamed() {
        let food = category("food", true);
        let shown = format!(" {} ", category_name(&food));
        let kept = edited(&food, &shown, "pizza".into(), "pale-oak".into());
        assert_eq!(kept.name, "food");
        assert!(kept.is_default);
        assert_eq!(kept.icon, "pizza");

        let renamed = edited(&food, " Restaurants ", "pizza".into(), "pale-oak".into());
        assert_eq!(renamed.name, "Restaurants");
        assert!(!renamed.is_default);
    }

    #[test]
    fn own_categories_take_the_typed_name() {
        let own = category("Souvenirs", false);
        let renamed = edited(&own, " Mitbringsel ", "gift".into(), "thistle".into());
        assert_eq!(renamed.name, "Mitbringsel");
        assert!(!renamed.is_default);
    }
}
