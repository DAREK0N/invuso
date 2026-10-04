use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;

use super::{SplitError, allocate};
use crate::domain::PersonId;

/// How an expense without line items is shared (idee.md 8.1, SPL-01).
/// Splitting by line items is [`split_by_items`](super::split_by_items).
#[derive(Debug, Clone, PartialEq)]
pub enum SplitMode {
    /// Everyone listed pays the same.
    Equal(BTreeSet<PersonId>),
    /// Proportional to weights, e.g. 2 : 1 : 1.
    Weights(BTreeMap<PersonId, Decimal>),
    /// Percentages that must add up to exactly 100.
    Percent(BTreeMap<PersonId, Decimal>),
    /// Fixed amounts in minor units that must add up to the total.
    Exact(BTreeMap<PersonId, i64>),
}

/// Each person's share of `total` minor units.
pub fn split(total: i64, mode: &SplitMode) -> Result<BTreeMap<PersonId, i64>, SplitError> {
    match mode {
        SplitMode::Equal(people) => {
            let weights = people.iter().map(|p| (p.clone(), Decimal::ONE)).collect();
            allocate(total, &weights)
        }
        SplitMode::Weights(weights) => allocate(total, weights),
        SplitMode::Percent(percents) => {
            let sum: Decimal = percents.values().sum();
            if sum != Decimal::ONE_HUNDRED {
                return Err(SplitError::PercentNot100(sum));
            }
            allocate(total, percents)
        }
        SplitMode::Exact(amounts) => {
            if amounts.is_empty() {
                return Err(SplitError::NoParticipants);
            }
            let actual = amounts
                .values()
                .try_fold(0_i64, |acc, v| acc.checked_add(*v))
                .ok_or(SplitError::Overflow)?;
            if actual != total {
                return Err(SplitError::ExactSumMismatch {
                    expected: total,
                    actual,
                });
            }
            Ok(amounts.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn p(id: &str) -> PersonId {
        PersonId::from(id)
    }

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    #[test]
    fn equal() {
        let people = [p("anna"), p("ben"), p("cleo")].into_iter().collect();
        let result = split(1000, &SplitMode::Equal(people)).unwrap();
        assert_eq!(
            result.values().copied().collect::<Vec<_>>(),
            [334, 333, 333]
        );
    }

    #[test]
    fn weights() {
        let weights = [(p("anna"), d("2")), (p("ben"), d("1")), (p("cleo"), d("1"))].into();
        let result = split(1000, &SplitMode::Weights(weights)).unwrap();
        assert_eq!(
            result.values().copied().collect::<Vec<_>>(),
            [500, 250, 250]
        );
    }

    #[test]
    fn percent_must_be_100() {
        let ok = [(p("anna"), d("70")), (p("ben"), d("30"))].into();
        let result = split(999, &SplitMode::Percent(ok)).unwrap();
        assert_eq!(result.values().copied().collect::<Vec<_>>(), [699, 300]);

        let bad = [(p("anna"), d("70")), (p("ben"), d("20"))].into();
        assert_eq!(
            split(999, &SplitMode::Percent(bad)),
            Err(SplitError::PercentNot100(d("90")))
        );
    }

    #[test]
    fn exact_must_match_total() {
        let ok: BTreeMap<_, _> = [(p("anna"), 700), (p("ben"), 300)].into();
        assert_eq!(split(1000, &SplitMode::Exact(ok.clone())).unwrap(), ok);
        assert_eq!(
            split(1001, &SplitMode::Exact(ok)),
            Err(SplitError::ExactSumMismatch {
                expected: 1001,
                actual: 1000
            })
        );
    }

    #[test]
    fn nobody_to_split_between() {
        assert_eq!(
            split(100, &SplitMode::Equal(BTreeSet::new())),
            Err(SplitError::NoParticipants)
        );
    }
}
