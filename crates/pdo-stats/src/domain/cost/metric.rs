//! The lower-bound bookkeeping of one cost row: dollars, readable and
//! unknown executions, the median's sample, per harness.

use std::collections::{BTreeMap, BTreeSet};

use pdo_run::domain::run_cost::{CostContribution, ModelEffortSlice};
use pdo_run::domain::state::CostForm;

use super::{StatsCostAggregate, StatsHarnessCost};
use crate::domain::distribution::r7_distribution;
use crate::domain::provenance::provenance;

#[derive(Debug, Clone, Default)]
pub struct CostMetricAcc {
    pub usd: f64,
    pub readable_usd: f64,
    pub has_usd: bool,
    pub estimated: bool,
    pub partial: bool,
    pub executions: i64,
    pub readable: i64,
    pub unknown: i64,
    /// One entry per **readable** execution, the raw material of `median_usd`
    /// (#811). Kept alongside `readable_usd` rather than replacing it: the sum
    /// is what an average needs, and a median needs the sample itself. Every
    /// `add_*` below pushes exactly `readable` entries summing to
    /// `readable_usd`, so the two figures always describe the same population.
    pub readable_values: Vec<f64>,
    pub unpriced_models: BTreeSet<String>,
    pub missing_reasons: BTreeSet<String>,
}

impl CostMetricAcc {
    /// Spread one readable slice's dollars over the executions it covers — a
    /// contribution carries a single figure for `n` executions, and only the
    /// per-execution values make a median. Splitting evenly leaves the mean
    /// untouched (`n × usd/n = usd`), so `median_usd` and `average_usd` stay
    /// two readings of one population instead of two populations.
    pub fn record_readable(&mut self, usd: f64, executions: i64) {
        if executions <= 0 {
            return;
        }
        let each = usd / executions as f64;
        self.readable_values
            .extend(std::iter::repeat_n(each, executions as usize));
    }

    /// R-7 median of the readable executions' costs (#811), `None` when no
    /// execution's cost was readable — never `0`, exactly like `average_usd`.
    /// R-7 so Cost and Performance say « median » about the same estimator.
    pub fn median_usd(&self) -> Option<f64> {
        r7_distribution(&self.readable_values).map(|stats| stats.median)
    }

    pub fn add_contribution(&mut self, contribution: &CostContribution) {
        self.executions += contribution.executions;
        self.readable += contribution.readable_executions;
        self.unknown += contribution.executions - contribution.readable_executions;
        if let Some(usd) = contribution.usd {
            self.usd += usd;
            self.has_usd = true;
            if contribution.readable_executions > 0 {
                self.readable_usd += usd;
                self.record_readable(usd, contribution.readable_executions);
            }
        }
        self.estimated |= contribution.form == Some(CostForm::Derived);
        self.partial |= contribution.partial;
        self.unpriced_models
            .extend(contribution.unpriced_models.iter().cloned());
        self.missing_reasons
            .extend(contribution.unavailable_reasons.iter().cloned());
    }

    pub fn add_run(&mut self, contributions: &[&CostContribution]) {
        if contributions.is_empty() {
            return;
        }
        self.executions += 1;
        let mut all_readable = true;
        let mut run_usd = 0.0;
        for contribution in contributions {
            if let Some(usd) = contribution.usd {
                self.usd += usd;
                run_usd += usd;
                self.has_usd = true;
            }
            all_readable &= if contribution.executions > 0 {
                contribution.readable_executions == contribution.executions
            } else {
                contribution.usd.is_some()
            };
            self.estimated |= contribution.form == Some(CostForm::Derived);
            self.partial |= contribution.partial;
            self.unpriced_models
                .extend(contribution.unpriced_models.iter().cloned());
            self.missing_reasons
                .extend(contribution.unavailable_reasons.iter().cloned());
        }
        if all_readable {
            self.readable_usd += run_usd;
            self.readable_values.push(run_usd);
        }
        self.readable += i64::from(all_readable);
        self.unknown += i64::from(!all_readable);
    }

    pub fn wire(&self) -> StatsCostAggregate {
        StatsCostAggregate {
            usd: self.has_usd.then_some(self.usd),
            average_usd: (self.readable > 0).then_some(self.readable_usd / self.readable as f64),
            median_usd: self.median_usd(),
            estimated: self.estimated,
            partial: self.partial,
            executions: self.executions,
            readable: self.readable,
            unknown: self.unknown.max(0),
            unpriced_models: self.unpriced_models.iter().cloned().collect(),
            missing_reasons: self.missing_reasons.iter().cloned().collect(),
            harnesses: Vec::new(),
        }
    }

