use std::collections::BTreeMap;

use super::{ExpenseEntry, SettlementEntry, SplitError, Transfer, simplify_debts};
use crate::domain::PersonId;

/// Pairwise debts without simplification (SPL-07): who owes whom from what
/// they actually laid out for each other.
///
/// Within each expense, everyone owes their share to the payers in
/// proportion to what each payer paid; the rounding cents are placed so that
/// every share and every payment is matched exactly. Debts between the same
/// two people are then netted against each other, and settlements reduce the
/// debt of their pair. Debts that run in a circle (A owes B, B owes C, C
/// owes A) cancel out, so a settled group shows no debts (user decision in
/// AP-23); this only lowers amounts and never adds a pair. Per person, the
/// debts add up to exactly the balance of [`balances`](super::balances).
/// Ordered by debtor, then creditor.
pub fn pairwise_debts(
    expenses: &[ExpenseEntry],
    settlements: &[SettlementEntry],
) -> Result<Vec<Transfer>, SplitError> {
    // Key (a, b) with a < b; a positive value means a owes b.
    let mut pairs: BTreeMap<(PersonId, PersonId), i64> = BTreeMap::new();
    let mut owe = |debtor: &PersonId, creditor: &PersonId, amount: i64| -> Result<(), SplitError> {
        if debtor == creditor || amount == 0 {
            return Ok(());
        }
        let (key, signed) = if debtor < creditor {
            ((debtor.clone(), creditor.clone()), amount)
        } else {
            ((creditor.clone(), debtor.clone()), -amount)
        };
        let entry = pairs.entry(key).or_default();
        *entry = entry.checked_add(signed).ok_or(SplitError::Overflow)?;
        Ok(())
    };

    for expense in expenses {
        for (debtor, creditor, amount) in expense_flows(expense)? {
            owe(&debtor, &creditor, amount)?;
        }
    }
    for settlement in settlements {
        // Paying someone is the same as them now owing you that amount.
        owe(&settlement.to, &settlement.from, settlement.amount_minor)?;
    }

    // Directed: (debtor, creditor) → positive amount.
    let mut edges: BTreeMap<(PersonId, PersonId), i64> = pairs
        .into_iter()
        .filter(|(_, amount)| *amount != 0)
        .map(|((a, b), amount)| {
            if amount > 0 {
                ((a, b), amount)
            } else {
                ((b, a), -amount)
            }
        })
        .collect();
    while let Some(cycle) = find_cycle(&edges) {
        let smallest = cycle
            .iter()
            .map(|edge| edges[edge])
            .min()
            .unwrap_or_default();
        for edge in cycle {
            let remaining = edges[&edge] - smallest;
            if remaining == 0 {
                edges.remove(&edge);
            } else {
                edges.insert(edge, remaining);
            }
        }
    }

    // BTreeMap order is debtor, then creditor.
    Ok(edges
        .into_iter()
        .map(|((from, to), amount_minor)| Transfer {
            from,
            to,
            amount_minor,
        })
        .collect())
}

/// The edges of the first circle a depth-first search finds, starting
/// people and following creditors in person order; `None` if there is none.
fn find_cycle(edges: &BTreeMap<(PersonId, PersonId), i64>) -> Option<Vec<(PersonId, PersonId)>> {
    let mut graph: BTreeMap<&PersonId, Vec<&PersonId>> = BTreeMap::new();
    for (from, to) in edges.keys() {
        graph.entry(from).or_default().push(to);
    }
    let mut done: std::collections::BTreeSet<&PersonId> = std::collections::BTreeSet::new();
    for start in graph.keys().copied() {
        if done.contains(start) {
            continue;
        }
        // Path from `start` and, per step, the index of the next creditor.
        let mut path: Vec<(&PersonId, usize)> = vec![(start, 0)];
        while let Some((node, next)) = path.last_mut() {
            let node = *node;
            let creditors = graph.get(node).map_or(&[][..], Vec::as_slice);
            let Some(creditor) = creditors.get(*next).copied() else {
                done.insert(node);
                path.pop();
                continue;
            };
            *next += 1;
            if let Some(index) = path.iter().position(|(p, _)| *p == creditor) {
                let mut cycle: Vec<(PersonId, PersonId)> = path[index..]
                    .windows(2)
                    .map(|w| (w[0].0.clone(), w[1].0.clone()))
                    .collect();
                cycle.push((node.clone(), creditor.clone()));
                return Some(cycle);
            }
            if !done.contains(creditor) {
                path.push((creditor, 0));
            }
        }
    }
    None
}

