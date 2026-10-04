use std::collections::BTreeMap;

use super::SplitError;
use crate::domain::PersonId;

/// One suggested payment of the simplified debt list ("Anna → Ben: 23.40").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub from: PersonId,
    pub to: PersonId,
    pub amount_minor: i64,
}

/// Turns balances into a short list of payments that settles everyone
/// (SPL-04, idee.md 8.3): the largest debtor repeatedly pays the largest
/// creditor. Needs at most `n − 1` payments; ties go by person order, so the
/// suggestion is identical on every device.
pub fn simplify_debts(balances: &BTreeMap<PersonId, i64>) -> Result<Vec<Transfer>, SplitError> {
    let total = balances
        .values()
        .try_fold(0_i64, |acc, v| acc.checked_add(*v))
        .ok_or(SplitError::Overflow)?;
    if total != 0 {
        return Err(SplitError::BalancesDoNotSumToZero(total));
    }

    let mut open: BTreeMap<PersonId, i64> = balances
        .iter()
        .filter(|(_, balance)| **balance != 0)
        .map(|(person, balance)| (person.clone(), *balance))
        .collect();
    let mut transfers = Vec::new();

    while let (Some(creditor), Some(debtor)) = (extreme(&open, true), extreme(&open, false)) {
        let amount = open[&creditor].min(-open[&debtor]);
        transfers.push(Transfer {
            from: debtor.clone(),
            to: creditor.clone(),
            amount_minor: amount,
        });
        settle(&mut open, &creditor, -amount);
        settle(&mut open, &debtor, amount);
    }

    Ok(transfers)
}

/// Largest creditor (`want_positive`) or largest debtor; first by person
/// order on ties.
fn extreme(open: &BTreeMap<PersonId, i64>, want_positive: bool) -> Option<PersonId> {
    open.iter()
        .filter(|(_, balance)| (**balance > 0) == want_positive)
        .fold(None, |best: Option<(&PersonId, i64)>, (person, balance)| {
            let magnitude = balance.abs();
            match best {
                Some((_, best_magnitude)) if best_magnitude >= magnitude => best,
                _ => Some((person, magnitude)),
            }
        })
        .map(|(person, _)| person.clone())
}

fn settle(open: &mut BTreeMap<PersonId, i64>, person: &PersonId, delta: i64) {
    let remaining = open[person] + delta;
    if remaining == 0 {
        open.remove(person);
    } else {
        open.insert(person.clone(), remaining);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str) -> PersonId {
        PersonId::from(id)
    }

    fn balances(entries: &[(&str, i64)]) -> BTreeMap<PersonId, i64> {
        entries.iter().map(|(k, v)| (p(k), *v)).collect()
    }

    fn t(from: &str, to: &str, amount: i64) -> Transfer {
        Transfer {
            from: p(from),
            to: p(to),
            amount_minor: amount,
        }
    }

    #[test]
    fn simple_case() {
        let result = simplify_debts(&balances(&[
            ("anna", 6000),
            ("ben", -1500),
            ("cleo", -4500),
        ]));
        assert_eq!(
            result.unwrap(),
            [t("cleo", "anna", 4500), t("ben", "anna", 1500)]
        );
    }

    #[test]
    fn chain_is_shortened() {
        // Anna owes Ben 10, Ben owes Cleo 10 → Anna pays Cleo directly.
        let result = simplify_debts(&balances(&[("anna", -1000), ("ben", 0), ("cleo", 1000)]));
        assert_eq!(result.unwrap(), [t("anna", "cleo", 1000)]);
    }

    #[test]
    fn at_most_n_minus_one_payments_and_everything_settles() {
        let input = balances(&[
            ("a", 2500),
            ("b", -700),
            ("c", -1100),
            ("d", 300),
            ("e", -1000),
            ("f", 0),
        ]);
        let transfers = simplify_debts(&input).unwrap();
        assert!(transfers.len() <= 4, "{transfers:?}");

        let mut after = input.clone();
        for transfer in &transfers {
            assert!(transfer.amount_minor > 0);
            *after.get_mut(&transfer.from).unwrap() += transfer.amount_minor;
            *after.get_mut(&transfer.to).unwrap() -= transfer.amount_minor;
        }
        assert!(after.values().all(|v| *v == 0), "{after:?}");
    }

    #[test]
    fn ties_are_deterministic() {
        let result = simplify_debts(&balances(&[("anna", 500), ("ben", 500), ("cleo", -1000)]));
        assert_eq!(
            result.unwrap(),
            [t("cleo", "anna", 500), t("cleo", "ben", 500)]
        );
    }

    #[test]
    fn nothing_to_settle() {
        assert_eq!(simplify_debts(&balances(&[("anna", 0)])).unwrap(), []);
    }

    #[test]
    fn rejects_inconsistent_balances() {
        assert_eq!(
            simplify_debts(&balances(&[("anna", 100), ("ben", -99)])),
            Err(SplitError::BalancesDoNotSumToZero(1))
        );
    }
}
