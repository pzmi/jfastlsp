//! The few statistics the harness reports: median, percentile (nearest rank, as the E1 spike
//! did) and the relative spread that states a run's noise.

/// `p` in 0..=1 of an ascending sample, nearest rank; 0 for an empty sample.
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let last = sorted.len().saturating_sub(1);
    let rank = (last as f64 * p.clamp(0.0, 1.0)).round();
    let index = (rank as usize).min(last);
    sorted.get(index).copied().unwrap_or(0.0)
}

/// `values` ascending (NaN sorts last, so it never becomes a median by accident).
pub fn sorted(values: &[f64]) -> Vec<f64> {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v
}

/// The percentile of an unsorted sample.
pub fn percentile(values: &[f64], p: f64) -> f64 {
    percentile_sorted(&sorted(values), p)
}

/// The median; the mean of the two middle values for an even count; 0 for an empty sample.
pub fn median(values: &[f64]) -> f64 {
    let v = sorted(values);
    let n = v.len();
    if n == 0 {
        return 0.0;
    }
    let mid = n / 2;
    let hi = v.get(mid).copied().unwrap_or(0.0);
    if n % 2 == 1 {
        return hi;
    }
    let lo = v.get(mid.saturating_sub(1)).copied().unwrap_or(0.0);
    (lo + hi) / 2.0
}

/// `(max - min) / median` in percent: how far apart a metric's runs were. 0 for fewer than two
/// runs or a zero median.
pub fn spread_pct(values: &[f64]) -> f64 {
    let m = median(values);
    if values.len() < 2 || m == 0.0 {
        return 0.0;
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (max - min) / m.abs() * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_take_the_middle_value_when_the_count_is_odd() {
        // given
        let xs = [5.0, 1.0, 3.0];
        // when
        let m = median(&xs);
        // then
        assert_eq!(m, 3.0);
    }

    #[test]
    fn should_average_the_middle_pair_when_the_count_is_even() {
        // given
        let xs = [4.0, 1.0, 3.0, 2.0];
        // when
        let m = median(&xs);
        // then
        assert_eq!(m, 2.5);
    }

    #[test]
    fn should_pick_nearest_rank_when_asked_for_a_percentile() {
        // given: 1..=20
        let xs: Vec<f64> = (1..=20).map(f64::from).collect();
        // when
        let p50 = percentile(&xs, 0.5);
        let p95 = percentile(&xs, 0.95);
        let max = percentile(&xs, 1.0);
        // then: (n - 1) * p rounded, as the E1 spike's pct()
        assert_eq!(p50, 11.0);
        assert_eq!(p95, 19.0);
        assert_eq!(max, 20.0);
    }

    #[test]
    fn should_return_zero_when_the_sample_is_empty() {
        // given
        // when
        // then
        assert_eq!(median(&[]), 0.0);
        assert_eq!(percentile(&[], 0.95), 0.0);
        assert_eq!(spread_pct(&[]), 0.0);
    }

    #[test]
    fn should_state_the_relative_range_when_runs_differ() {
        // given
        let xs = [90.0, 100.0, 110.0];
        // when
        let s = spread_pct(&xs);
        // then
        assert!((s - 20.0).abs() < 1e-9);
    }
}
