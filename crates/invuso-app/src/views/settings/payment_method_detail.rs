use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdArchive, LdBanknote, LdChevronRight, LdCircleAlert, LdPencil, LdWallet},
};
use invuso_core::domain::PaymentMethodId;

use super::payment_methods::{PaymentMethodFormSheet, subtitle};
use crate::Route;
use crate::components::{
    Button, ButtonVariant, CardSection, EmptyState, ErrorBanner, MoneyText, PaymentMethodIcon,
    TopBar,
};
use crate::format::{NumberFormat, format_money};
use crate::preferences::display_date;
use crate::services::summary::method_totals;
use crate::state::DataRevision;
use crate::storage::{Db, MethodPayment, MethodPaymentSource};

/// `/settings/payment-methods/:id`: what was paid with a method, summed
/// per currency, and every payment across groups, personal expenses and
/// withdrawals (PAY-05). Editing opens the method's form.
#[component]
pub fn PaymentMethodDetail(id: String) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let mut editing = use_signal(|| false);

    let method_id = use_memo(use_reactive!(|id| PaymentMethodId::new(id)));
    let payments_db = db.clone();
    let people_db = db.clone();
    let method = use_memo(move || {
        revision.track();
        db.payment_method(&method_id()).map_err(|e| e.to_string())
    });
    let payments = use_memo(move || {
        revision.track();
        payments_db
            .method_payments(&method_id())
            .map_err(|e| format!("{} {e}", t!("payment.load_error_title")))
    });
    let people = use_memo(move || {
        revision.track();
        people_db.people().unwrap_or_default()
    });

    rsx! {
        TopBar { title: t!("page.payment_method_detail").to_string(), show_back: true }
        match &*method.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("payment.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(None) => rsx! {
                EmptyState {
                    title: t!("method_detail.not_found_title").to_string(),
                    text: t!("method_detail.not_found_text").to_string(),
                    Icon { icon: LdWallet, class: "h-8 w-8" }
                }
            },
            Ok(Some(method)) => {
                let owner = method
                    .owner_person_id
                    .as_ref()
                    .and_then(|id| people.read().iter().find(|p| &p.id == id).map(|p| p.name.clone()));
                rsx! {
                    div { class: "mx-4 flex flex-col gap-4 pt-6 safe-area-x",
                        div { class: "flex flex-col items-center gap-3 text-center",
                            PaymentMethodIcon { icon: method.icon.clone(), color: method.color.clone() }
                            h2 { class: "text-2xl font-semibold break-words text-floral-white-50", "{method.name}" }
                            p { class: "text-sm text-floral-white-400", {subtitle(method, owner.as_deref())} }
                            if method.archived {
                                span { class: "flex min-h-8 items-center gap-1.5 rounded-full bg-jet-black-800 px-3 text-sm font-medium text-floral-white-300",
                                    Icon { icon: LdArchive, class: "h-4 w-4" }
                                    {t!("method_detail.archived").to_string()}
                                }
                            }
                        }
                        match &*payments.read() {
                            Err(message) => rsx! { ErrorBanner { error: Some(message.clone()) } },
                            Ok(payments) if payments.is_empty() => rsx! {
                                EmptyState {
                                    title: t!("method_detail.empty_title").to_string(),
                                    text: t!("method_detail.empty_text").to_string(),
                                    Icon { icon: LdWallet, class: "h-8 w-8" }
                                }
                            },
                            Ok(payments) => rsx! { Payments { payments: payments.clone() } },
                        }
                        Button {
                            variant: ButtonVariant::Secondary,
                            class: "w-full",
                            onclick: move |_| editing.set(true),
                            Icon { icon: LdPencil, class: "h-5 w-5" }
                            {t!("common.edit").to_string()}
                        }
                    }
                    if editing() {
                        PaymentMethodFormSheet {
                            method: Some(method.clone()),
                            people: people(),
                            on_saved: move |_| editing.set(false),
                            on_close: move |_| editing.set(false),
                        }
                    }
                }
            }
        }
    }
}

/// Totals per currency and the list of payments, latest first.
#[component]
fn Payments(payments: Vec<MethodPayment>) -> Element {
    let totals = method_totals(&payments);
    let count = payments.len();
    rsx! {
        CardSection { title: t!("method_detail.totals").to_string(),
            div { class: "flex flex-col gap-1 px-4 py-4",
                for total in totals {
                    MoneyText {
                        key: "{total.currency().code()}",
                        amount: total,
                        class: "text-2xl font-semibold text-floral-white-50",
                    }
                }
                span { class: "text-sm text-floral-white-400",
                    if count == 1 {
                        {t!("breakdown.payments_one").to_string()}
                    } else {
                        {t!("breakdown.payments_other", count = count).to_string()}
                    }
                }
            }
        }
        CardSection { title: t!("method_detail.payments").to_string(),
            for (index, payment) in payments.into_iter().enumerate() {
                PaymentRow { key: "{index}", payment }
            }
        }
    }
}

/// One payment: what for, when, where and by whom; an expense opens its
/// detail.
#[component]
fn PaymentRow(payment: MethodPayment) -> Element {
    let nav = use_navigator();
    let date = display_date(payment.occurred_at.get(0..10).unwrap_or_default());
    let row = "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0";
    match payment.source {
        MethodPaymentSource::Expense {
            id,
            title,
            group_name,
        } => {
            let place = group_name.unwrap_or_else(|| t!("method_detail.personal").to_string());
            rsx! {
                button {
                    class: "{row} active:bg-jet-black-800 transition-colors ease-apple",
                    r#type: "button",
                    onclick: move |_| {
                        nav.push(Route::ExpenseDetail { id: id.as_str().to_string() });
                    },
                    span { class: "flex min-w-0 flex-1 flex-col",
                        span { class: "truncate text-base text-floral-white-50", "{title}" }
                        span { class: "truncate text-sm text-floral-white-400", "{date} · {place} · {payment.person_name}" }
                    }
                    MoneyText { amount: payment.amount, class: "shrink-0 text-base font-semibold text-floral-white-100" }
                    Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
                }
            }
        }
        MethodPaymentSource::Withdrawal { cash } => {
            let withdrawn = (cash != payment.amount).then(|| {
                t!(
                    "method_detail.withdrawn",
                    amount = format_money(cash, NumberFormat::current())
                )
                .to_string()
            });
            rsx! {
                div { class: "{row}",
                    span { class: "flex min-w-0 flex-1 flex-col",
                        span { class: "flex items-center gap-1.5 truncate text-base text-floral-white-50",
                            Icon { icon: LdBanknote, class: "h-4 w-4 shrink-0 text-floral-white-400" }
                            {t!("method_detail.withdrawal").to_string()}
                        }
                        span { class: "truncate text-sm text-floral-white-400", "{date} · {payment.person_name}" }
                        if let Some(withdrawn) = withdrawn {
                            span { class: "truncate text-sm tabular-nums text-floral-white-400", "{withdrawn}" }
                        }
                    }
                    MoneyText { amount: payment.amount, class: "shrink-0 text-base font-semibold text-floral-white-100" }
                    // Lines up with the chevron of the expense rows.
                    span { class: "w-5 shrink-0" }
                }
            }
        }
    }
}
