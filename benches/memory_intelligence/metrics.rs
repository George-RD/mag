//! Scoring primitives shared by the task families.
//!
//! Every function here is pure. The conventions are stated once so the family
//! scorers do not each invent their own edge-case handling:
//!
//! - `precision` is 1.0 when nothing was predicted, `recall` is 1.0 when nothing
//!   was expected, and `f1` is 0.0 when precision and recall are both 0.0.
//! - Latency samples are microseconds; the percentile helpers return
//!   milliseconds.

use std::collections::BTreeSet;

/// Widens a count to `f64`. Counts in this harness are dataset-sized, far below
/// 2^53, so the conversion is exact.
pub fn count(value: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f64
    }
}

/// Divides two counts, returning 0.0 for a zero denominator.
pub fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    count(numerator) / count(denominator)
}

/// Precision, recall and F1 as fractions in `[0.0, 1.0]`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Prf {
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

/// Confusion counts accumulated across cases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
}

impl Counts {
    pub fn add(&mut self, other: Self) {
        self.true_positives += other.true_positives;
        self.false_positives += other.false_positives;
        self.false_negatives += other.false_negatives;
    }

    pub fn prf(self) -> Prf {
        let precision = if self.true_positives + self.false_positives == 0 {
            1.0
        } else {
            ratio(
                self.true_positives,
                self.true_positives + self.false_positives,
            )
        };
        let recall = if self.true_positives + self.false_negatives == 0 {
            1.0
        } else {
            ratio(
                self.true_positives,
                self.true_positives + self.false_negatives,
            )
        };
        let f1 = if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        };
        Prf {
            precision,
            recall,
            f1,
        }
    }
}

/// Confusion counts for one predicted set against one expected set.
pub fn compare_sets(predicted: &BTreeSet<String>, expected: &BTreeSet<String>) -> Counts {
    Counts {
        true_positives: predicted.intersection(expected).count(),
        false_positives: predicted.difference(expected).count(),
        false_negatives: expected.difference(predicted).count(),
    }
}

/// Fraction of `relevant` keys appearing in the first `k` entries of `ranked`.
/// Returns 1.0 when nothing is relevant.
pub fn recall_at_k(ranked: &[String], relevant: &BTreeSet<String>, k: usize) -> f64 {
    if relevant.is_empty() {
        return 1.0;
    }
    let head: BTreeSet<&String> = ranked.iter().take(k).collect();
    let hits = relevant.iter().filter(|key| head.contains(key)).count();
    ratio(hits, relevant.len())
}

/// Reciprocal rank of the first relevant entry, or 0.0 when none is retrieved.
pub fn reciprocal_rank(ranked: &[String], relevant: &BTreeSet<String>) -> f64 {
    for (index, key) in ranked.iter().enumerate() {
        if relevant.contains(key) {
            return ratio(1, index + 1);
        }
    }
    0.0
}

/// Arithmetic mean, 0.0 for an empty slice.
pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / count(values.len())
}

/// Weighted cluster purity: for each predicted cluster, the size of its largest
/// overlap with a gold cluster, summed and divided by the number of clustered
/// items. Returns 1.0 when there is nothing to cluster.
pub fn cluster_purity(predicted: &[BTreeSet<String>], gold: &[BTreeSet<String>]) -> f64 {
    let total: usize = predicted.iter().map(BTreeSet::len).sum();
    if total == 0 {
        return 1.0;
    }
    let matched: usize = predicted
        .iter()
        .map(|cluster| {
            gold.iter()
                .map(|reference| cluster.intersection(reference).count())
                .max()
                .unwrap_or(0)
        })
        .sum();
    ratio(matched, total)
}

/// Fraction of gold clusters recovered whole inside a single predicted cluster.
/// Returns 1.0 when there are no gold clusters.
pub fn cluster_coverage(predicted: &[BTreeSet<String>], gold: &[BTreeSet<String>]) -> f64 {
    if gold.is_empty() {
        return 1.0;
    }
    let recovered = gold
        .iter()
        .filter(|reference| {
            predicted
                .iter()
                .any(|cluster| reference.is_subset(cluster) && cluster.len() == reference.len())
        })
        .count();
    ratio(recovered, gold.len())
}

/// Nearest-rank percentile over microsecond samples, returned in milliseconds.
pub fn percentile_of_micros(samples: &[u128], percentile: f64) -> f64 {
    crate::bench_utils::stats::percentile_ms(samples, percentile) / 1000.0
}

/// One microsecond duration in milliseconds. Harness durations are seconds at
/// most, so the widening is exact.
pub fn micros_to_ms(micros: u128) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        micros as f64 / 1000.0
    }
}

