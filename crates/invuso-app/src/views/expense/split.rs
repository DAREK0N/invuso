//! The "split between" part of the expense form (EXP-04, SPL-01): the
//! inputs of each mode and how they turn into a [`SplitMode`].

use std::collections::{BTreeMap, BTreeSet};

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdSquare, LdSquareCheck},
};
use invuso_core::Decimal;
use invuso_core::domain::{Currency, ExpenseError, Money, Person, PersonId};
use invuso_core::split::{SplitError, SplitMode};

use crate::components::{Avatar, AvatarSize, CompactAmountInput, CompactNumberInput};
use crate::format::{
    NumberFormat, amount_text, fit_amount_text, format_money, format_number, number_text,
    parse_amount, parse_number,
};

/// Decimals a weight or a percentage can be typed with.
const SHARE_DECIMALS: u32 = 2;

/// The split mode the form edits (idee.md 8.1 without `Items`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SplitKind {
    #[default]
    Equal,
    Weights,
    Percent,
    Exact,
}

impl SplitKind {
    pub(super) const ALL: [Self; 4] = [Self::Equal, Self::Weights, Self::Percent, Self::Exact];

    pub(super) fn label(self) -> String {
        match self {
            Self::Equal => t!("expense.split_mode_equal"),
            Self::Weights => t!("expense.split_mode_weights"),
            Self::Percent => t!("expense.split_mode_percent"),
            Self::Exact => t!("expense.split_mode_exact"),
        }
        .to_string()
    }
}

/// What the split section holds while the form is open. Each mode keeps
/// its own texts (canonical, see `format`), so switching back and forth
/// loses nothing.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct SplitDraft {
    pub kind: SplitKind,
    pub participants: BTreeSet<PersonId>,
    weights: BTreeMap<PersonId, String>,
    percents: BTreeMap<PersonId, String>,
    amounts: BTreeMap<PersonId, String>,
}

impl SplitDraft {
    /// Everyone listed, equally (idee.md 7.3).
    pub(super) fn equal(participants: BTreeSet<PersonId>) -> Self {
        Self {
            participants,
            ..Self::default()
        }
    }

    /// The inputs of a saved split, to edit it (EXP-05).
    pub(super) fn from_mode(mode: &SplitMode, currency: Currency, format: NumberFormat) -> Self {
        let texts = |map: &BTreeMap<PersonId, Decimal>| {
            map.iter()
                .map(|(person, value)| (person.clone(), number_text(*value, format)))
                .collect()
        };
        let mut draft = Self::equal(mode.participants());
        match mode {
            SplitMode::Equal(_) => {}
            SplitMode::Weights(weights) => {
                draft.kind = SplitKind::Weights;
                draft.weights = texts(weights);
            }
            SplitMode::Percent(percents) => {
                draft.kind = SplitKind::Percent;
                draft.percents = texts(percents);
            }
            SplitMode::Exact(amounts) => {
                draft.kind = SplitKind::Exact;
                draft.amounts = amounts
                    .iter()
                    .map(|(person, minor)| {
                        let text = amount_text(Money::new(*minor, currency), format);
                        (person.clone(), text)
                    })
                    .collect();
            }
        }
        draft
    }

    /// Selects or deselects a person.
    pub(super) fn toggle(&mut self, person: PersonId) {
        if !self.participants.remove(&person) {
            self.participants.insert(person);
        }
    }

    /// The text a person's field shows in the current mode. An untouched
    /// weight shows the member's default weight (PER-04).
    pub(super) fn text(
        &self,
        person: &PersonId,
        default_weight: Decimal,
        format: NumberFormat,
    ) -> String {
        match self.kind {
            SplitKind::Equal => String::new(),
            SplitKind::Weights => self
                .weights
                .get(person)
                .cloned()
                .unwrap_or_else(|| number_text(default_weight, format)),
            SplitKind::Percent => self.percents.get(person).cloned().unwrap_or_default(),
            SplitKind::Exact => self.amounts.get(person).cloned().unwrap_or_default(),
        }
    }

    pub(super) fn set_text(&mut self, person: PersonId, text: String) {
        let texts = match self.kind {
            SplitKind::Equal => return,
            SplitKind::Weights => &mut self.weights,
            SplitKind::Percent => &mut self.percents,
            SplitKind::Exact => &mut self.amounts,
        };
        texts.insert(person, text);
    }

    /// Shortens the exact amounts to what `currency` allows.
    pub(super) fn fit_currency(&mut self, currency: Currency, format: NumberFormat) {
        for text in self.amounts.values_mut() {
            *text = fit_amount_text(text, currency, format);
        }
    }

