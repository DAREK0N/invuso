//! The settlement as plain text for messengers (SPL-09).

use std::collections::BTreeMap;

use invuso_core::domain::{Money, Person, PersonId};
use invuso_core::split::Transfer;

use crate::format::{NumberFormat, format_money};

/// "Japan Reise – Abrechnung", the total and one line per payment, e.g.
/// "Anna → Ben: 23,40 €"; `debts` are in `total`'s currency, simplified or
/// pairwise as currently shown.
pub fn settlement_text(
    group_name: &str,
    total: Money,
    debts: &[Transfer],
    people: &BTreeMap<PersonId, Person>,
    format: NumberFormat,
) -> String {
    let name = |id: &PersonId| {
        people.get(id).map_or_else(
            || t!("expense_detail.unknown_person").to_string(),
            |person| person.name.clone(),
        )
    };
    let mut lines = vec![
        t!("settle.share_heading", group = group_name).to_string(),
        t!("settle.share_total", amount = format_money(total, format)).to_string(),
        String::new(),
    ];
    if debts.is_empty() {
        lines.push(t!("summary.all_settled").to_string());
    }
    for debt in debts {
        lines.push(
            t!(
                "settle.share_line",
                from = name(&debt.from),
                to = name(&debt.to),
                amount = format_money(Money::new(debt.amount_minor, total.currency()), format)
            )
            .to_string(),
        );
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::Currency;

    use super::*;

    fn person(id: &str, name: &str) -> (PersonId, Person) {
        (
            PersonId::new(id),
            Person {
                id: PersonId::new(id),
                name: name.into(),
                color: "thistle".into(),
                avatar_path: None,
                is_me: false,
                note: None,
            },
        )
    }

    fn german() -> NumberFormat {
        rust_i18n::set_locale("de");
        NumberFormat::current()
    }

    #[test]
    fn lists_total_and_payments() {
        let format = german();
        let eur = Currency::from_code("EUR").unwrap();
        let people = [person("a", "Anna"), person("b", "Ben")].into();
        let debts = [Transfer {
            from: PersonId::new("b"),
            to: PersonId::new("a"),
            amount_minor: 2_340,
        }];
        let text = settlement_text(
            "Japan Reise",
            Money::new(123_456, eur),
            &debts,
            &people,
            format,
        );
        assert_eq!(
            text,
            "Japan Reise – Abrechnung\nGesamt: 1.234,56\u{a0}€\n\nBen → Anna: 23,40\u{a0}€"
        );
    }

    #[test]
    fn says_when_everything_is_settled() {
        let format = german();
        let jpy = Currency::from_code("JPY").unwrap();
        let text = settlement_text("WG", Money::new(3_000, jpy), &[], &BTreeMap::new(), format);
        assert_eq!(
            text,
            "WG – Abrechnung\nGesamt: 3.000\u{a0}¥\n\nAlles ausgeglichen."
        );
    }
}