/// `(debtor, creditor, amount)` of one expense, self-payments included.
fn expense_flows(expense: &ExpenseEntry) -> Result<Vec<(PersonId, PersonId, i64)>, SplitError> {
    let paid = sum(expense.payments.values())?;
    let shared = sum(expense.shares.values())?;
    if paid != shared {
        return Err(SplitError::UnbalancedExpense { paid, shared });
    }
    let proportional = paid > 0
        && expense.payments.values().all(|amount| *amount >= 0)
        && expense.shares.values().all(|amount| *amount >= 0);
    if !proportional {
        // A negative share (e.g. someone only got a discount) has no
        // meaningful proportion; fall back to the simplified flows of this
        // one expense, which are still exact.
        let net = net_of(expense)?;
        return Ok(simplify_debts(&net)?
            .into_iter()
            .map(|t| (t.from, t.to, t.amount_minor))
            .collect());
    }

    // Floors of share × payment / total, then the missing cents per row
    // (share) and column (payment) matched up in person order. Each row
    // misses less than one cent per payer, so the result stays within a
    // few cents of the exact proportion.
    let total = i128::from(paid);
    let mut flows: BTreeMap<(PersonId, PersonId), i64> = BTreeMap::new();
    let mut row_missing: Vec<(PersonId, i64)> = Vec::new();
    let mut column_missing: BTreeMap<PersonId, i64> = expense.payments.clone();
    for (debtor, share) in &expense.shares {
        let mut left = *share;
        for (creditor, payment) in &expense.payments {
            let part = i128::from(*share) * i128::from(*payment) / total;
            let part = i64::try_from(part).map_err(|_| SplitError::Overflow)?;
            flows.insert((debtor.clone(), creditor.clone()), part);
            left -= part;
            if let Some(column) = column_missing.get_mut(creditor) {
                *column -= part;
            }
        }
        row_missing.push((debtor.clone(), left));
    }
    for (debtor, mut left) in row_missing {
        for (creditor, column) in column_missing.iter_mut() {
            if left == 0 {
                break;
            }
            let extra = left.min(*column);
            if extra > 0 {
                *flows.entry((debtor.clone(), creditor.clone())).or_default() += extra;
                left -= extra;
                *column -= extra;
            }
        }
    }

    Ok(flows
        .into_iter()
        .filter(|(_, amount)| *amount != 0)
        .map(|((debtor, creditor), amount)| (debtor, creditor, amount))
        .collect())
}

/// Paid minus share of every person in one expense.
fn net_of(expense: &ExpenseEntry) -> Result<BTreeMap<PersonId, i64>, SplitError> {
    let mut net: BTreeMap<PersonId, i64> = BTreeMap::new();
    for (person, amount) in &expense.payments {
        let entry = net.entry(person.clone()).or_default();
        *entry = entry.checked_add(*amount).ok_or(SplitError::Overflow)?;
    }
    for (person, amount) in &expense.shares {
        let entry = net.entry(person.clone()).or_default();
        *entry = entry.checked_sub(*amount).ok_or(SplitError::Overflow)?;
    }
    Ok(net)
}

