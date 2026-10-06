use std::rc::Rc;

use dioxus::history::{History, MemoryHistory};
use dioxus::prelude::*;
use dioxus::router::components::HistoryProvider;

mod appearance;
mod clock;
mod components;
mod format;
mod layouts;
mod platform;
mod preferences;
mod services;
mod state;
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
        // `setup` marks the step right after creating the group (AP-08).
        #[route("/groups/:id/members?:setup")]
        GroupMembers { id: String, setup: bool },
        // `record` opens the sheet for a new settlement (plus button, AP-23).
        #[route("/groups/:id/settle?:record")]
        GroupSettle { id: String, record: bool },
        #[route("/groups/:id/edit")]
        GroupEdit { id: String },

        #[route("/expense/:id")]
        ExpenseDetail { id: String },

        #[route("/receipts")]
        ReceiptArchive {},
        // `person` empty shows the cash of "Ich" (AP-22).
        #[route("/cash?:person")]
        Cash { person: String },

        #[route("/converter")]
        Converter {},
        #[route("/converter/history")]
        RateHistory {},

        #[route("/settings")]
        Settings {},
        #[route("/settings/people")]
        SettingsPeople {},
        #[route("/settings/people/:id")]
        PersonDetail { id: String },
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
        #[route("/settings/licenses")]
        SettingsLicenses {},
    #[end_layout]

    #[layout(FocusShell)]
        // `group` preselects a group, e.g. from its timeline (GRP-23);
        // `receipt` attaches a receipt just photographed (RCP-03).
        #[route("/expense/new?:group&:receipt")]
        ExpenseNew { group: String, receipt: String },
        #[route("/expense/:id/edit")]
        ExpenseEdit { id: String },
        // `source` is `camera` or `gallery` (RCP-01, RCP-02).
        #[route("/scan?:source")]
        Scan { source: String },
        #[route("/scan/:receipt_id/review")]
        ReceiptReview { receipt_id: String },
        #[route("/onboarding")]
        Onboarding {},
        #[route("/:..route")]
        NotFound { route: Vec<String> },
}

fn main() {
    // The stored app language (SET-04) is set in `AppRoot`, once the
    // database is open; English stays the fallback for missing keys.
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

/// Provides the database and the shared state to every screen and starts
/// the router on the screen [`start_route`] picks.
#[component]
fn AppRoot(db: Db) -> Element {
    let has_me = use_hook(|| db.me().map(|me| me.is_some()).map_err(|e| e.to_string()));
    let ui_language =
        use_context_provider(|| appearance::UiLanguage::new(appearance::startup_locale(&db)));
    let look = use_context_provider(|| Signal::new(appearance::Appearance::load(&db)));
    use_effect(move || appearance::apply(look(), ui_language.locale()));
    use_context_provider(|| db);
    use_context_provider(state::DataRevision::new);
    use_context_provider(state::Toaster::new);
    use_context_provider(state::RateStatus::new);
    use_context_provider(services::ocr::OcrJobs::new);
    use_context_provider(services::translation::downloads::PackDownloads::new);
    services::rates::use_rate_refresh();
    services::receipts::use_receipt_files();

    match has_me {
        // The router has no hook for its very first route, so the history it
        // reads starts there; `replace` after onboarding then leaves Home as
        // the only entry and back closes the app.
        // `t!` is not reactive: a new app language rebuilds the router under
        // a new key, on the screen the switch happened (SET-04).
        Ok(has_me) => rsx! {
            for locale in [ui_language.locale()] {
                HistoryProvider {
                    key: "{locale}",
                    history: move |_| {
                        let route = ui_language.restart_route().unwrap_or(start_route(has_me));
                        Rc::new(MemoryHistory::with_initial_path(route)) as Rc<dyn History>
                    },
                    Router::<Route> {}
                }
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
    fn members_route_marks_the_setup_step() {
        let setup = Route::GroupMembers {
            id: "g1".into(),
            setup: true,
        };
        assert_eq!(setup.to_string().parse::<Route>().ok(), Some(setup));
        assert_eq!(
            "/groups/g1/members".parse::<Route>().ok(),
            Some(Route::GroupMembers {
                id: "g1".into(),
                setup: false
            })
        );
    }

    #[test]
    fn settle_route_can_open_the_sheet() {
        let record = Route::GroupSettle {
            id: "g1".into(),
            record: true,
        };
        assert_eq!(record.to_string().parse::<Route>().ok(), Some(record));
        assert_eq!(
            "/groups/g1/settle".parse::<Route>().ok(),
            Some(Route::GroupSettle {
                id: "g1".into(),
                record: false
            })
        );
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