    /// The split the inputs describe; empty fields count as 0. Whether it
    /// is valid decides `validate_split`.
    pub(super) fn mode(
        &self,
        default_weights: &BTreeMap<PersonId, Decimal>,
        currency: Currency,
        format: NumberFormat,
    ) -> SplitMode {
        let number = |texts: &BTreeMap<PersonId, String>, person: &PersonId, fallback: Decimal| {
            texts.get(person).map_or(fallback, |text| {
                parse_number(text, format).unwrap_or(Decimal::ZERO)
            })
        };
        let people = self.participants.iter().cloned();
        match self.kind {
            SplitKind::Equal => SplitMode::Equal(self.participants.clone()),
            SplitKind::Weights => SplitMode::Weights(
                people
                    .map(|person| {
                        let fallback = default_weights
                            .get(&person)
                            .copied()
                            .unwrap_or(Decimal::ONE);
                        let weight = number(&self.weights, &person, fallback);
                        (person, weight)
                    })
                    .collect(),
            ),
            SplitKind::Percent => SplitMode::Percent(
                people
                    .map(|person| {
                        let percent = number(&self.percents, &person, Decimal::ZERO);
                        (person, percent)
                    })
                    .collect(),
            ),
            SplitKind::Exact => SplitMode::Exact(
                people
                    .map(|person| {
                        let minor = self
                            .amounts
                            .get(&person)
                            .and_then(|text| parse_amount(text, currency, format))
                            .map_or(0, |m| m.amount_minor());
                        (person, minor)
                    })
                    .collect(),
            ),
        }
    }
}

/// Running total of the inputs for modes that must add up (idee.md 8.1),
/// and whether it does.
pub(super) fn sum_hint(
    mode: &SplitMode,
    total: Money,
    format: NumberFormat,
) -> Option<(String, bool)> {
    match mode {
        SplitMode::Percent(percents) => {
            let sum: Decimal = percents.values().sum();
            let text = t!("expense.percent_sum", sum = format_number(sum, format)).to_string();
            Some((text, sum == Decimal::ONE_HUNDRED))
        }
        SplitMode::Exact(amounts) => {
            let sum = amounts
                .values()
                .fold(0_i64, |acc, v| acc.saturating_add(*v));
            let text = t!(
                "expense.exact_sum",
                sum = format_money(Money::new(sum, total.currency()), format),
                total = format_money(total, format)
            )
            .to_string();
            Some((text, sum == total.amount_minor()))
        }
        SplitMode::Equal(_) | SplitMode::Weights(_) => None,
    }
}

/// The errors of `validate_split` in the app language.
pub(super) fn split_error_text(
    error: &ExpenseError,
    currency: Currency,
    format: NumberFormat,
) -> String {
    match error {
        ExpenseError::NoParticipants | ExpenseError::Split(SplitError::NoParticipants) => {
            t!("expense.participants_required").to_string()
        }
        ExpenseError::Split(SplitError::PercentNot100(sum)) => t!(
            "expense.percent_mismatch",
            sum = format_number(*sum, format)
        )
        .to_string(),
        ExpenseError::Split(SplitError::ExactSumMismatch { expected, actual }) => t!(
            "expense.exact_mismatch",
            sum = format_money(Money::new(*actual, currency), format),
            total = format_money(Money::new(*expected, currency), format)
        )
        .to_string(),
        ExpenseError::Split(SplitError::ZeroTotalWeight) => t!("expense.weights_zero").to_string(),
        other => other.to_string(),
    }
}

