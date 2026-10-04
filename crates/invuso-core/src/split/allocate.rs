use std::collections::BTreeMap;

use rust_decimal::Decimal;

use super::SplitError;

/// Distributes `total` minor units proportionally to `weights` without
/// losing or creating a single unit (idee.md 8.4, SPL-05).
///
/// Largest-remainder method on exact integers: everyone first gets the
/// truncated share; the leftover units go one each to the largest
/// remainders, ties broken by key order. Negative totals are split by
/// magnitude and keep their sign. Every key of `weights` appears in the
/// result, zero-weight keys with 0.
pub fn allocate<K: Ord + Clone>(
    total: i64,
    weights: &BTreeMap<K, Decimal>,
) -> Result<BTreeMap<K, i64>, SplitError> {
    if weights.is_empty() {
        return Err(SplitError::NoParticipants);
    }
    if weights
        .values()
        .any(|w| w.is_sign_negative() && !w.is_zero())
    {
        return Err(SplitError::NegativeWeight);
    }

    let integer_weights = to_common_scale(weights)?;
    let weight_sum = integer_weights
        .iter()
        .try_fold(0_i128, |acc, w| acc.checked_add(*w))
        .ok_or(SplitError::Overflow)?;
    if weight_sum == 0 {
        return Err(SplitError::ZeroTotalWeight);
    }

    let magnitude = i128::from(total).abs();
    let mut shares = Vec::with_capacity(integer_weights.len());
    let mut distributed = 0_i128;
    for weight in &integer_weights {
        let product = magnitude.checked_mul(*weight).ok_or(SplitError::Overflow)?;
        let share = product / weight_sum;
        shares.push((share, product % weight_sum));
        distributed += share;
    }

    // Truncation leaves fewer leftover units than there are recipients.
    let leftover = magnitude - distributed;
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|&a, &b| shares[b].1.cmp(&shares[a].1).then(a.cmp(&b)));
    for &index in order
        .iter()
        .take(usize::try_from(leftover).map_err(|_| SplitError::Overflow)?)
    {
        shares[index].0 += 1;
    }

    let sign = if total < 0 { -1 } else { 1 };
    weights
        .keys()
        .zip(shares)
        .map(|(key, (share, _))| {
            let signed = i64::try_from(share * sign).map_err(|_| SplitError::Overflow)?;
            Ok((key.clone(), signed))
        })
        .collect()
}

/// Rescales `parts` (e.g. shares in the original currency) so they add up to
/// `target_total` (e.g. the converted total in the base currency).
///
/// Converting each part separately could make the converted parts disagree
/// with the converted total by a few units; distributing the converted total
/// by the original parts keeps every sum exact.
pub fn rescale<K: Ord + Clone>(
    target_total: i64,
    parts: &BTreeMap<K, i64>,
) -> Result<BTreeMap<K, i64>, SplitError> {
    if parts.is_empty() {
        return Err(SplitError::NoParticipants);
    }
    let has_positive = parts.values().any(|v| *v > 0);
    let has_negative = parts.values().any(|v| *v < 0);
    if has_positive && has_negative {
        return Err(SplitError::MixedSigns);
    }
    let weights: BTreeMap<K, Decimal> = parts
        .iter()
        .map(|(key, value)| (key.clone(), Decimal::from(value.unsigned_abs())))
        .collect();
    allocate(target_total, &weights)
}

