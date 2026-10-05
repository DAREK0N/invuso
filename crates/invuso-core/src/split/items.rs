use std::collections::BTreeMap;

use rust_decimal::Decimal;

use super::{SplitError, allocate};
use crate::domain::PersonId;

/// One assignable receipt line (article, discount, deposit) in minor units.
///
/// Lines that cannot be assigned (tax added on top, tip, service charge,
/// discount on the whole receipt) are *not* passed as items: they are the
/// difference between the expense total and the item sum and are shared
/// proportionally (idee.md 8.2 step 4).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemLine {
    pub amount_minor: i64,
    /// Who carries this line, with weights. Empty = everyone (the
    /// "Allgemeinheit"), split by the participants' default weights.
    pub assigned_to: BTreeMap<PersonId, Decimal>,
}

/// Splits an expense by its line items (idee.md 8.2, SPL-02, SPL-08).
///
/// 1. Lines assigned to people are shared only among them.
/// 2. Unassigned lines are shared among `participants` by default weight.
/// 3. `total − Σ items` (tax, tip, rounding, receipt-wide discount) is shared
///    proportionally to what each person carries so far; if nobody carries
///    anything positive yet, by default weight.
///
/// The result lists every participant and every assigned person and adds up
/// to `total` exactly.
pub fn split_by_items(
    total: i64,
    participants: &BTreeMap<PersonId, Decimal>,
    items: &[ItemLine],
) -> Result<BTreeMap<PersonId, i64>, SplitError> {
    if participants.is_empty() {
        return Err(SplitError::NoParticipants);
    }

    let mut shares: BTreeMap<PersonId, i64> = participants.keys().map(|p| (p.clone(), 0)).collect();
    // Lines with the same holders are added up before rounding, so the
    // rest units of many small lines do not all land on the same people.
    let mut pooled: BTreeMap<&BTreeMap<PersonId, Decimal>, i64> = BTreeMap::new();
    let mut items_sum = 0_i64;
    for item in items {
        let weights = if item.assigned_to.is_empty() {
            participants
        } else {
            &item.assigned_to
        };
        let pool = pooled.entry(weights).or_insert(0);
        *pool = pool
            .checked_add(item.amount_minor)
            .ok_or(SplitError::Overflow)?;
        items_sum = items_sum
            .checked_add(item.amount_minor)
            .ok_or(SplitError::Overflow)?;
    }
    for (weights, amount) in pooled {
        for (person, part) in allocate(amount, weights)? {
            add(&mut shares, person, part)?;
        }
    }

    let rest = total.checked_sub(items_sum).ok_or(SplitError::Overflow)?;
    if rest != 0 {
        let proportional: BTreeMap<PersonId, Decimal> = shares
            .iter()
            .map(|(person, share)| (person.clone(), Decimal::from((*share).max(0))))
            .collect();
        let weights = if proportional.values().any(|w| !w.is_zero()) {
            &proportional
        } else {
            participants
        };
        for (person, amount) in allocate(rest, weights)? {
            add(&mut shares, person, amount)?;
        }
    }

    Ok(shares)
}

