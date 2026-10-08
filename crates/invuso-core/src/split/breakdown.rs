use std::collections::BTreeMap;

use super::SplitError;
use crate::domain::{
    CategoryId, Currency, Expense, ExpenseError, Money, PaymentMethodId, PersonId,
};

/// One part of a [`Breakdown`], e.g. what a category cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice<K> {
    pub key: K,
    /// In minor units of the breakdown's currency.
    pub amount_minor: i64,
    /// Expenses (or payments) that went into it.
    pub count: u32,
}

/// A group's spending split into parts that add up to exactly `total`
/// (GRP-16, GRP-17), largest part first, ties by key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakdown<K> {
    pub total: Money,
    pub slices: Vec<Slice<K>>,
}

impl<K: Ord + Clone> Breakdown<K> {
    /// Merges the parts by a coarser key, e.g. payment methods by their
    /// kind; the total stays the same.
    pub fn regroup<K2: Ord + Clone>(
        &self,
        coarser: impl Fn(&K) -> K2,
    ) -> Result<Breakdown<K2>, SplitError> {
        let mut parts: BTreeMap<K2, (i64, u32)> = BTreeMap::new();
        for slice in &self.slices {
            add(
                &mut parts,
                coarser(&slice.key),
                slice.amount_minor,
                slice.count,
            )?;
        }
        Ok(sorted(self.total, parts))
    }
}

/// Who paid with which method: the payer and the method, `None` if none
/// was chosen.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PaymentKey {
    pub person: PersonId,
    pub method: Option<PaymentMethodId>,
}

/// Spending per category in `base` (GRP-16), `None` for expenses without
/// a category. Like [`summarize`](super::summarize), expenses whose base
/// amount is in another currency are left out.
pub fn by_category(
    base: Currency,
    expenses: &[Expense],
) -> Result<Breakdown<Option<CategoryId>>, ExpenseError> {
    let mut parts: BTreeMap<Option<CategoryId>, (i64, u32)> = BTreeMap::new();
    let mut total = Money::zero(base);
    for expense in counted(base, expenses) {
        total = total
            .checked_add(expense.total_in_base)
            .map_err(|_| SplitError::Overflow)?;
        add(
            &mut parts,
            expense.category_id.clone(),
            expense.total_in_base.amount_minor(),
            1,
        )?;
    }
    Ok(sorted(total, parts))
}

/// What each person paid with each method, in `base` (GRP-17). Each
/// payment is converted like the balances convert it
/// ([`Expense::payments_in_base`]), so the parts add up to the same total
/// as [`by_category`] and the overview.
pub fn by_payment(
    base: Currency,
    expenses: &[Expense],
) -> Result<Breakdown<PaymentKey>, ExpenseError> {
    let mut parts: BTreeMap<PaymentKey, (i64, u32)> = BTreeMap::new();
    let mut total = Money::zero(base);
    for expense in counted(base, expenses) {
        total = total
            .checked_add(expense.total_in_base)
            .map_err(|_| SplitError::Overflow)?;
        let in_base = expense.payments_in_base()?;
        for payment in &expense.payments {
            // A person pays an expense once (`validate_payments`), so the
            // person's converted amount is this payment's.
            let amount = in_base.get(&payment.person_id).copied().unwrap_or(0);
            let key = PaymentKey {
                person: payment.person_id.clone(),
                method: payment.payment_method_id.clone(),
            };
            add(&mut parts, key, amount, 1)?;
        }
    }
    Ok(sorted(total, parts))
}

fn counted(base: Currency, expenses: &[Expense]) -> impl Iterator<Item = &Expense> {
    expenses
        .iter()
        .filter(move |expense| expense.total_in_base.currency() == base)
}

fn add<K: Ord>(
    parts: &mut BTreeMap<K, (i64, u32)>,
    key: K,
    amount: i64,
    count: u32,
) -> Result<(), SplitError> {
    let part = parts.entry(key).or_insert((0, 0));
    part.0 = part.0.checked_add(amount).ok_or(SplitError::Overflow)?;
    part.1 = part.1.saturating_add(count);
    Ok(())
}

