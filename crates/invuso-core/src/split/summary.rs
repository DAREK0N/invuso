use std::collections::BTreeMap;

use super::{PersonTotals, SettlementEntry, SplitError, Transfer, balances, simplify_debts};
use crate::domain::{Currency, Expense, ExpenseError, Money, PersonId};

/// Everything the group overview shows (GRP-10..14, SPL-03, SPL-04), in the
/// group's base currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSummary {
    /// Sum of the counted expenses (GRP-10).
    pub total: Money,
    /// Expenses that went into the figures.
    pub expense_count: u32,
    /// Expenses left out because their base amount is in another currency,
    /// e.g. after the group's base currency was changed (user decision in
    /// AP-14: leave out and say so, never convert them anew).
    pub skipped_count: u32,
    /// Every member and everyone else who paid or shares a counted expense.
    pub people: BTreeMap<PersonId, PersonTotals>,
    /// Simplified "who pays whom" (SPL-04).
    pub transfers: Vec<Transfer>,
}

impl GroupSummary {
    /// "Who paid most" (GRP-11): highest amount paid first, ties by person.
    pub fn paid_ranking(&self) -> Vec<(PersonId, i64)> {
        let mut ranking: Vec<(PersonId, i64)> = self
            .people
            .iter()
            .map(|(person, totals)| (person.clone(), totals.paid))
            .collect();
        // Stable sort keeps person order on ties.
        ranking.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        ranking
    }

    /// "Who owes most" (GRP-12): everyone by balance, the largest debt
    /// first and the largest credit last, ties by person.
    pub fn balance_ranking(&self) -> Vec<(PersonId, i64)> {
        let mut ranking: Vec<(PersonId, i64)> = self
            .people
            .iter()
            .map(|(person, totals)| (person.clone(), totals.balance))
            .collect();
        ranking.sort_by_key(|entry| entry.1);
        ranking
    }
}

