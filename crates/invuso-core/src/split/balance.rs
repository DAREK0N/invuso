use std::collections::BTreeMap;

use super::SplitError;
use crate::domain::PersonId;

/// One expense in the group's base currency: who paid how much and who
/// carries how much. Both sides must add up to the same total.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpenseEntry {
    pub payments: BTreeMap<PersonId, i64>,
    pub shares: BTreeMap<PersonId, i64>,
}

/// A recorded settlement payment `from` → `to` in the base currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementEntry {
    pub from: PersonId,
    pub to: PersonId,
    pub amount_minor: i64,
}

/// Per-person figures of a group (GRP-11, GRP-12, GRP-14), all in base
/// currency minor units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PersonTotals {
    /// Paid for expenses.
    pub paid: i64,
    /// Own share of expenses ("verbraucht").
    pub consumed: i64,
    /// Settlement payments sent to others.
    pub settled_out: i64,
    /// Settlement payments received from others.
    pub settled_in: i64,
    /// `paid − consumed + settled_out − settled_in`: positive = gets money
    /// back, negative = owes money (idee.md 8.3).
    pub balance: i64,
}

/// Computes every person's totals. The balances of a valid group always add
/// up to exactly 0.
pub fn balances(
    expenses: &[ExpenseEntry],
    settlements: &[SettlementEntry],
) -> Result<BTreeMap<PersonId, PersonTotals>, SplitError> {
    let mut totals: BTreeMap<PersonId, PersonTotals> = BTreeMap::new();

    for expense in expenses {
        let paid = sum(expense.payments.values())?;
        let shared = sum(expense.shares.values())?;
        if paid != shared {
            return Err(SplitError::UnbalancedExpense { paid, shared });
        }
        for (person, amount) in &expense.payments {
            let entry = totals.entry(person.clone()).or_default();
            entry.paid = checked_add(entry.paid, *amount)?;
        }
        for (person, amount) in &expense.shares {
            let entry = totals.entry(person.clone()).or_default();
            entry.consumed = checked_add(entry.consumed, *amount)?;
        }
    }

    for settlement in settlements {
        let from = totals.entry(settlement.from.clone()).or_default();
        from.settled_out = checked_add(from.settled_out, settlement.amount_minor)?;
        let to = totals.entry(settlement.to.clone()).or_default();
        to.settled_in = checked_add(to.settled_in, settlement.amount_minor)?;
    }

    for entry in totals.values_mut() {
        entry.balance = entry
            .paid
            .checked_sub(entry.consumed)
            .and_then(|v| v.checked_add(entry.settled_out))
            .and_then(|v| v.checked_sub(entry.settled_in))
            .ok_or(SplitError::Overflow)?;
    }

    Ok(totals)
}

fn sum<'a>(mut values: impl Iterator<Item = &'a i64>) -> Result<i64, SplitError> {
    values.try_fold(0_i64, |acc, v| checked_add(acc, *v))
}

fn checked_add(a: i64, b: i64) -> Result<i64, SplitError> {
    a.checked_add(b).ok_or(SplitError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str) -> PersonId {
        PersonId::from(id)
    }

    fn map(entries: &[(&str, i64)]) -> BTreeMap<PersonId, i64> {
        entries.iter().map(|(k, v)| (p(k), *v)).collect()
    }

    #[test]
    fn balances_follow_paid_minus_consumed() {
        let expenses = [
            // Anna pays dinner 90.00 for three.
            ExpenseEntry {
                payments: map(&[("anna", 9000)]),
                shares: map(&[("anna", 3000), ("ben", 3000), ("cleo", 3000)]),
            },
            // Ben pays taxi 30.00 for Ben and Cleo.
            ExpenseEntry {
                payments: map(&[("ben", 3000)]),
                shares: map(&[("ben", 1500), ("cleo", 1500)]),
            },
        ];
        let result = balances(&expenses, &[]).unwrap();
        assert_eq!(result[&p("anna")].balance, 6000);
        assert_eq!(result[&p("ben")].balance, -1500);
        assert_eq!(result[&p("cleo")].balance, -4500);
        assert_eq!(result.values().map(|t| t.balance).sum::<i64>(), 0);
        assert_eq!(result[&p("anna")].paid, 9000);
        assert_eq!(result[&p("cleo")].consumed, 4500);
    }

    #[test]
    fn settlements_move_balances_towards_zero() {
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 6000)]),
            shares: map(&[("anna", 3000), ("ben", 3000)]),
        }];
        let settlements = [SettlementEntry {
            from: p("ben"),
            to: p("anna"),
            amount_minor: 2000,
        }];
        let result = balances(&expenses, &settlements).unwrap();
        assert_eq!(result[&p("anna")].balance, 1000);
        assert_eq!(result[&p("ben")].balance, -1000);
        assert_eq!(result[&p("ben")].settled_out, 2000);
        assert_eq!(result[&p("anna")].settled_in, 2000);
    }

    #[test]
    fn split_payment_by_two_payers() {
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 2500), ("ben", 500)]),
            shares: map(&[("anna", 1000), ("ben", 1000), ("cleo", 1000)]),
        }];
        let result = balances(&expenses, &[]).unwrap();
        assert_eq!(result[&p("anna")].balance, 1500);
        assert_eq!(result[&p("ben")].balance, -500);
        assert_eq!(result[&p("cleo")].balance, -1000);
    }

    #[test]
    fn rejects_unbalanced_expense() {
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 1000)]),
            shares: map(&[("anna", 500), ("ben", 499)]),
        }];
        assert_eq!(
            balances(&expenses, &[]),
            Err(SplitError::UnbalancedExpense {
                paid: 1000,
                shared: 999
            })
        );
    }
}
