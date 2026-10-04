use dioxus::prelude::*;

mod components;
mod layouts;
mod platform;
mod storage;
mod views;

use dioxus_free_icons::{Icon, icons::ld_icons::LdDatabase};
use layouts::{AppShell, FocusShell};
use storage::Db;
use views::*;

#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en");

const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

/// All screens of the app (idee.md 6). Screens with the BottomNav live under
/// `AppShell`; capture and edit flows use `FocusShell` without it.
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(AppShell)]
        #[route("/")]
        Home {},

        #[route("/groups")]
        GroupList {},
        #[route("/groups/new")]
        GroupNew {},
        #[route("/groups/:id")]
        GroupOverview { id: String },
        #[route("/groups/:id/timeline")]
        GroupTimeline { id: String },
        #[route("/groups/:id/members")]
        GroupMembers { id: String },
        #[route("/groups/:id/settle")]
        GroupSettle { id: String },
        #[route("/groups/:id/edit")]
        GroupEdit { id: String },

        #[route("/expense/:id")]
        ExpenseDetail { id: String },

        #[route("/receipts")]
        ReceiptArchive {},
        #[route("/cash")]
        Cash {},

        #[route("/converter")]
        Converter {},
        #[route("/converter/history")]
        RateHistory {},

        #[route("/settings")]
        Settings {},
        #[route("/settings/people")]
        SettingsPeople {},
        #[route("/settings/payment-methods")]
        SettingsPaymentMethods {},
        #[route("/settings/categories")]
        SettingsCategories {},
        #[route("/settings/appearance")]
        SettingsAppearance {},
        #[route("/settings/languages")]
        SettingsLanguages {},
        #[route("/settings/data")]
        SettingsData {},
    #[end_layout]

    #[layout(FocusShell)]
        #[route("/expense/new")]
        ExpenseNew {},
        #[route("/expense/:id/edit")]
        ExpenseEdit { id: String },
        #[route("/scan")]
        Scan {},
        #[route("/scan/:receipt_id/review")]
        ReceiptReview { receipt_id: String },
        #[route("/onboarding")]
        Onboarding {},
        #[route("/:..route")]
        NotFound { route: Vec<String> },
}

fn main() {
    // German is the primary UI language until the language setting (SET-04)
    // exists; English stays the fallback for missing keys.
    rust_i18n::set_locale("de");
    dioxus::launch(App);
}

const DATABASE_FILE: &str = "invuso.sqlite3";

#[component]
fn App() -> Element {
    // Opened once per app start; migrations run here (CORE-04).
    let db = use_hook(|| {
        platform::data_dir()
            .and_then(|dir| Db::open(&dir.join(DATABASE_FILE)).map_err(|e| e.to_string()))
    });

    rsx! {
        document::Title { {t!("app.name").to_string()} }
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, viewport-fit=cover",
        }
        document::Stylesheet { href: TAILWIND_CSS }
        match db {
            Ok(db) => rsx! { AppRoot { db } },
            Err(message) => rsx! { StartupError { message } },
        }
    }
}

/// Provides the database to every screen and starts the router.
#[component]
fn AppRoot(db: Db) -> Element {
    use_context_provider(|| db);
    rsx! { Router::<Route> {} }
}

/// Shown instead of the app when the database cannot be opened, so the
/// user sees why rather than a crash.
#[component]
fn StartupError(message: String) -> Element {
    rsx! {
        div { class: "flex min-h-screen items-center justify-center app-header-inset",
            components::EmptyState {
                title: t!("startup.db_error_title").to_string(),
                text: format!("{}\n{message}", t!("startup.db_error_text")),
                Icon { icon: LdDatabase, class: "h-8 w-8" }
            }
        }
    }
}
