use dioxus::prelude::*;
use invuso_core::domain::Currency;
use invuso_core::fx::Rate;

use crate::clock::local_now;
use crate::components::RateSheet;
use crate::services::converter::save_manual_rate;
use crate::state::{DataRevision, Toaster};
use crate::storage::Db;

/// Sheet to type in a rate, e.g. an exchange office's (FX-10). It is
/// archived as "manual" and the converter uses it for this pair until the
/// user goes back to the daily rate. `reference` is the daily rate
/// `from → to` for comparison. `on_saved` gets the new rate's id.
#[component]
pub(super) fn ManualRateSheet(
    from: Currency,
    to: Currency,
    reference: Option<Rate>,
    on_saved: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let mut toaster = use_context::<Toaster>();
    let mut error = use_signal(|| None::<String>);

    let save = move |rate: Rate| match save_manual_rate(
        &db,
        rate.base(),
        rate.quote(),
        rate.value(),
        &local_now().0,
    ) {
        Ok(id) => {
            revision.bump();
            toaster.show(t!("manual_rate.saved").to_string(), None);
            on_saved.call(id);
        }
        Err(e) => error.set(Some(format!("{} {e}", t!("manual_rate.save_error")))),
    };

    rsx! {
        RateSheet {
            from,
            to,
            reference,
            title: t!("manual_rate.title").to_string(),
            hint: t!("manual_rate.hint").to_string(),
            error: error(),
            on_submit: save,
            on_close,
        }
    }
}