/// Brings all weights to the largest decimal scale so they become exact
/// integers with the same unit.
fn to_common_scale<K>(weights: &BTreeMap<K, Decimal>) -> Result<Vec<i128>, SplitError> {
    let scale = weights.values().map(|w| w.scale()).max().unwrap_or(0);
    weights
        .values()
        .map(|w| {
            let factor = 10_i128
                .checked_pow(scale - w.scale())
                .ok_or(SplitError::Overflow)?;
            w.mantissa()
                .abs()
                .checked_mul(factor)
                .ok_or(SplitError::Overflow)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::domain::PersonId;

    fn w(entries: &[(&str, &str)]) -> BTreeMap<PersonId, Decimal> {
        entries
            .iter()
            .map(|(k, v)| (PersonId::from(*k), Decimal::from_str(v).unwrap()))
            .collect()
    }

    fn get(result: &BTreeMap<PersonId, i64>, key: &str) -> i64 {
        result[&PersonId::from(key)]
    }

    #[test]
    fn equal_split_gives_leftover_to_first_keys() {
        let result = allocate(1000, &w(&[("anna", "1"), ("ben", "1"), ("cleo", "1")])).unwrap();
        assert_eq!(get(&result, "anna"), 334);
        assert_eq!(get(&result, "ben"), 333);
        assert_eq!(get(&result, "cleo"), 333);
    }

    #[test]
    fn leftover_goes_to_largest_remainder_first() {
        // 100 by 1 : 2 : 2 → 20, 40, 40 exactly; 101 → 20.2, 40.4, 40.4 → +1 to ben.
        let result = allocate(101, &w(&[("anna", "1"), ("ben", "2"), ("cleo", "2")])).unwrap();
        assert_eq!(get(&result, "anna"), 20);
        assert_eq!(get(&result, "ben"), 41);
        assert_eq!(get(&result, "cleo"), 40);
    }

    #[test]
    fn decimal_weights_are_exact() {
        // Child counts half: 1 : 1 : 0.5 of 2500 → 1000, 1000, 500.
        let result = allocate(2500, &w(&[("anna", "1"), ("ben", "1.0"), ("kid", "0.5")])).unwrap();
        assert_eq!(get(&result, "anna"), 1000);
        assert_eq!(get(&result, "ben"), 1000);
        assert_eq!(get(&result, "kid"), 500);
    }

    #[test]
    fn negative_total_keeps_sign_and_sum() {
        let result = allocate(-1000, &w(&[("anna", "1"), ("ben", "1"), ("cleo", "1")])).unwrap();
        assert_eq!(result.values().sum::<i64>(), -1000);
        assert_eq!(get(&result, "anna"), -334);
    }

    #[test]
    fn zero_weight_gets_nothing_but_is_listed() {
        let result = allocate(999, &w(&[("anna", "1"), ("ben", "0")])).unwrap();
        assert_eq!(get(&result, "anna"), 999);
        assert_eq!(get(&result, "ben"), 0);
    }

    #[test]
    fn sum_is_always_preserved() {
        for total in [0, 1, 7, 99, 1001, 123_457, -5, i64::MAX / 4] {
            let result = allocate(
                total,
                &w(&[("a", "3"), ("b", "0.7"), ("c", "11"), ("d", "2.25")]),
            )
            .unwrap();
            assert_eq!(result.values().sum::<i64>(), total, "total {total}");
        }
    }

    #[test]
    fn rejects_invalid_weights() {
        assert_eq!(
            allocate(100, &BTreeMap::<PersonId, Decimal>::new()),
            Err(SplitError::NoParticipants)
        );
        assert_eq!(
            allocate(100, &w(&[("anna", "0"), ("ben", "0")])),
            Err(SplitError::ZeroTotalWeight)
        );
        assert_eq!(
            allocate(100, &w(&[("anna", "1"), ("ben", "-1")])),
            Err(SplitError::NegativeWeight)
        );
    }

    #[test]
    fn rescale_keeps_converted_total_exact() {
        // 1000 JPY split 334/333/333, converted total is 6.13 EUR = 613 cents.
        let parts: BTreeMap<PersonId, i64> = [("anna", 334), ("ben", 333), ("cleo", 333)]
            .into_iter()
            .map(|(k, v)| (PersonId::from(k), v))
            .collect();
        let result = rescale(613, &parts).unwrap();
        assert_eq!(result.values().sum::<i64>(), 613);
        assert_eq!(get(&result, "anna"), 205);
    }

    #[test]
    fn rescale_rejects_mixed_signs() {
        let parts: BTreeMap<&str, i64> = [("a", 10), ("b", -5)].into_iter().collect();
        assert_eq!(rescale(5, &parts), Err(SplitError::MixedSigns));
    }
}
