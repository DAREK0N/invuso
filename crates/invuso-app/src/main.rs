use std::rc::Rc;

use dioxus::history::{History, MemoryHistory};
use dioxus::prelude::*;
use dioxus::router::components::HistoryProvider;

mod components;
mod layouts;
mod platform;
mod preferences;
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

/// Provides the database to every screen and starts the router on the
/// screen [`start_route`] picks.
#[component]
fn AppRoot(db: Db) -> Element {
    let has_me = use_hook(|| db.me().map(|me| me.is_some()).map_err(|e| e.to_string()));
    use_context_provider(|| db);

    match has_me {
        // The router has no hook for its very first route, so the history it
        // reads starts there; `replace` after onboarding then leaves Home as
        // the only entry and back closes the app.
        Ok(has_me) => rsx! {
            HistoryProvider {
                history: move |_| {
                    Rc::new(MemoryHistory::with_initial_path(start_route(has_me))) as Rc<dyn History>
                },
                Router::<Route> {}
            }
        },
        Err(message) => rsx! { StartupError { message } },
    }
}

/// First screen after launch: onboarding until "Ich" exists (idee.md 7.5),
/// Home afterwards.
fn start_route(has_me: bool) -> Route {
    if has_me {
        Route::Home {}
    } else {
        Route::Onboarding {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_start_goes_to_onboarding() {
        assert_eq!(start_route(false), Route::Onboarding {});
        assert_eq!(start_route(false).to_string(), "/onboarding");
    }

    #[test]
    fn later_starts_go_home() {
        assert_eq!(start_route(true), Route::Home {});
        assert_eq!(start_route(true).to_string(), "/");
    }

    #[test]
    fn onboarding_decision_follows_the_database() {
        let db = Db::open_in_memory().unwrap();
        let has_me = |db: &Db| db.me().unwrap().is_some();
        assert_eq!(start_route(has_me(&db)), Route::Onboarding {});

        db.save_profile(&storage::Profile {
            name: "Konstantin".into(),
            home_currency: preferences::default_home_currency(),
            target_language: "de".into(),
        })
        .unwrap();
        assert_eq!(start_route(has_me(&db)), Route::Home {});
    }
}
