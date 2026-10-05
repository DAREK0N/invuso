//! Totals, balances and debts of groups (GRP-02, GRP-10..14, SPL-03,
//! SPL-04, PER-03), computed from the stored expenses by
//! [`invuso_core::split::summarize`].

use std::collections::BTreeMap;

use invuso_core::domain::{Currency, Group, Money, PersonId};
use invuso_core::split::{GroupSummary, PersonTotals, summarize};

use crate::storage::{Db, StorageError};

/// Totals, per-person figures and simplified debts of the group in its
/// base currency.
pub fn group_summary(db: &Db, group: &Group) -> Result<GroupSummary, StorageError> {
    let members = db.group_members(&group.id)?;
    let expenses = db.group_expenses(&group.id)?;
    // Settlements join the balances once they can be recorded (AP-23).
    Ok(summarize(
        group.base_currency,
        members.into_iter().map(|member| member.person.id),
        &expenses,
        &[],
    )?)
}

/// A person's figures in one group, in that group's base currency.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonGroupBalance {
    pub group: Group,
    pub totals: PersonTotals,
}

impl PersonGroupBalance {
    pub fn balance(&self) -> Money {
        Money::new(self.totals.balance, self.group.base_currency)
    }
}

/// The person's figures in every group they belong to or still have a
/// share in, in the order of [`Db::groups`].
pub fn person_balances(
    db: &Db,
    person: &PersonId,
) -> Result<Vec<PersonGroupBalance>, StorageError> {
    let mut balances = Vec::new();
    for group in db.groups()? {
        let summary = group_summary(db, &group)?;
        if let Some(totals) = summary.people.get(person) {
            balances.push(PersonGroupBalance {
                group,
                totals: *totals,
            });
        }
    }
    Ok(balances)
}

