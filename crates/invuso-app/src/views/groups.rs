use dioxus::prelude::*;

use super::PlaceholderPage;

#[component]
pub fn GroupList() -> Element {
    rsx! { PlaceholderPage { title: t!("page.groups").to_string() } }
}

#[component]
pub fn GroupNew() -> Element {
    rsx! { PlaceholderPage { title: t!("page.group_new").to_string(), show_back: true } }
}

#[component]
pub fn GroupOverview(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.group_overview").to_string(), show_back: true } }
}

#[component]
pub fn GroupTimeline(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.group_timeline").to_string(), show_back: true } }
}

#[component]
pub fn GroupMembers(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.group_members").to_string(), show_back: true } }
}

#[component]
pub fn GroupSettle(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.group_settle").to_string(), show_back: true } }
}

#[component]
pub fn GroupEdit(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.group_edit").to_string(), show_back: true } }
}
