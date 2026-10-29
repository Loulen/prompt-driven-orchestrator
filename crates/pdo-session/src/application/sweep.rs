//! The orphan sweep (reaper, ADR-0038): kill the `pdo-*` sessions whose owner
//! is archived, absent, completed past its TTL, or — for the library assistant —
//! that nobody uses any more. Runs at boot and every reaper interval; an
//! internal transition, never authorized.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::application::assistant::FocusCell;
use crate::application::hosted::HostedRegistry;
use crate::application::ports::{SessionOwners, Tmux};
use crate::domain::names::{parse_session_name, ParsedSession};
use crate::domain::reaper::{
    decide_sweep, SweepDecision, SweepInput, SweepTally, SweepTtls, SweepVerdict,
};

/// The reaper's since-boot counters, for the `reaper` gauge of `GET /sessions`.
///
/// They ACCUMULATE: a per-pass gauge answers "did the *last* pass kill?", which is
/// ~always "no" — the killing pass is followed within seconds by an idle one that
/// would overwrite the count with 0. Cumulative also makes the tally independent
/// of *which* pass killed.
#[derive(Debug, Default)]
pub struct ReaperGauge {
    last_sweep_ms: AtomicI64,
    killed: AtomicI64,
    killed_for_absent_run: AtomicI64,
}

impl ReaperGauge {
    /// When the last pass ran (epoch ms, `0` before the first), the kills, and the
    /// kills that were absence verdicts.
    pub fn read(&self) -> (i64, i64, i64) {
        let ord = Ordering::Relaxed;
        (
            self.last_sweep_ms.load(ord),
            self.killed.load(ord),
            self.killed_for_absent_run.load(ord),
        )
    }

    fn record(&self, now_ms: i64, tally: SweepTally) {
        let ord = Ordering::Relaxed;
        self.last_sweep_ms.store(now_ms, ord);
        self.killed.fetch_add(tally.killed, ord);
        self.killed_for_absent_run
            .fetch_add(tally.killed_for_absent_run, ord);
    }
}

// One orphan-sweep pass: inventory tmux, resolve owners, decide, kill.
//
// **THE ORDER OF THE FIRST TWO STEPS IS LOAD-BEARING (ADR-0038).** The sweep's
// "absent" verdict is never true on its own — it is true *relative to two
// observations*, the tmux inventory and the event-log read. Snapshot-then-list
// killed a session born in between: live in tmux, missing from a snapshot that
// predated it, so its owner read as absent → killed, in one case 150 ms after
// its own spawn.
//
// The correct order is sound because **the log only grows**: an absence
// observed *after* the inventory implies absence *at* the inventory, so a
// session missing from the log we read here cannot have existed when we listed
// — it is simply not in `names`, and it gets judged on the next pass with its
// reservation visible. Monotonicity works in this direction only; the reverse
// order has no symmetric proof. Do not "tidy" this into snapshot-then-list.
//
// The window is not one instruction: it is one storage read per run, so it **grows
// with the age of the instance**. The boot-time caller never sees it (it runs
// before `build_router`, so no spawn is concurrent); only the periodic reaper races
// live spawns.
//
// Finally, this function's correctness rests on a precondition it cannot enforce —
// **no tmux session exists before the event that reserves it is durably appended**.
// `node_spawn::spawn_node`, `node_primitives::start_node`'s callers and
// `spawn_manager_session` all own that half; the reaper is only its consumer.
pub struct SweepOrphans {
    tmux: Arc<dyn Tmux>,
    owners: Arc<dyn SessionOwners>,
    focus: Arc<FocusCell>,
    gauge: Arc<ReaperGauge>,
    hosted: Option<Arc<HostedRegistry>>,
}

impl SweepOrphans {
    pub fn new(
        tmux: Arc<dyn Tmux>,
        owners: Arc<dyn SessionOwners>,
        focus: Arc<FocusCell>,
        gauge: Arc<ReaperGauge>,
    ) -> Self {
        Self {
            tmux,
            owners,
            focus,
            gauge,
            hosted: None,
        }
    }

    /// The same sweep, keeping the Sessions hébergées `hosted` records live
    /// (ADR-0091). Without it, every name with their prefix is killed.
    pub fn with_hosted(mut self, hosted: Arc<HostedRegistry>) -> Self {
        self.hosted = Some(hosted);
        self
    }

