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
//!
//! ## Tukey fences (#811, story #808)
//!
//! The « Fenced » zoom level of a box-plot draws its whiskers at the **Tukey
//! fences** rather than at min/max: the smallest observation still inside
//! `[Q1 − 1.5 IQR, Q3 + 1.5 IQR]` and the largest one. They are computed
//! **here**, on the observations, because the observations never leave the
//! daemon (CONTEXT.md, « Niveau de zoom d'un box-plot »: « Aucun point
//! individuel : le daemon ne renvoie jamais les observations ») — the frontend
//! could not derive them from the six numbers alone. A fence is always an
//! actual observation, never the theoretical bound: that is what makes the
//! whisker a real value the tooltip can name.

/// The statistics one boxplot cell renders: the box (Q1–Q3), the median line,
/// the mean (tooltip only since #811), the min/max whiskers of the « Full »
/// zoom level and the Tukey fences of the « Fenced » one.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub(crate) struct SixStats {
    pub mean: f64,
    pub median: f64,
    pub q1: f64,
    pub q3: f64,
    pub min: f64,
    pub max: f64,
    /// Smallest observation ≥ `q1 - 1.5 * (q3 - q1)` (#811). Equals `min` when
    /// nothing lies below the bound, `q1` when the IQR is zero, and the single
    /// value of a one-observation sample.
    pub fence_low: f64,
    /// Largest observation ≤ `q3 + 1.5 * (q3 - q1)` (#811). Mirror of
    /// [`Self::fence_low`].
    pub fence_high: f64,
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
    let q1 = r7_quantile(&sorted, 0.25);
    let q3 = r7_quantile(&sorted, 0.75);
    let (fence_low, fence_high) = tukey_fences(&sorted, q1, q3);
    Some(SixStats {
        mean,
        median: r7_quantile(&sorted, 0.5),
        q1,
        q3,
        min: sorted[0],
        max: *sorted.last().expect("non-empty"),
        fence_low,
        fence_high,
    })
}

/// The pair of **observed** Tukey fences of a sorted, non-empty sample: the
/// smallest value ≥ `q1 - 1.5 IQR` and the largest ≤ `q3 + 1.5 IQR`.
///
/// Both searches always succeed — `q1` itself is ≥ the lower bound and `q3` ≤
/// the upper one, and an R-7 quartile always sits inside `[min, max]` — so the
/// fallbacks below never fire in practice; they keep the function total rather
/// than panicking on an impossible sample. A zero IQR collapses the bounds onto
/// the quartiles, which is exactly the acceptance criterion ("IQR nul donne des
/// bornes égales à Q1 et Q3"), and a one-value sample collapses everything onto
/// that value.
fn tukey_fences(sorted: &[f64], q1: f64, q3: f64) -> (f64, f64) {
    let whisker = 1.5 * (q3 - q1);
    let low_bound = q1 - whisker;
    let high_bound = q3 + whisker;
    let fence_low = sorted
        .iter()
        .copied()
        .find(|value| *value >= low_bound)
        .unwrap_or(q1);
    let fence_high = sorted
        .iter()
        .rev()
        .copied()
        .find(|value| *value <= high_bound)
        .unwrap_or(q3);
    (fence_low, fence_high)
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
                fence_low: 42.0,
                fence_high: 42.0,
            }
        );
    }

    #[test]
    fn tukey_fences_fall_back_on_min_and_max_when_no_observation_is_an_outlier() {
        // A tight sample: every value sits inside [Q1 − 1.5 IQR, Q3 + 1.5 IQR],
        // so the Fenced level draws exactly the same whiskers as Full.
        let stats = r7_distribution(&[10.0, 20.0, 30.0, 40.0, 50.0]).unwrap();
        assert!(close(stats.q1, 20.0), "{stats:?}");
        assert!(close(stats.q3, 40.0), "{stats:?}");
        assert_eq!(stats.fence_low, stats.min);
        assert_eq!(stats.fence_high, stats.max);
    }

    #[test]
    fn tukey_fences_cut_an_outlier_on_one_side_only() {
        // The story's very shape (#808): comparable runs and one much longer.
        // Q1 = 22.5, Q3 = 47.5, IQR = 25 → bounds [-15, 85]; only 500 is out,
        // and the low fence stays on the untouched min.
        let stats = r7_distribution(&[10.0, 20.0, 30.0, 40.0, 50.0, 500.0]).unwrap();
        assert_eq!(stats.max, 500.0);
        assert_eq!(stats.fence_low, 10.0, "{stats:?}");
        assert_eq!(stats.fence_high, 50.0, "{stats:?}");

        // Mirror image: the outlier below, the high fence untouched.
        let stats = r7_distribution(&[-500.0, 10.0, 20.0, 30.0, 40.0, 50.0]).unwrap();
        assert_eq!(stats.min, -500.0);
        assert_eq!(stats.fence_low, 10.0, "{stats:?}");
        assert_eq!(stats.fence_high, 50.0, "{stats:?}");
    }

    #[test]
    fn a_zero_iqr_puts_both_fences_on_the_quartiles() {
        // #811 AC: "IQR nul donne des bornes égales à Q1 et Q3" — with no
        // spread between the quartiles the bounds are the quartiles themselves,
        // so both extremes fall outside and the Fenced level collapses to the
        // median line.
        let stats = r7_distribution(&[1.0, 5.0, 5.0, 5.0, 5.0, 5.0, 9.0]).unwrap();
        assert!(close(stats.q1, 5.0), "{stats:?}");
        assert!(close(stats.q3, 5.0), "{stats:?}");
        assert_eq!(stats.fence_low, stats.q1, "{stats:?}");
        assert_eq!(stats.fence_high, stats.q3, "{stats:?}");
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.max, 9.0);
    }

    #[test]
    fn a_fence_is_always_an_observation_never_the_theoretical_bound() {
        // Q1 = 2, Q3 = 4, IQR = 2 → bounds [-1, 7]; the fences land on 1 and 5,
        // the observations, not on -1 and 7.
        let stats = r7_distribution(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!(stats.fence_low, 1.0, "{stats:?}");
        assert_eq!(stats.fence_high, 5.0, "{stats:?}");
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