/// Builds the summary of one group (idee.md 8.3): each expense's payments
/// and shares in `base`, rescaled so they add up to its converted total
/// exactly (SPL-05), then balances and the simplified debts. `members`
/// appear even without any expense; `settlements` are already in `base`.
pub fn summarize(
    base: Currency,
    members: impl IntoIterator<Item = PersonId>,
    expenses: &[Expense],
    settlements: &[SettlementEntry],
) -> Result<GroupSummary, ExpenseError> {
    let mut total = Money::zero(base);
    let mut entries = Vec::new();
    let mut skipped_count = 0_u32;
    for expense in expenses {
        if expense.total_in_base.currency() != base {
            skipped_count += 1;
            continue;
        }
        total = total
            .checked_add(expense.total_in_base)
            .map_err(|_| SplitError::Overflow)?;
        entries.push(expense.entry()?);
    }

    let mut people = balances(&entries, settlements)?;
    for member in members {
        people.entry(member).or_default();
    }
    let open: BTreeMap<PersonId, i64> = people
        .iter()
        .map(|(person, totals)| (person.clone(), totals.balance))
        .collect();
    let transfers = simplify_debts(&open)?;

    Ok(GroupSummary {
        total,
        expense_count: u32::try_from(entries.len()).map_err(|_| SplitError::Overflow)?,
        skipped_count,
        people,
        transfers,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rust_decimal::Decimal;

    use super::*;
    use crate::domain::{ExpenseId, ExpensePayment, ExpenseSource};
    use crate::split::SplitMode;

    fn p(id: &str) -> PersonId {
        PersonId::new(id)
    }

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn expense(
        id: &str,
        total: Money,
        total_in_base: Money,
        payers: &[(&str, i64)],
        split: SplitMode,
    ) -> Expense {
        Expense {
            id: ExpenseId::new(id),
            group_id: None,
            title: id.to_string(),
            category_id: None,
            occurred_at: "2026-10-04T12:00:00+09:00".to_string(),
            total,
            fx_rate_id: None,
            total_in_base,
            split,
            source: ExpenseSource::Manual,
            payments: payers
                .iter()
                .map(|(person, amount)| ExpensePayment {
                    person_id: p(person),
                    payment_method_id: None,
                    amount: Money::new(*amount, total.currency()),
                })
                .collect(),
        }
    }

    fn eur(minor: i64) -> Money {
        Money::new(minor, cur("EUR"))
    }

    fn everyone() -> BTreeSet<PersonId> {
        [p("anna"), p("ben"), p("cleo")].into()
    }

    /// The example group, computed by hand:
    ///
    /// | Expense | Paid by | Split | Anna | Ben | Cleo |
    /// |---|---|---|---|---|---|
    /// | Hotel 90.00 € | Anna | equal | 30.00 | 30.00 | 30.00 |
    /// | Ramen 3,000 ¥ = 16.83 € | Ben | equal | 5.61 | 5.61 | 5.61 |
    /// | Taxi 10.00 € | Cleo | 2 : 1 (Anna, Cleo) | 6.67 | – | 3.33 |
    /// | Snack 5.00 $ (base USD) | Anna | – | left out | | |
    ///
    /// Ramen: 1,000 ¥ each, rescaled to 16.83 € → 5.61 € each. Taxi: 6.666…
    /// and 3.333…; the leftover cent goes to Anna's larger remainder.
    ///
    /// Total 116.83 €. Paid: Anna 90.00, Ben 16.83, Cleo 10.00, Dan 0.
    /// Consumed: Anna 42.28, Ben 35.61, Cleo 38.94. Balance: Anna +47.72,
    /// Ben −18.78, Cleo −28.94, Dan 0 – together 0. Cleo, the largest
    /// debtor, pays Anna 28.94, then Ben pays Anna 18.78.
    fn example() -> GroupSummary {
        let expenses = [
            expense(
                "hotel",
                eur(9_000),
                eur(9_000),
                &[("anna", 9_000)],
                SplitMode::Equal(everyone()),
            ),
            expense(
                "ramen",
                Money::new(3_000, cur("JPY")),
                eur(1_683),
                &[("ben", 3_000)],
                SplitMode::Equal(everyone()),
            ),
            expense(
                "taxi",
                eur(1_000),
                eur(1_000),
                &[("cleo", 1_000)],
                SplitMode::Weights([(p("anna"), Decimal::TWO), (p("cleo"), Decimal::ONE)].into()),
            ),
            expense(
                "snack",
                Money::new(500, cur("USD")),
                Money::new(500, cur("USD")),
                &[("anna", 500)],
                SplitMode::Equal(everyone()),
            ),
        ];
        let members = [p("anna"), p("ben"), p("cleo"), p("dan")];
        summarize(cur("EUR"), members, &expenses, &[]).unwrap()
    }

    fn totals(paid: i64, consumed: i64, balance: i64) -> PersonTotals {
        PersonTotals {
            paid,
            consumed,
            settled_out: 0,
            settled_in: 0,
            balance,
        }
    }

    #[test]
    fn example_group_matches_the_hand_calculation() {
        let summary = example();
        assert_eq!(summary.total, eur(11_683));
        assert_eq!(summary.expense_count, 3);
        assert_eq!(summary.skipped_count, 1);
        assert_eq!(
            summary.people,
            [
                (p("anna"), totals(9_000, 4_228, 4_772)),
                (p("ben"), totals(1_683, 3_561, -1_878)),
                (p("cleo"), totals(1_000, 3_894, -2_894)),
                (p("dan"), totals(0, 0, 0)),
            ]
            .into()
        );
        assert_eq!(
            summary.transfers,
            [
                Transfer {
                    from: p("cleo"),
                    to: p("anna"),
                    amount_minor: 2_894
                },
                Transfer {
                    from: p("ben"),
                    to: p("anna"),
                    amount_minor: 1_878
                },
            ]
        );
    }

    #[test]
    fn balances_add_up_to_zero_and_consumption_to_the_total() {
        let summary = example();
        let people = summary.people.values();
        assert_eq!(people.clone().map(|t| t.balance).sum::<i64>(), 0);
        assert_eq!(
            people.clone().map(|t| t.consumed).sum::<i64>(),
            summary.total.amount_minor()
        );
        assert_eq!(
            people.map(|t| t.paid).sum::<i64>(),
            summary.total.amount_minor()
        );
    }

    #[test]
    fn rankings_order_by_paid_and_by_balance() {
        let summary = example();
        assert_eq!(
            summary.paid_ranking(),
            [
                (p("anna"), 9_000),
                (p("ben"), 1_683),
                (p("cleo"), 1_000),
                (p("dan"), 0)
            ]
        );
        assert_eq!(
            summary.balance_ranking(),
            [
                (p("cleo"), -2_894),
                (p("ben"), -1_878),
                (p("dan"), 0),
                (p("anna"), 4_772)
            ]
        );
    }

    #[test]
    fn ranking_ties_keep_person_order() {
        let expenses = [expense(
            "dinner",
            eur(2_000),
            eur(2_000),
            &[("ben", 1_000), ("anna", 1_000)],
            SplitMode::Equal([p("anna"), p("ben")].into()),
        )];
        let summary = summarize(cur("EUR"), [], &expenses, &[]).unwrap();
        assert_eq!(
            summary.paid_ranking(),
            [(p("anna"), 1_000), (p("ben"), 1_000)]
        );
        assert_eq!(summary.transfers, []);
    }

    #[test]
    fn empty_group_has_members_with_zeros() {
        let summary = summarize(cur("JPY"), [p("anna")], &[], &[]).unwrap();
        assert_eq!(summary.total, Money::zero(cur("JPY")));
        assert_eq!(summary.expense_count, 0);
        assert_eq!(
            summary.people,
            [(p("anna"), PersonTotals::default())].into()
        );
        assert_eq!(summary.transfers, []);
    }

    #[test]
    fn settlements_count_towards_the_balances() {
        let expenses = [expense(
            "hotel",
            eur(6_000),
            eur(6_000),
            &[("anna", 6_000)],
            SplitMode::Equal([p("anna"), p("ben")].into()),
        )];
        let settlements = [SettlementEntry {
            from: p("ben"),
            to: p("anna"),
            amount_minor: 1_000,
        }];
        let summary = summarize(cur("EUR"), [], &expenses, &settlements).unwrap();
        assert_eq!(summary.people[&p("ben")].balance, -2_000);
        assert_eq!(
            summary.transfers,
            [Transfer {
                from: p("ben"),
                to: p("anna"),
                amount_minor: 2_000
            }]
        );
    }

    #[test]
    fn three_decimal_base_currency_stays_exact() {
        // 10.00 € = 4.700 BHD split three ways: 3.34 + 3.33 + 3.33 € in the
        // expense's currency, rescaled to 1.5698 → 1.570 and 1.5651 → 1.565.
        let expenses = [expense(
            "tea",
            eur(1_000),
            Money::new(4_700, cur("BHD")),
            &[("anna", 1_000)],
            SplitMode::Equal(everyone()),
        )];
        let summary = summarize(cur("BHD"), [], &expenses, &[]).unwrap();
        let consumed: Vec<i64> = summary.people.values().map(|t| t.consumed).collect();
        assert_eq!(consumed, [1_570, 1_565, 1_565]);
        assert_eq!(summary.people[&p("anna")].balance, 3_130);
    }

    #[test]
    fn broken_expense_is_an_error_not_a_wrong_figure() {
        let broken = expense(
            "nobody",
            eur(1_000),
            eur(1_000),
            &[],
            SplitMode::Equal(everyone()),
        );
        assert!(summarize(cur("EUR"), [], &[broken], &[]).is_err());
    }
}