    pub fn wire_harness(&self, harness: String) -> StatsHarnessCost {
        StatsHarnessCost {
            harness,
            usd: self.has_usd.then_some(self.usd),
            estimated: self.estimated,
            partial: self.partial,
            executions: self.executions,
            readable: self.readable,
            unknown: self.unknown.max(0),
            average_usd: (self.readable > 0).then_some(self.readable_usd / self.readable as f64),
            median_usd: self.median_usd(),
            unpriced_models: self.unpriced_models.iter().cloned().collect(),
            missing_reasons: self.missing_reasons.iter().cloned().collect(),
            provenance: None,
            effort_provenance: None,
            provider: None,
        }
    }

    /// The same lower-bound bookkeeping, for one model × effort slice (ADR-0065).
    /// An execution counts — and costs — in every bucket it provably ran on, so a
    /// bucket's executions may sum to more than the execution count of its
    /// parent aggregate; that double count is the design, not a leak.
    pub fn add_slice(&mut self, slice: &ModelEffortSlice) {
        self.executions += slice.executions;
        let readable = i64::from(slice.usd.is_some()) * slice.executions;
        self.readable += readable;
        self.unknown += slice.executions - readable;
        if let Some(usd) = slice.usd {
            self.usd += usd;
            self.has_usd = true;
            self.readable_usd += usd;
            self.record_readable(usd, slice.executions);
        }
        self.estimated |= slice.estimated;
        self.partial |= slice.partial;
        self.unpriced_models
            .extend(slice.unpriced_models.iter().cloned());
        self.missing_reasons
            .extend(slice.missing_reasons.iter().cloned());
    }
}

#[derive(Debug, Clone, Default)]
pub struct CostAggregateAcc {
    pub total: CostMetricAcc,
    pub harnesses: BTreeMap<String, HarnessMetricAcc>,
}

/// One harness's slice of a row: its dollars plus the provenance counters and
/// providers the wire's harness entry carries on the « By model » rows and the
/// Node pairs (#736). Everything is read off the slices — the run-level adds
/// (`add_run` / `add_contribution`) touch only the metric, so rows without
/// slices wire no provenance at all.
#[derive(Debug, Clone, Default)]
pub struct HarnessMetricAcc {
    pub metric: CostMetricAcc,
    pub model_observed: i64,
    pub model_requested: i64,
    pub effort_observed: i64,
    pub effort_requested: i64,
    pub providers: BTreeSet<String>,
}

impl HarnessMetricAcc {
    pub fn add_slice(&mut self, slice: &ModelEffortSlice) {
        self.metric.add_slice(slice);
        if slice.model_observed {
            self.model_observed += 1;
        } else {
            self.model_requested += 1;
        }
        match slice.effort_observed {
            Some(true) => self.effort_observed += 1,
            Some(false) => self.effort_requested += 1,
            None => {}
        }
        if let Some(provider) = &slice.provider {
            self.providers.insert(provider.clone());
        }
    }

    pub fn wire(self, harness: String) -> StatsHarnessCost {
        let mut wired = self.metric.wire_harness(harness);
        if self.model_observed + self.model_requested > 0 {
            wired.provenance = Some(provenance(self.model_observed, self.model_requested));
        }
        if self.effort_observed + self.effort_requested > 0 {
            wired.effort_provenance = Some(provenance(self.effort_observed, self.effort_requested));
        }
        if !self.providers.is_empty() {
            wired.provider = Some(self.providers.into_iter().collect::<Vec<_>>().join(", "));
        }
        wired
    }
}

impl CostAggregateAcc {
    pub fn add_run(&mut self, contributions: &[CostContribution]) {
        let all: Vec<_> = contributions.iter().collect();
        self.total.add_run(&all);
        let mut names = BTreeSet::new();
        for contribution in contributions {
            if contribution.executions > 0 || contribution.usd.is_some() {
                names.insert(contribution.harness.clone());
            }
        }
        for harness in names {
            let matching: Vec<_> = contributions
                .iter()
                .filter(|contribution| contribution.harness == harness)
                .collect();
            self.harnesses
                .entry(harness)
                .or_default()
                .metric
                .add_run(&matching);
        }
    }

    pub fn add_contribution(&mut self, contribution: &CostContribution) {
        self.total.add_contribution(contribution);
        self.harnesses
            .entry(contribution.harness.clone())
            .or_default()
            .metric
            .add_contribution(contribution);
    }

    pub fn add_slice(&mut self, harness: &str, slice: &ModelEffortSlice) {
        self.total.add_slice(slice);
        self.harnesses
            .entry(harness.to_string())
            .or_default()
            .add_slice(slice);
    }

    pub fn wire(&self) -> StatsCostAggregate {
        let mut aggregate = self.total.wire();
        aggregate.harnesses = self
            .harnesses
            .iter()
            .map(|(harness, metric)| metric.clone().wire(harness.clone()))
            .collect();
        aggregate
    }
}
