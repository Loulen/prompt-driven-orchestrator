//! The settings context over SQLite: the stored tier (`instance_config`), the
//! agentic profiles (`agent_profiles`) and the Apparence of each Actor
//! (`actor_appearances`), behind the context's ports. The
//! free functions stay public for the readers of another tier (who names a
//! skill or a staging profile, the sweep's TTLs in the composition root).

pub mod agent_profiles;
pub mod appearances;
pub mod instance_config;
mod schema;

use std::collections::BTreeMap;

use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::Actor;
use sqlx::SqlitePool;

pub use schema::init;

use crate::application::ports::{
    AgentProfileStore, AppearanceStore, BoxFuture, InstanceConfigStore, StorageError, Stored,
};
use crate::domain::agent_profile::{AgentProfile, ProfileDraft};
use crate::domain::appearance::AppearanceDocument;
use crate::domain::instance_config::{InstanceConfig, UpdateInstanceConfig};

fn stored<T>(result: Result<T, sqlx::Error>) -> Stored<T> {
    result.map_err(|e| StorageError(e.to_string()))
}

/// The stored tier of `pdo.db`.
#[derive(Clone)]
pub struct SqliteInstanceConfig {
    db: SqlitePool,
}

impl SqliteInstanceConfig {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl InstanceConfigStore for SqliteInstanceConfig {
    fn get(&self) -> BoxFuture<'_, Stored<InstanceConfig>> {
        Box::pin(async move { stored(instance_config::get(&self.db).await) })
    }

    fn update(&self, edit: UpdateInstanceConfig) -> BoxFuture<'_, Stored<InstanceConfig>> {
        Box::pin(async move { stored(instance_config::update(&self.db, edit).await) })
    }

    fn retired_sandbox_values(&self) -> BoxFuture<'_, Stored<Vec<(&'static str, String)>>> {
        Box::pin(async move { stored(instance_config::retired_sandbox_values(&self.db).await) })
    }

    fn triggers_paused(&self) -> BoxFuture<'_, Stored<bool>> {
        Box::pin(async move { stored(instance_config::triggers_paused(&self.db).await) })
    }

    fn set_triggers_paused(&self, paused: bool) -> BoxFuture<'_, Stored<()>> {
        Box::pin(
            async move { stored(instance_config::set_triggers_paused(&self.db, paused).await) },
        )
    }
}

/// The agentic profiles of `pdo.db`.
#[derive(Clone)]
pub struct SqliteAgentProfiles {
    db: SqlitePool,
}

impl SqliteAgentProfiles {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl AgentProfileStore for SqliteAgentProfiles {
    fn list(&self) -> BoxFuture<'_, Stored<Vec<AgentProfile>>> {
        Box::pin(async move { stored(agent_profiles::list(&self.db).await) })
    }

    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Option<AgentProfile>>> {
        Box::pin(async move { stored(agent_profiles::get(&self.db, id).await) })
    }

    fn find_by_name<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Stored<Option<AgentProfile>>> {
        Box::pin(async move { stored(agent_profiles::find_by_name(&self.db, name).await) })
    }

    fn create(&self, draft: ProfileDraft) -> BoxFuture<'_, Stored<AgentProfile>> {
        Box::pin(async move { stored(agent_profiles::create(&self.db, draft).await) })
    }

    fn replace<'a>(
        &'a self,
        id: &'a str,
        draft: ProfileDraft,
    ) -> BoxFuture<'a, Stored<Option<AgentProfile>>> {
        Box::pin(async move { stored(agent_profiles::replace(&self.db, id, draft).await) })
    }

    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<bool>> {
        Box::pin(async move { stored(agent_profiles::delete(&self.db, id).await) })
    }

    fn snapshot(&self) -> BoxFuture<'_, Stored<BTreeMap<String, ResolvedCombo>>> {
        Box::pin(async move { stored(agent_profiles::snapshot(&self.db).await) })
    }
}

/// The Apparence of each Actor, in `pdo.db`.
#[derive(Clone)]
pub struct SqliteAppearances {
    db: SqlitePool,
}

impl SqliteAppearances {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl AppearanceStore for SqliteAppearances {
    fn get<'a>(&'a self, actor: &'a Actor) -> BoxFuture<'a, Stored<Option<AppearanceDocument>>> {
        Box::pin(async move { stored(appearances::get(&self.db, actor).await) })
    }

    fn put<'a>(
        &'a self,
        actor: &'a Actor,
        document: &'a AppearanceDocument,
    ) -> BoxFuture<'a, Stored<()>> {
        Box::pin(async move { stored(appearances::put(&self.db, actor, document).await) })
    }
}
