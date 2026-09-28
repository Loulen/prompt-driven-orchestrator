//! The trigger context, assembled: its ports, its knobs, and the state its
//! heartbeat shares with "Run now" (the tick lock, the liveness gauge).

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::event_bus::EventBus;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use pdo_platform::domain::audit::{ActorHint, NewAuditEntry};

use super::ports::{
    AuditTrail, GuardRunner, Repositories, SandboxProfiles, StorageError, TriggerPipelines,
    TriggerRepository, TriggerRuns, TriggerSettings,
};

/// Every port the context needs, as the composition root plugs them in.
pub struct TriggersPorts {
    pub store: Arc<dyn TriggerRepository>,
    pub guards: Arc<dyn GuardRunner>,
    /// The pause and the guard timeout (implemented by the settings context).
    pub settings: Arc<dyn TriggerSettings>,
    pub pipelines: Arc<dyn TriggerPipelines>,
    pub repositories: Arc<dyn Repositories>,
    pub sandbox_profiles: Arc<dyn SandboxProfiles>,
    /// The Runs a fire starts (implemented over the run context).
    pub runs: Arc<dyn TriggerRuns>,
    pub audit: Arc<dyn AuditTrail>,
    /// The kernel's bus: the Trigger announcements `/ws` relays.
    pub bus: Arc<dyn EventBus>,
    /// Asked before every mutation and every fire.
    pub extensions: Arc<dyn ExtensionHost>,
}

/// The daemon's knobs the context reads.
#[derive(Debug, Clone, Default)]
pub struct TriggersOptions {
    /// The daemon's repo root: the repository a legacy Trigger naming none is
    /// grouped under in the list.
    pub repo_root: PathBuf,
    /// Debug-only fault injection: when a due Trigger's name matches, its tick
    /// `panic!`s, exercising the heartbeat's panic isolation.
    pub panic_on_trigger_name: Option<String>,
}

/// The context, assembled: its use cases over its ports.
pub struct Triggers {
    pub(crate) store: Arc<dyn TriggerRepository>,
    pub(crate) guards: Arc<dyn GuardRunner>,
    pub(crate) settings: Arc<dyn TriggerSettings>,
    pub(crate) pipelines: Arc<dyn TriggerPipelines>,
    pub(crate) repositories: Arc<dyn Repositories>,
    pub(crate) sandbox_profiles: Arc<dyn SandboxProfiles>,
    pub(crate) runs: Arc<dyn TriggerRuns>,
    audit: Arc<dyn AuditTrail>,
    bus: Arc<dyn EventBus>,
    extensions: Arc<dyn ExtensionHost>,
    pub(crate) options: TriggersOptions,
    /// Serializes fires. A fire is read-decide-write (`due` → guard subprocess
    /// → `set_next_fire`) with an await in the middle; two concurrent ticks —
    /// or a tick and a "Run now" — would both see the same Trigger as due and
    /// double-fire it.
    pub(crate) tick_lock: tokio::sync::Mutex<()>,
    /// Epoch-millis of the **start** of the most recent tick, or `0` if it has
    /// never ticked. Written at tick start so a wedged tick freezes the value;
    /// an isolated panic still advances it (the loop is alive).
    pub(crate) last_tick_ms: AtomicI64,
}

impl Triggers {
    pub fn new(ports: TriggersPorts, options: TriggersOptions) -> Arc<Self> {
        let TriggersPorts {
            store,
            guards,
            settings,
            pipelines,
            repositories,
            sandbox_profiles,
            runs,
            audit,
            bus,
            extensions,
        } = ports;
        Arc::new(Self {
            store,
            guards,
            settings,
            pipelines,
            repositories,
            sandbox_profiles,
            runs,
            audit,
            bus,
            extensions,
            options,
            tick_lock: tokio::sync::Mutex::new(()),
            last_tick_ms: AtomicI64::new(0),
        })
    }

    /// Create the context's storage if it does not exist yet.
    pub async fn init(&self) -> Result<(), StorageError> {
        self.store.init().await
    }

    /// The Triggers and their history.
    pub fn store(&self) -> &Arc<dyn TriggerRepository> {
        &self.store
    }

    /// Epoch-millis of the start of the last tick, `0` before the first.
    pub fn last_tick_ms(&self) -> i64 {
        self.last_tick_ms.load(Ordering::Relaxed)
    }

    /// Ask the extensions whether `actor` may run `operation`.
    pub(crate) fn authorize(&self, actor: &Actor, operation: &Operation) -> Result<(), Denied> {
        authorize(self.extensions.as_ref(), actor, operation)
    }

    /// Announce on the bus, as `/ws` relays it.
    pub(crate) fn announce(&self, message: serde_json::Value) {
        self.bus.publish(message);
    }

    /// Journal a configuration mutation, AFTER it committed (ADR-0044 §2·B —
    /// never over-report).
    pub(crate) async fn journal(
        &self,
        hint: ActorHint,
        action: &str,
        target: (&str, Option<&str>),
        before: Option<serde_json::Value>,
        after: Option<serde_json::Value>,
    ) {
        self.audit
            .record(NewAuditEntry {
                actor_hint: hint.as_hint().to_string(),
                action: action.to_string(),
                target_kind: Some(target.0.to_string()),
                target_id: target.1.map(str::to_string),
                before,
                after,
            })
            .await;
    }
}