/// Builds a `BTreeSet` from anything iterable of strings.
pub fn set_of<I, S>(items: I) -> BTreeSet<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    items.into_iter().map(Into::into).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn perfect_prediction_scores_one() {
        let counts = compare_sets(&set(&["a", "b"]), &set(&["a", "b"]));
        let prf = counts.prf();
        assert_eq!(counts.true_positives, 2);
        assert!((prf.precision - 1.0).abs() < 1e-12);
        assert!((prf.recall - 1.0).abs() < 1e-12);
        assert!((prf.f1 - 1.0).abs() < 1e-12);
    }

    #[test]
    fn half_correct_prediction() {
        // Predicted {a, c} against expected {a, b}: one hit, one miss, one spurious.
        let prf = compare_sets(&set(&["a", "c"]), &set(&["a", "b"])).prf();
        assert!((prf.precision - 0.5).abs() < 1e-12);
        assert!((prf.recall - 0.5).abs() < 1e-12);
        assert!((prf.f1 - 0.5).abs() < 1e-12);
    }

    #[test]
    fn empty_prediction_and_empty_expectation_scores_one() {
        let prf = compare_sets(&set(&[]), &set(&[])).prf();
        assert!((prf.f1 - 1.0).abs() < 1e-12);
    }

    #[test]
    fn spurious_prediction_against_no_expectation_scores_zero_precision() {
        let prf = compare_sets(&set(&["a"]), &set(&[])).prf();
        assert!(prf.precision.abs() < 1e-12);
        assert!((prf.recall - 1.0).abs() < 1e-12);
        assert!(prf.f1.abs() < 1e-12);
    }

    #[test]
    fn missed_expectation_scores_zero_recall() {
        let prf = compare_sets(&set(&[]), &set(&["a"])).prf();
        assert!((prf.precision - 1.0).abs() < 1e-12);
        assert!(prf.recall.abs() < 1e-12);
        assert!(prf.f1.abs() < 1e-12);
    }

    #[test]
    fn counts_accumulate() {
        let mut total = Counts::default();
        total.add(compare_sets(&set(&["a"]), &set(&["a"])));
        total.add(compare_sets(&set(&["b"]), &set(&["c"])));
        assert_eq!(
            total,
            Counts {
                true_positives: 1,
                false_positives: 1,
                false_negatives: 1
            }
        );
    }

    #[test]
    fn recall_at_k_respects_the_cutoff() {
        let ranked = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        assert!((recall_at_k(&ranked, &set(&["z"]), 3) - 1.0).abs() < 1e-12);
        assert!(recall_at_k(&ranked, &set(&["z"]), 2).abs() < 1e-12);
        assert!((recall_at_k(&ranked, &set(&["x", "z"]), 2) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn recall_at_k_with_no_relevant_items_is_one() {
        assert!((recall_at_k(&[], &set(&[]), 5) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn reciprocal_rank_uses_the_first_hit() {
        let ranked = vec!["x".to_string(), "y".to_string()];
        assert!((reciprocal_rank(&ranked, &set(&["y"])) - 0.5).abs() < 1e-12);
        assert!((reciprocal_rank(&ranked, &set(&["x"])) - 1.0).abs() < 1e-12);
        assert!(reciprocal_rank(&ranked, &set(&["q"])).abs() < 1e-12);
    }

    #[test]
    fn cluster_purity_on_a_hand_worked_fixture() {
        // Predicted {a,b,c} and {d}; gold {a,b} and {c,d}.
        // Largest overlaps are 2 and 1, over 4 clustered items.
        let predicted = vec![set(&["a", "b", "c"]), set(&["d"])];
        let gold = vec![set(&["a", "b"]), set(&["c", "d"])];
        assert!((cluster_purity(&predicted, &gold) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn cluster_purity_is_one_for_a_perfect_partition() {
        let clusters = vec![set(&["a", "b"]), set(&["c"])];
        assert!((cluster_purity(&clusters, &clusters) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cluster_purity_of_nothing_is_one() {
        assert!((cluster_purity(&[], &[]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cluster_coverage_counts_exactly_recovered_clusters() {
        let predicted = vec![set(&["a", "b"]), set(&["c"]), set(&["d"])];
        let gold = vec![set(&["a", "b"]), set(&["c", "d"])];
        assert!((cluster_coverage(&predicted, &gold) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn cluster_coverage_credits_a_singleton_gold_cluster() {
        // A one-member gold cluster is satisfied by the memory existing
        // un-clustered. The grouping family therefore keeps singletons out of
        // the coverage denominator rather than expecting this function to
        // discount them.
        let predicted = vec![set(&["a"]), set(&["b"])];
        assert!((cluster_coverage(&predicted, &[set(&["a"])]) - 1.0).abs() < 1e-12);
        assert!(cluster_coverage(&predicted, &[set(&["a", "b"])]).abs() < 1e-12);
    }

    #[test]
    fn micros_convert_to_milliseconds() {
        assert!((micros_to_ms(1_500) - 1.5).abs() < 1e-12);
        assert!(micros_to_ms(0).abs() < 1e-12);
    }

    #[test]
    fn mean_of_empty_is_zero() {
        assert!(mean(&[]).abs() < 1e-12);
        assert!((mean(&[1.0, 2.0]) - 1.5).abs() < 1e-12);
    }

    #[test]
    fn percentile_of_micros_converts_to_milliseconds() {
        let samples = vec![1_000u128, 2_000, 3_000];
        assert!((percentile_of_micros(&samples, 50.0) - 2.0).abs() < 1e-12);
        assert!(percentile_of_micros(&[], 95.0).abs() < 1e-12);
    }
}