fn sum<'a>(mut values: impl Iterator<Item = &'a i64>) -> Result<i64, SplitError> {
    values.try_fold(0_i64, |acc, v| {
        acc.checked_add(*v).ok_or(SplitError::Overflow)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::balances;

    fn p(id: &str) -> PersonId {
        PersonId::from(id)
    }

    fn map(entries: &[(&str, i64)]) -> BTreeMap<PersonId, i64> {
        entries.iter().map(|(k, v)| (p(k), *v)).collect()
    }

    fn debt(from: &str, to: &str, amount_minor: i64) -> Transfer {
        Transfer {
            from: p(from),
            to: p(to),
            amount_minor,
        }
    }

    /// Per person, owed to them minus owed by them.
    fn net(debts: &[Transfer]) -> BTreeMap<PersonId, i64> {
        let mut net = BTreeMap::new();
        for d in debts {
            *net.entry(d.to.clone()).or_default() += d.amount_minor;
            *net.entry(d.from.clone()).or_default() -= d.amount_minor;
        }
        net
    }

    fn assert_matches_balances(expenses: &[ExpenseEntry], settlements: &[SettlementEntry]) {
        let debts = pairwise_debts(expenses, settlements).unwrap();
        let net = net(&debts);
        for (person, totals) in balances(expenses, settlements).unwrap() {
            assert_eq!(net.get(&person).copied().unwrap_or(0), totals.balance);
        }
    }

    #[test]
    fn single_payer_is_owed_every_share() {
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 9_000)]),
            shares: map(&[("anna", 3_000), ("ben", 3_000), ("cleo", 3_000)]),
        }];
        assert_eq!(
            pairwise_debts(&expenses, &[]).unwrap(),
            [debt("ben", "anna", 3_000), debt("cleo", "anna", 3_000)]
        );
    }

    #[test]
    fn debts_between_two_people_are_netted() {
        // Anna pays 30.00 for both, Ben pays 10.00 for both: Ben owes Anna
        // 15.00 − 5.00.
        let expenses = [
            ExpenseEntry {
                payments: map(&[("anna", 3_000)]),
                shares: map(&[("anna", 1_500), ("ben", 1_500)]),
            },
            ExpenseEntry {
                payments: map(&[("ben", 1_000)]),
                shares: map(&[("anna", 500), ("ben", 500)]),
            },
        ];
        assert_eq!(
            pairwise_debts(&expenses, &[]).unwrap(),
            [debt("ben", "anna", 1_000)]
        );
    }

    #[test]
    fn keeps_debts_that_simplifying_would_merge() {
        // Cleo owes Ben, Ben owes Anna: the simplified list would let Cleo
        // pay Anna directly, the pairwise list keeps both.
        let expenses = [
            ExpenseEntry {
                payments: map(&[("anna", 1_000)]),
                shares: map(&[("ben", 1_000)]),
            },
            ExpenseEntry {
                payments: map(&[("ben", 1_000)]),
                shares: map(&[("cleo", 1_000)]),
            },
        ];
        assert_eq!(
            pairwise_debts(&expenses, &[]).unwrap(),
            [debt("ben", "anna", 1_000), debt("cleo", "ben", 1_000)]
        );
        assert_matches_balances(&expenses, &[]);
    }

    #[test]
    fn two_payers_are_owed_in_proportion_to_what_they_paid() {
        // Anna paid 2/3, Ben 1/3; Cleo's 30.00 go 20.00 / 10.00.
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 6_000), ("ben", 3_000)]),
            shares: map(&[("anna", 3_000), ("ben", 3_000), ("cleo", 3_000)]),
        }];
        assert_eq!(
            pairwise_debts(&expenses, &[]).unwrap(),
            [
                debt("ben", "anna", 1_000),
                debt("cleo", "anna", 2_000),
                debt("cleo", "ben", 1_000)
            ]
        );
        assert_matches_balances(&expenses, &[]);
    }

    #[test]
    fn rounding_cents_keep_shares_and_payments_exact() {
        // 1.00 paid 0.01 + 0.99, three shares 0.34 / 0.33 / 0.33: every
        // proportion has a remainder.
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 1), ("ben", 99)]),
            shares: map(&[("cleo", 34), ("dan", 33), ("eve", 33)]),
        }];
        let debts = pairwise_debts(&expenses, &[]).unwrap();
        let net = net(&debts);
        assert_eq!(net[&p("anna")], 1);
        assert_eq!(net[&p("ben")], 99);
        assert_eq!(net[&p("cleo")], -34);
        assert_eq!(net[&p("dan")], -33);
        assert_eq!(net[&p("eve")], -33);
    }

    #[test]
    fn settlements_reduce_their_pair_and_may_turn_it_around() {
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 6_000)]),
            shares: map(&[("anna", 3_000), ("ben", 3_000)]),
        }];
        let part = [SettlementEntry {
            from: p("ben"),
            to: p("anna"),
            amount_minor: 1_000,
        }];
        assert_eq!(
            pairwise_debts(&expenses, &part).unwrap(),
            [debt("ben", "anna", 2_000)]
        );
        let full = [SettlementEntry {
            from: p("ben"),
            to: p("anna"),
            amount_minor: 3_000,
        }];
        assert_eq!(pairwise_debts(&expenses, &full).unwrap(), []);
        let too_much = [SettlementEntry {
            from: p("ben"),
            to: p("anna"),
            amount_minor: 3_500,
        }];
        assert_eq!(
            pairwise_debts(&expenses, &too_much).unwrap(),
            [debt("anna", "ben", 500)]
        );
    }

    #[test]
    fn settlement_outside_the_pair_still_matches_the_balances() {
        // The simplified list told Cleo to pay Anna, though she owes Ben.
        let expenses = [
            ExpenseEntry {
                payments: map(&[("anna", 1_000)]),
                shares: map(&[("ben", 1_000)]),
            },
            ExpenseEntry {
                payments: map(&[("ben", 1_000)]),
                shares: map(&[("cleo", 1_000)]),
            },
        ];
        let settlements = [SettlementEntry {
            from: p("cleo"),
            to: p("anna"),
            amount_minor: 1_000,
        }];
        // Ben → Anna, Cleo → Ben and Anna → Cleo form a circle: all settled.
        assert_eq!(pairwise_debts(&expenses, &settlements).unwrap(), []);
        assert_matches_balances(&expenses, &settlements);
    }

    #[test]
    fn circles_cancel_down_to_their_smallest_debt() {
        // Ben owes Anna 5, Cleo owes Ben 3, Anna owes Cleo 3: the circle
        // takes 3 off each, leaving Ben → Anna 2.
        let expenses = [
            ExpenseEntry {
                payments: map(&[("anna", 500)]),
                shares: map(&[("ben", 500)]),
            },
            ExpenseEntry {
                payments: map(&[("ben", 300)]),
                shares: map(&[("cleo", 300)]),
            },
            ExpenseEntry {
                payments: map(&[("cleo", 300)]),
                shares: map(&[("anna", 300)]),
            },
        ];
        assert_eq!(
            pairwise_debts(&expenses, &[]).unwrap(),
            [debt("ben", "anna", 200)]
        );
        assert_matches_balances(&expenses, &[]);
    }

    #[test]
    fn settled_group_has_no_pairwise_debts() {
        // The device test of AP-23: everyone paid "Ich" what the simplified
        // list said, though they also owed each other.
        let all = &[("me", 1), ("anna", 1), ("ben", 1), ("cleo", 1)];
        let equal = |payer: &str, total: i64, between: &[(&str, i64)]| ExpenseEntry {
            payments: map(&[(payer, total)]),
            shares: between
                .iter()
                .map(|(p, _)| (PersonId::from(*p), total / between.len() as i64))
                .collect(),
        };
        let expenses = [
            equal("me", 36_000, all),
            equal("anna", 4_800, all),
            equal("ben", 3_000, &[("anna", 1), ("ben", 1), ("cleo", 1)]),
        ];
        let paid = |from: &str, amount_minor: i64| SettlementEntry {
            from: p(from),
            to: p("me"),
            amount_minor,
        };
        let settlements = [
            paid("anna", 2_000),
            paid("cleo", 11_200),
            paid("ben", 8_200),
            paid("anna", 4_400),
        ];
        assert_eq!(pairwise_debts(&expenses, &settlements).unwrap(), []);
    }

    #[test]
    fn negative_share_falls_back_to_exact_flows() {
        // Ben only got a −2.00 discount; Anna paid 8.00 net.
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 800)]),
            shares: map(&[("anna", 500), ("ben", -200), ("cleo", 500)]),
        }];
        assert_matches_balances(&expenses, &[]);
    }

    #[test]
    fn zero_decimal_currency_and_empty_input() {
        // 1,000 ¥ for three: 334 / 333 / 333.
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 1_000)]),
            shares: map(&[("anna", 334), ("ben", 333), ("cleo", 333)]),
        }];
        assert_eq!(
            pairwise_debts(&expenses, &[]).unwrap(),
            [debt("ben", "anna", 333), debt("cleo", "anna", 333)]
        );
        assert_eq!(pairwise_debts(&[], &[]).unwrap(), []);
    }

    #[test]
    fn rejects_unbalanced_expense() {
        let expenses = [ExpenseEntry {
            payments: map(&[("anna", 1_000)]),
            shares: map(&[("ben", 999)]),
        }];
        assert!(pairwise_debts(&expenses, &[]).is_err());
    }
}