/// Sum of the balances per currency; groups in different base currencies
/// cannot be added up into one amount.
pub fn total_by_currency(balances: &[PersonGroupBalance]) -> Vec<Money> {
    let mut totals: BTreeMap<&str, (Currency, i64)> = BTreeMap::new();
    for entry in balances {
        let currency = entry.group.base_currency;
        let total = totals.entry(currency.code()).or_insert((currency, 0));
        total.1 = total.1.saturating_add(entry.totals.balance);
    }
    totals
        .into_values()
        .map(|(currency, amount)| Money::new(amount, currency))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use invuso_core::split::SplitMode;

    use super::*;
    use crate::storage::{NewExpense, NewExpensePayment, NewGroup, NewPerson, Profile};

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn group(db: &Db, name: &str, base: &str) -> Group {
        db.create_group(NewGroup {
            name: name.into(),
            icon: "plane".into(),
            color: "cerulean".into(),
            base_currency: cur(base),
            start_date: None,
            end_date: None,
            target_language: None,
        })
        .unwrap()
    }

    fn person(db: &Db, name: &str) -> PersonId {
        db.create_person(NewPerson {
            name: name.into(),
            color: "thistle".into(),
            is_me: false,
            note: None,
        })
        .unwrap()
        .id
    }

    /// `total` in the group's own currency, paid by `payer`, equal split.
    fn add(db: &Db, group: &Group, payer: &PersonId, total: i64, between: &[&PersonId]) {
        let rate = db
            .latest_rate(group.base_currency, group.base_currency)
            .unwrap()
            .unwrap();
        db.create_expense(
            NewExpense {
                group_id: Some(group.id.clone()),
                title: "Essen".into(),
                category_id: None,
                occurred_at: "2026-10-04T12:00:00+02:00".into(),
                total: Money::new(total, group.base_currency),
                payments: vec![NewExpensePayment {
                    person_id: payer.clone(),
                    payment_method_id: None,
                    amount_minor: total,
                }],
                split: SplitMode::Equal(between.iter().map(|p| (*p).clone()).collect()),
                receipt_id: None,
                line_items: Vec::new(),
                source: invuso_core::domain::ExpenseSource::Manual,
            },
            &rate,
        )
        .unwrap();
    }

    fn setup() -> (Db, PersonId) {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: cur("EUR"),
            target_language: "de".into(),
        })
        .unwrap();
        let me = db.me().unwrap().unwrap().id;
        (db, me)
    }

    #[test]
    fn group_summary_reads_what_was_saved() {
        let (db, me) = setup();
        let anna = person(&db, "Anna");
        let ben = person(&db, "Ben");
        let trip = group(&db, "Japan", "EUR");
        for id in [&anna, &ben] {
            db.add_group_member(&trip.id, id).unwrap();
        }
        // 30.00 by me for all three, 10.01 by Anna for Anna and Ben.
        add(&db, &trip, &me, 3_000, &[&me, &anna, &ben]);
        add(&db, &trip, &anna, 1_001, &[&anna, &ben]);

        let summary = group_summary(&db, &trip).unwrap();
        assert_eq!(summary.total, Money::new(4_001, cur("EUR")));
        assert_eq!(summary.expense_count, 2);
        // Anna's 10.01 splits 5.01 / 5.00 by person order of the ids.
        let anna_share = if anna < ben { 501 } else { 500 };
        assert_eq!(summary.people[&me].balance, 2_000);
        assert_eq!(summary.people[&anna].balance, 1_001 - 1_000 - anna_share);
        assert_eq!(summary.people.values().map(|t| t.balance).sum::<i64>(), 0);
        assert_eq!(
            summary
                .transfers
                .iter()
                .map(|t| t.amount_minor)
                .sum::<i64>(),
            summary
                .people
                .values()
                .filter(|t| t.balance > 0)
                .map(|t| t.balance)
                .sum::<i64>()
        );
    }

    #[test]
    fn deleted_expenses_and_other_groups_do_not_count() {
        let (db, me) = setup();
        let anna = person(&db, "Anna");
        let trip = group(&db, "Japan", "EUR");
        let flat = group(&db, "WG", "EUR");
        for g in [&trip, &flat] {
            db.add_group_member(&g.id, &anna).unwrap();
        }
        add(&db, &trip, &me, 2_000, &[&me, &anna]);
        add(&db, &flat, &anna, 5_000, &[&me, &anna]);
        let gone = db.group_expenses(&trip.id).unwrap()[0].id.clone();
        add(&db, &trip, &anna, 800, &[&me, &anna]);
        db.delete_expense(&gone).unwrap();

        let summary = group_summary(&db, &trip).unwrap();
        assert_eq!(summary.total, Money::new(800, cur("EUR")));
        assert_eq!(summary.people[&me].balance, -400);
    }

    #[test]
    fn changed_base_currency_leaves_old_expenses_out() {
        let (db, me) = setup();
        let mut trip = group(&db, "Japan", "EUR");
        add(&db, &trip, &me, 2_000, &[&me]);
        trip.base_currency = cur("JPY");
        db.update_group(&trip).unwrap();
        add(&db, &trip, &me, 500, &[&me]);

        let summary = group_summary(&db, &trip).unwrap();
        assert_eq!(summary.total, Money::new(500, cur("JPY")));
        assert_eq!(summary.expense_count, 1);
        assert_eq!(summary.skipped_count, 1);
    }

    #[test]
    fn person_balances_cover_their_groups_and_sum_per_currency() {
        let (db, me) = setup();
        let anna = person(&db, "Anna");
        let trip = group(&db, "Japan", "JPY");
        let flat = group(&db, "WG", "EUR");
        let tour = group(&db, "Tour", "EUR");
        let _without_anna = group(&db, "Allein", "EUR");
        for g in [&trip, &flat, &tour] {
            db.add_group_member(&g.id, &anna).unwrap();
        }
        add(&db, &trip, &me, 3_000, &[&me, &anna]);
        add(&db, &flat, &me, 1_000, &[&me, &anna]);
        add(&db, &tour, &anna, 600, &[&me, &anna]);

        let balances = person_balances(&db, &anna).unwrap();
        let names: BTreeSet<&str> = balances.iter().map(|b| b.group.name.as_str()).collect();
        assert_eq!(names, BTreeSet::from(["Japan", "WG", "Tour"]));
        assert_eq!(
            total_by_currency(&balances),
            [Money::new(-200, cur("EUR")), Money::new(-1_500, cur("JPY"))]
        );
    }
}