fn add(
    shares: &mut BTreeMap<PersonId, i64>,
    person: PersonId,
    amount: i64,
) -> Result<(), SplitError> {
    let entry = shares.entry(person).or_insert(0);
    *entry = entry.checked_add(amount).ok_or(SplitError::Overflow)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn p(id: &str) -> PersonId {
        PersonId::from(id)
    }

    fn everyone(ids: &[&str]) -> BTreeMap<PersonId, Decimal> {
        ids.iter().map(|id| (p(id), Decimal::ONE)).collect()
    }

    fn general(amount: i64) -> ItemLine {
        ItemLine {
            amount_minor: amount,
            assigned_to: BTreeMap::new(),
        }
    }

    fn only(amount: i64, ids: &[&str]) -> ItemLine {
        ItemLine {
            amount_minor: amount,
            assigned_to: everyone(ids),
        }
    }

    #[test]
    fn personal_items_are_not_shared() {
        // Shared pizza 30.00, Anna's beer 5.00, Ben's water 2.00; total 37.00.
        let items = [general(3000), only(500, &["anna"]), only(200, &["ben"])];
        let result = split_by_items(3700, &everyone(&["anna", "ben", "cleo"]), &items).unwrap();
        assert_eq!(result[&p("anna")], 1500);
        assert_eq!(result[&p("ben")], 1200);
        assert_eq!(result[&p("cleo")], 1000);
        assert_eq!(result.values().sum::<i64>(), 3700);
    }

    #[test]
    fn tip_is_shared_proportionally() {
        // Items 20.00 (Anna) + 10.00 (Ben); 3.00 tip on top → 2.00 / 1.00.
        let items = [only(2000, &["anna"]), only(1000, &["ben"])];
        let result = split_by_items(3300, &everyone(&["anna", "ben"]), &items).unwrap();
        assert_eq!(result[&p("anna")], 2200);
        assert_eq!(result[&p("ben")], 1100);
    }

    #[test]
    fn receipt_discount_reduces_shares_proportionally() {
        let items = [only(3000, &["anna"]), only(1000, &["ben"])];
        let result = split_by_items(3600, &everyone(&["anna", "ben"]), &items).unwrap();
        assert_eq!(result[&p("anna")], 2700);
        assert_eq!(result[&p("ben")], 900);
    }

    #[test]
    fn item_shared_by_some_with_weights() {
        let mut item = only(900, &["anna", "ben"]);
        item.assigned_to
            .insert(p("anna"), Decimal::from_str("2").unwrap());
        let result = split_by_items(900, &everyone(&["anna", "ben", "cleo"]), &[item]).unwrap();
        assert_eq!(result[&p("anna")], 600);
        assert_eq!(result[&p("ben")], 300);
        assert_eq!(result[&p("cleo")], 0);
    }

    #[test]
    fn person_outside_participants_can_carry_an_item() {
        let items = [general(1000), only(400, &["guest"])];
        let result = split_by_items(1400, &everyone(&["anna", "ben"]), &items).unwrap();
        assert_eq!(result[&p("guest")], 400);
        assert_eq!(result.values().sum::<i64>(), 1400);
    }

    #[test]
    fn no_items_falls_back_to_default_weights() {
        let result = split_by_items(1000, &everyone(&["anna", "ben", "cleo"]), &[]).unwrap();
        assert_eq!(
            result.values().copied().collect::<Vec<_>>(),
            [334, 333, 333]
        );
    }

    #[test]
    fn small_shared_lines_are_rounded_together() {
        // Each line alone would give its rest cent to Anna (and Ben):
        // 0.50/0.50/0.49 + 1.33×3 + 0.20/0.20/0.19 + 0.33×3 → 2.36/2.36/2.34.
        let items = [general(149), general(399), general(59), general(99)];
        let result = split_by_items(706, &everyone(&["anna", "ben", "cleo"]), &items).unwrap();
        assert_eq!(
            result.values().copied().collect::<Vec<_>>(),
            [236, 235, 235]
        );

        // Same holders with different weights are separate pools.
        let mut heavy = only(100, &["anna", "ben"]);
        heavy
            .assigned_to
            .insert(p("anna"), Decimal::from_str("3").unwrap());
        let items = [only(101, &["anna", "ben"]), heavy];
        let result = split_by_items(201, &everyone(&["anna", "ben"]), &items).unwrap();
        assert_eq!(result[&p("anna")], 51 + 75);
        assert_eq!(result[&p("ben")], 50 + 25);
    }

    #[test]
    fn deposit_return_as_negative_item() {
        // Shared groceries 12.00, Anna's bottle deposit return −0.75.
        let items = [general(1200), only(-75, &["anna"])];
        let result = split_by_items(1125, &everyone(&["anna", "ben"]), &items).unwrap();
        assert_eq!(result[&p("anna")], 525);
        assert_eq!(result[&p("ben")], 600);
    }
}
