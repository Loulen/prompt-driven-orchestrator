//! R-7 six-number distribution summary (#585, Stats → Performance).
//!
//! Every Performance boxplot cell is backed by the same six statistics — mean,
//! median, Q1, Q3, min, max — computed once here rather than once per caller
//! (Node, Pipeline, Infrastructure role, subagent group all fold through this).
//! Quartiles use **R-7** (linear interpolation between closest ranks — R's
//! default `quantile()` type, and Excel/NumPy's default), per the issue's
//! Implementation Decisions: "Les quartiles utilisent l'interpolation linéaire
//! R-7." A one-value sample is valid and yields six identical statistics (the
//! issue's explicit acceptance criterion), not a degenerate `None`.
//!
//! Pure, allocation-light, no I/O: `&[f64]` in, `Option<SixStats>` out. `None`
//! only for an empty sample — the caller (never this module) turns "no readable
//! values" into a coverage/absence-reasons pair; a distribution itself does not
//! know why a value is missing, only how many there were.

/// The six statistics one boxplot cell renders: the box (Q1–Q3), the median
/// line, the mean dot, and the whiskers (min/max).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub(crate) struct SixStats {
    pub mean: f64,
    pub median: f64,
    pub q1: f64,
    pub q3: f64,
    pub min: f64,
    pub max: f64,
}

/// The R-7 quantile of `sorted` (already sorted ascending, non-empty) at
/// probability `p` ∈ [0, 1]: linear interpolation between the two closest ranks.
/// `h = (n - 1) * p` is the fractional rank; the integer part indexes the lower
/// value, the fractional part interpolates toward the next one. A single-value
/// sample has `n - 1 == 0`, so `h` is always `0` and every quantile collapses to
/// that one value — the "valid boxplot from one observation" acceptance
/// criterion falls out of the formula, no special case needed.
fn r7_quantile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let h = (n - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = h - lo as f64;
    sorted[lo] + frac * (sorted[hi] - sorted[lo])
}

/// Fold `values` (unordered, any finite `f64`) into the six R-7 statistics, or
/// `None` if `values` is empty. NaN/infinite inputs are the caller's bug (a
/// parsed token count is always a non-negative finite integer promoted to
/// `f64`) — this function does not defend against them beyond the sort not
/// panicking (`f64::total_cmp`).
pub(crate) fn r7_distribution(values: &[f64]) -> Option<SixStats> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    Some(SixStats {
        mean,
        median: r7_quantile(&sorted, 0.5),
        q1: r7_quantile(&sorted, 0.25),
        q3: r7_quantile(&sorted, 0.75),
        min: sorted[0],
        max: *sorted.last().expect("non-empty"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn r7_on_an_asymmetric_odd_sample() {
        // Values chosen to make mean, median and quartiles all distinct, so a
        // test that swaps two of them fails loudly.
        let values = [2.0, 5.0, 5.0, 9.0, 100.0, 3.0, 7.0];
        let stats = r7_distribution(&values).unwrap();
        assert!(close(stats.mean, 18.714_285_714_285_715), "{stats:?}");
        assert!(close(stats.median, 5.0), "{stats:?}");
        assert!(close(stats.q1, 4.0), "{stats:?}");
        assert!(close(stats.q3, 8.0), "{stats:?}");
        assert_eq!(stats.min, 2.0);
        assert_eq!(stats.max, 100.0);
    }

    #[test]
    fn r7_on_an_even_sample_interpolates_between_the_two_middle_ranks() {
        let values = [10.0, 20.0, 30.0, 40.0];
        let stats = r7_distribution(&values).unwrap();
        assert!(close(stats.q1, 17.5), "{stats:?}");
        assert!(close(stats.median, 25.0), "{stats:?}");
        assert!(close(stats.q3, 32.5), "{stats:?}");
        assert_eq!(stats.min, 10.0);
        assert_eq!(stats.max, 40.0);
        assert!(close(stats.mean, 25.0));
    }

    #[test]
    fn r7_is_order_independent() {
        let ordered = [1.0, 2.0, 3.0, 4.0, 5.0];
        let shuffled = [4.0, 1.0, 5.0, 3.0, 2.0];
        assert_eq!(r7_distribution(&ordered), r7_distribution(&shuffled));
    }

    #[test]
    fn a_single_value_sample_is_a_valid_boxplot_of_six_identical_values() {
        // #585 AC: "Un échantillon d'une valeur produit un boxplot valide" — all
        // six statistics equal that one value, not a `None`/degenerate cell.
        let stats = r7_distribution(&[42.0]).unwrap();
        assert_eq!(
            stats,
            SixStats {
                mean: 42.0,
                median: 42.0,
                q1: 42.0,
                q3: 42.0,
                min: 42.0,
                max: 42.0,
            }
        );
    }

    #[test]
    fn an_empty_sample_has_no_distribution() {
        assert_eq!(r7_distribution(&[]), None);
    }

    #[test]
    fn two_value_sample_interpolates_quartiles_toward_the_endpoints() {
        let stats = r7_distribution(&[0.0, 10.0]).unwrap();
        // n=2: h(0.25) = 0.25, h(0.75) = 0.75 — quartiles sit inside the pair.
        assert!(close(stats.q1, 2.5), "{stats:?}");
        assert!(close(stats.median, 5.0), "{stats:?}");
        assert!(close(stats.q3, 7.5), "{stats:?}");
    }
}
