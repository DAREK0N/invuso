use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;

use super::{ItemLine, SplitError, allocate, split_by_items};
use crate::domain::PersonId;

/// How an expense is shared (idee.md 8.1, SPL-01).
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
    /// By line items (idee.md 8.2, SPL-02): `items` belong to whom they are
    /// assigned to, unassigned ones and the rest up to the total are shared
    /// by `participants` with their default weights.
    Items {
        participants: BTreeMap<PersonId, Decimal>,
        items: Vec<ItemLine>,
    },
}

impl SplitMode {
    /// Stable code of the mode, stored as `Expense.split_mode`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Equal(_) => "equal",
            Self::Weights(_) => "weights",
            Self::Percent(_) => "percent",
            Self::Exact(_) => "exact",
            Self::Items { .. } => "items",
        }
    }

    /// Everyone the expense is split between, including people whose
    /// weight, percentage or amount is zero and, by items, everyone a line
    /// is assigned to.
    pub fn participants(&self) -> BTreeSet<PersonId> {
        match self {
            Self::Equal(people) => people.clone(),
            Self::Weights(map) | Self::Percent(map) => map.keys().cloned().collect(),
            Self::Exact(map) => map.keys().cloned().collect(),
            Self::Items {
                participants,
                items,
            } => participants
                .keys()
                .chain(items.iter().flat_map(|item| item.assigned_to.keys()))
                .cloned()
                .collect(),
        }
    }
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
        SplitMode::Items {
            participants,
            items,
        } => split_by_items(total, participants, items),
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
    fn participants_of_every_mode() {
        let both: BTreeSet<_> = [p("anna"), p("ben")].into();
        assert_eq!(SplitMode::Equal(both.clone()).participants(), both);
        let weights = [(p("anna"), d("2")), (p("ben"), d("0"))].into();
        assert_eq!(SplitMode::Weights(weights).participants(), both);
        let percents = [(p("anna"), d("100")), (p("ben"), d("0"))].into();
        assert_eq!(SplitMode::Percent(percents).participants(), both);
        let amounts = [(p("anna"), 5), (p("ben"), 0)].into();
        assert_eq!(SplitMode::Exact(amounts).participants(), both);
    }

    #[test]
    fn items_mode_splits_by_assignment() {
        // 6 beers at 4.50: Anna 2, Ben 1, Cleo 3; shared snacks 6.00.
        let beer = ItemLine {
            amount_minor: 2_700,
            assigned_to: [(p("anna"), d("2")), (p("ben"), d("1")), (p("cleo"), d("3"))].into(),
        };
        let snacks = ItemLine {
            amount_minor: 600,
            assigned_to: BTreeMap::new(),
        };
        let mode = SplitMode::Items {
            participants: [(p("anna"), d("1")), (p("ben"), d("1"))].into(),
            items: vec![beer, snacks],
        };
        assert_eq!(mode.code(), "items");
        assert_eq!(mode.participants(), [p("anna"), p("ben"), p("cleo")].into());
        let result = split(3_300, &mode).unwrap();
        assert_eq!(
            result,
            [(p("anna"), 1_200), (p("ben"), 750), (p("cleo"), 1_350)].into()
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