/// One person of a weighted, percentage or exact split: tap to take part,
/// type the value, see the resulting share underneath the name.
#[component]
pub(super) fn ShareRow(
    person: Person,
    selected: bool,
    kind: SplitKind,
    text: String,
    currency: Currency,
    share: Option<Money>,
    invalid: bool,
    on_toggle: EventHandler<()>,
    on_input: EventHandler<String>,
) -> Element {
    let format = NumberFormat::current();
    let name = person.name.clone();
    let id = format!("share-{}", person.id.as_str());
    let name_color = if selected {
        "text-floral-white-50"
    } else {
        "text-floral-white-300"
    };

    rsx! {
        div { class: "flex min-h-14 items-center gap-2 border-b border-jet-black-800 px-2 py-1 last:border-b-0",
            button {
                class: "flex min-h-12 min-w-0 flex-1 items-center gap-3 rounded-2xl px-1 text-left active:bg-jet-black-800 transition-colors ease-apple",
                r#type: "button",
                role: "checkbox",
                aria_checked: if selected { "true" } else { "false" },
                onclick: move |_| on_toggle.call(()),
                if selected {
                    Icon { icon: LdSquareCheck, class: "h-6 w-6 shrink-0 text-cerulean-300" }
                } else {
                    Icon { icon: LdSquare, class: "h-6 w-6 shrink-0 text-floral-white-500" }
                }
                Avatar { name: name.clone(), color: person.color.clone(), size: AvatarSize::Sm }
                span { class: "flex min-w-0 flex-1 flex-col",
                    span { class: "truncate text-base {name_color}", "{name}" }
                    if let Some(share) = share.filter(|_| selected) {
                        span { class: "text-sm tabular-nums text-floral-white-400", aria_live: "polite",
                            {format_money(share, format)}
                        }
                    }
                }
            }
            if selected {
                match kind {
                    SplitKind::Weights => rsx! {
                        CompactNumberInput {
                            id,
                            label: t!("expense.weight_label", name = name).to_string(),
                            value: text,
                            decimals: SHARE_DECIMALS,
                            unit: String::new(),
                            invalid,
                            oninput: on_input,
                        }
                    },
                    SplitKind::Percent => rsx! {
                        CompactNumberInput {
                            id,
                            label: t!("expense.percent_label", name = name).to_string(),
                            value: text,
                            decimals: SHARE_DECIMALS,
                            unit: "%",
                            invalid,
                            oninput: on_input,
                        }
                    },
                    SplitKind::Exact => rsx! {
                        CompactAmountInput {
                            id,
                            label: t!("expense.exact_label", name = name).to_string(),
                            value: text,
                            currency,
                            invalid,
                            oninput: on_input,
                        }
                    },
                    SplitKind::Equal => rsx! {},
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    const DE: NumberFormat = NumberFormat {
        decimal: ',',
        group: '.',
        symbol_before: false,
    };

    fn p(id: &str) -> PersonId {
        PersonId::new(id)
    }

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn eur() -> Currency {
        Currency::from_code("EUR").unwrap()
    }

    fn draft(kind: SplitKind) -> SplitDraft {
        SplitDraft {
            kind,
            ..SplitDraft::equal([p("a"), p("b"), p("c")].into())
        }
    }

    #[test]
    fn untouched_weights_use_the_default_weight() {
        let defaults = BTreeMap::from([(p("c"), d("0.5"))]);
        let mut weights = draft(SplitKind::Weights);
        weights.set_text(p("a"), "2".into());
        assert_eq!(weights.text(&p("c"), d("0.5"), DE), "0,5");
        assert_eq!(
            weights.mode(&defaults, eur(), DE),
            SplitMode::Weights(BTreeMap::from([
                (p("a"), d("2")),
                (p("b"), d("1")),
                (p("c"), d("0.5")),
            ]))
        );
    }

    #[test]
    fn empty_fields_count_as_zero_and_deselected_people_drop_out() {
        let mut percent = draft(SplitKind::Percent);
        percent.set_text(p("a"), "70".into());
        percent.set_text(p("b"), "30".into());
        percent.toggle(p("c"));
        assert_eq!(
            percent.mode(&BTreeMap::new(), eur(), DE),
            SplitMode::Percent(BTreeMap::from([(p("a"), d("70")), (p("b"), d("30"))]))
        );

        let mut exact = draft(SplitKind::Exact);
        exact.set_text(p("a"), "12,5".into());
        exact.set_text(p("b"), String::new());
        assert_eq!(
            exact.mode(&BTreeMap::new(), eur(), DE),
            SplitMode::Exact(BTreeMap::from([(p("a"), 1_250), (p("b"), 0), (p("c"), 0)]))
        );
    }

    #[test]
    fn saved_split_round_trips_through_the_draft() {
        let jpy = Currency::from_code("JPY").unwrap();
        let modes = [
            SplitMode::Equal([p("a"), p("b")].into()),
            SplitMode::Weights(BTreeMap::from([(p("a"), d("2")), (p("b"), d("0.5"))])),
            SplitMode::Percent(BTreeMap::from([(p("a"), d("33.33")), (p("b"), d("66.67"))])),
            SplitMode::Exact(BTreeMap::from([(p("a"), 1_200), (p("b"), 1_800)])),
        ];
        for mode in modes {
            let draft = SplitDraft::from_mode(&mode, jpy, DE);
            assert_eq!(draft.mode(&BTreeMap::new(), jpy, DE), mode);
        }
    }

    #[test]
    fn switching_modes_keeps_the_texts() {
        let mut draft = draft(SplitKind::Percent);
        draft.set_text(p("a"), "60".into());
        draft.kind = SplitKind::Exact;
        draft.set_text(p("a"), "10".into());
        draft.kind = SplitKind::Percent;
        assert_eq!(draft.text(&p("a"), Decimal::ONE, DE), "60");
    }

    #[test]
    fn exact_amounts_follow_the_currency() {
        let mut draft = draft(SplitKind::Exact);
        draft.set_text(p("a"), "12,50".into());
        draft.fit_currency(Currency::from_code("JPY").unwrap(), DE);
        assert_eq!(draft.text(&p("a"), Decimal::ONE, DE), "12");
    }
}