fn sorted<K: Ord>(total: Money, parts: BTreeMap<K, (i64, u32)>) -> Breakdown<K> {
    let mut slices: Vec<Slice<K>> = parts
        .into_iter()
        .map(|(key, (amount_minor, count))| Slice {
            key,
            amount_minor,
            count,
        })
        .collect();
    // Stable sort keeps key order on ties.
    slices.sort_by_key(|slice| std::cmp::Reverse(slice.amount_minor));
    Breakdown { total, slices }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::{ExpenseId, ExpensePayment, ExpenseSource};
    use crate::split::SplitMode;

    fn p(id: &str) -> PersonId {
        PersonId::new(id)
    }

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn cat(id: &str) -> Option<CategoryId> {
        Some(CategoryId::new(id))
    }

    fn method(id: &str) -> Option<PaymentMethodId> {
        Some(PaymentMethodId::new(id))
    }

    fn expense(
        category: Option<CategoryId>,
        total: Money,
        total_in_base: Money,
        payers: &[(&str, Option<PaymentMethodId>, i64)],
    ) -> Expense {
        let everyone: BTreeSet<PersonId> = [p("anna"), p("ben")].into();
        Expense {
            id: ExpenseId::new("e"),
            group_id: None,
            title: "e".into(),
            category_id: category,
            occurred_at: "2026-10-04T12:00:00+09:00".into(),
            total,
            fx_rate_id: None,
            total_in_base,
            split: SplitMode::Equal(everyone),
            source: ExpenseSource::Manual,
            receipt_id: None,
            note: None,
            location: None,
            coordinates: None,
            payments: payers
                .iter()
                .map(|(person, method, amount)| ExpensePayment {
                    person_id: p(person),
                    payment_method_id: method.clone(),
                    amount: Money::new(*amount, total.currency()),
                })
                .collect(),
            line_items: Vec::new(),
        }
    }

    fn eur(minor: i64) -> Money {
        Money::new(minor, cur("EUR"))
    }

    fn jpy(minor: i64) -> Money {
        Money::new(minor, cur("JPY"))
    }

    /// The example trip, computed by hand:
    ///
    /// | Expense | Category | Base | Paid with |
    /// |---|---|---|---|
    /// | Hotel 90.00 € | lodging | 90.00 | Anna Visa 90.00 |
    /// | Ramen 3,000 ¥ | food | 16.83 | Ben cash 1,000 ¥, Anna Visa 2,000 ¥ |
    /// | Sushi 2,000 ¥ | food | 11.22 | Ben cash 2,000 ¥ |
    /// | Taxi 10.00 € | – | 10.00 | Ben, no method, 10.00 |
    /// | Snack 5.00 $ (base USD) | food | left out | |
    ///
    /// Ramen 16.83 € by 1 : 2 is 5.61 and 11.22. Total 128.05 €.
    /// Categories: lodging 90.00, food 28.05, none 10.00.
    /// Methods: Anna Visa 101.22, Ben cash 16.83, Ben none 10.00.
    fn trip() -> Vec<Expense> {
        vec![
            expense(
                cat("lodging"),
                eur(9_000),
                eur(9_000),
                &[("anna", method("visa"), 9_000)],
            ),
            expense(
                cat("food"),
                jpy(3_000),
                eur(1_683),
                &[
                    ("ben", method("cash"), 1_000),
                    ("anna", method("visa"), 2_000),
                ],
            ),
            expense(
                cat("food"),
                jpy(2_000),
                eur(1_122),
                &[("ben", method("cash"), 2_000)],
            ),
            expense(None, eur(1_000), eur(1_000), &[("ben", None, 1_000)]),
            expense(
                cat("food"),
                Money::new(500, cur("USD")),
                Money::new(500, cur("USD")),
                &[("anna", method("visa"), 500)],
            ),
        ]
    }

    fn slice<K>(key: K, amount_minor: i64, count: u32) -> Slice<K> {
        Slice {
            key,
            amount_minor,
            count,
        }
    }

    fn sum<K>(breakdown: &Breakdown<K>) -> i64 {
        breakdown.slices.iter().map(|s| s.amount_minor).sum()
    }

    #[test]
    fn categories_match_the_hand_calculation() {
        let breakdown = by_category(cur("EUR"), &trip()).unwrap();
        assert_eq!(breakdown.total, eur(12_805));
        assert_eq!(
            breakdown.slices,
            [
                slice(cat("lodging"), 9_000, 1),
                slice(cat("food"), 2_805, 2),
                slice(None, 1_000, 1),
            ]
        );
        assert_eq!(sum(&breakdown), breakdown.total.amount_minor());
    }

    #[test]
    fn payments_match_the_hand_calculation() {
        let breakdown = by_payment(cur("EUR"), &trip()).unwrap();
        let key = |person: &str, method: Option<PaymentMethodId>| PaymentKey {
            person: p(person),
            method,
        };
        assert_eq!(breakdown.total, eur(12_805));
        assert_eq!(
            breakdown.slices,
            [
                slice(key("anna", method("visa")), 10_122, 2),
                slice(key("ben", method("cash")), 1_683, 2),
                slice(key("ben", None), 1_000, 1),
            ]
        );
        assert_eq!(sum(&breakdown), breakdown.total.amount_minor());
    }

    #[test]
    fn regrouping_keeps_the_total() {
        let breakdown = by_payment(cur("EUR"), &trip()).unwrap();
        // By payer only.
        let by_person = breakdown.regroup(|key| key.person.clone()).unwrap();
        assert_eq!(by_person.total, breakdown.total);
        assert_eq!(
            by_person.slices,
            [slice(p("anna"), 10_122, 2), slice(p("ben"), 2_683, 3)]
        );
        assert_eq!(sum(&by_person), by_person.total.amount_minor());
    }

    #[test]
    fn converted_parts_add_up_without_losing_a_cent() {
        // 999 ¥ = 6.01 € paid by three people in thirds: 2.00 / 2.00 /
        // 2.01 by largest remainder, never 2.00 × 3 = 6.00.
        let shared = expense(
            cat("food"),
            jpy(999),
            eur(601),
            &[
                ("anna", method("visa"), 333),
                ("ben", method("cash"), 333),
                ("cleo", None, 333),
            ],
        );
        let breakdown = by_payment(cur("EUR"), &[shared]).unwrap();
        assert_eq!(sum(&breakdown), 601);
        let mut amounts: Vec<i64> = breakdown.slices.iter().map(|s| s.amount_minor).collect();
        amounts.sort_unstable();
        assert_eq!(amounts, [200, 200, 201]);
    }

    #[test]
    fn zero_and_three_decimal_currencies_stay_exact() {
        let expenses = [
            expense(
                cat("food"),
                jpy(1_234),
                jpy(1_234),
                &[("anna", None, 1_234)],
            ),
            expense(
                cat("tea"),
                eur(1_000),
                Money::new(4_700, cur("BHD")),
                &[("anna", None, 1_000)],
            ),
        ];
        let yen = by_category(cur("JPY"), &expenses).unwrap();
        assert_eq!(yen.total, jpy(1_234));
        assert_eq!(yen.slices, [slice(cat("food"), 1_234, 1)]);
        let dinar = by_payment(cur("BHD"), &expenses).unwrap();
        assert_eq!(dinar.total, Money::new(4_700, cur("BHD")));
        assert_eq!(sum(&dinar), 4_700);
    }

    #[test]
    fn empty_group_has_no_slices() {
        let breakdown = by_category(cur("EUR"), &[]).unwrap();
        assert_eq!(breakdown.total, eur(0));
        assert!(breakdown.slices.is_empty());
        assert!(by_payment(cur("EUR"), &[]).unwrap().slices.is_empty());
    }

    #[test]
    fn ties_keep_key_order() {
        let expenses = [
            expense(cat("b"), eur(500), eur(500), &[("anna", None, 500)]),
            expense(cat("a"), eur(500), eur(500), &[("anna", None, 500)]),
        ];
        let breakdown = by_category(cur("EUR"), &expenses).unwrap();
        assert_eq!(
            breakdown.slices,
            [slice(cat("a"), 500, 1), slice(cat("b"), 500, 1)]
        );
    }

    #[test]
    fn broken_expense_is_an_error_not_a_wrong_figure() {
        let broken = expense(cat("food"), eur(1_000), eur(1_000), &[]);
        assert!(by_payment(cur("EUR"), &[broken]).is_err());
    }
}