    /// One pass, on the TTLs read fresh from the settings: a `PUT /settings`
    /// takes effect on the next sweep without a restart (ADR-0015). `Err` when the
    /// owners could not be read: nothing was killed.
    pub async fn run(&self) -> Result<SweepTally, String> {
        let ttls = self.owners.ttls().await;
        self.run_with(ttls).await
    }

    /// One pass on explicit TTLs.
    pub async fn run_with(&self, ttls: SweepTtls) -> Result<SweepTally, String> {
        // (1) EDGE FIRST — the tmux inventory. Sorted so kill order and log order
        //     are deterministic.
        let mut names: Vec<String> = self.tmux.list_pdo_sessions().into_iter().collect();
        names.sort();

        // (2) ONE clock for the whole pass, taken right after the inventory and
        //     before any read, so the TTLs are not "a bit more aggressive because
        //     the reads took 4 s". The assistant's presence facts are read against
        //     the same `now`.
        let now = chrono::Utc::now();

        // (3) EDGE SECOND — the owners of the sessions that actually exist, read
        //     AFTER the inventory (the log only grows, so an absence seen now was an
        //     absence at the inventory). An error aborts the pass: a storage timeout
        //     must never read a live run as "absent" and kill its session.
        let mut inputs: Vec<SweepInput> = Vec::with_capacity(names.len());
        let mut owners: Vec<(String, String)> = Vec::new();
        let mut owner_of: HashMap<usize, usize> = HashMap::new();
        for (index, session_name) in names.into_iter().enumerate() {
            let parsed = parse_session_name(&session_name);
            let mut attached = false;
            let mut focus_age = None;
            let mut recorded = false;
            match &parsed {
                None => {}
                // A Session hébergée lives by its record alone.
                Some(ParsedSession::Hosted) => {
                    recorded = self
                        .hosted
                        .as_ref()
                        .is_some_and(|hosted| hosted.holds_live(&session_name));
                }
                // A library assistant owns no Run: it is judged on the presence
                // of its Actor (its own focus).
                Some(ParsedSession::LibAssist) => {
                    attached = self.tmux.is_attached(&session_name);
                    focus_age = self.focus.get(&session_name).map(|f| f.at).map(|at| {
                        now.signed_duration_since(at)
                            .to_std()
                            .unwrap_or(Duration::ZERO)
                    });
                }
                Some(ParsedSession::Manager { run_id }) => {
                    owner_of.insert(index, owners.len());
                    owners.push((run_id.clone(), "__manager__".into()));
                }
                Some(ParsedSession::Shell { run_id }) => {
                    owner_of.insert(index, owners.len());
                    owners.push((run_id.clone(), "__shell__".into()));
                }
                Some(ParsedSession::NodeRun {
                    run_id, node_id, ..
                }) => {
                    owner_of.insert(index, owners.len());
                    owners.push((run_id.clone(), node_id.clone()));
                }
            }
            inputs.push(SweepInput {
                session_name,
                parsed,
                info: None,
                attached,
                focus_age,
                recorded,
            });
        }
        let facts = self.owners.owners(&owners).await?;
        for (index, input) in inputs.iter_mut().enumerate() {
            if let Some(&owner) = owner_of.get(&index) {
                input.info = facts.get(owner).cloned().flatten();
            }
        }

        // (4) PURE — no tmux, no storage, no clock inside.
        let decisions = decide_sweep(&inputs, ttls, now);

        // (5) EDGE — log + kill. Nothing is killed before this point.
        let tally = self.apply(&decisions);
        self.gauge.record(now.timestamp_millis(), tally);
        Ok(tally)
    }

    /// Execute the decisions: log the reason verbatim, then kill the session.
    ///
    /// Log levels split routine housekeeping from "can no longer happen" (#485):
    /// *archived* / *stale* stay `info!` (nominal, every 60 s — promoting them
    /// would train the operator to ignore a permanent warning stream), while
    /// *absent* / *unrecognised* are `warn!` so `journalctl -p warning` surfaces
    /// them without a full-log grep.
    fn apply(&self, decisions: &[SweepDecision]) -> SweepTally {
        let mut tally = SweepTally::default();
        for decision in decisions {
            let reason = match &decision.verdict {
                SweepVerdict::Keep => continue,
                SweepVerdict::Kill(reason) => reason,
            };
            if reason.is_absence() {
                warn!("{reason}");
                tally.killed_for_absent_run += 1;
            } else {
                info!("{reason}");
            }
            tally.killed += 1;
            self.tmux.kill(&decision.session_name);
        }
        tally
    }
}
