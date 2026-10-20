//! In-memory stores and recorders for the use-case tests (stage 2).

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::application::ports::*;
use crate::domain::project::{DeletedProject, Project, StoredSetting};
use crate::domain::provisioning::{ProvisioningRules, ProvisioningScope};

use super::ready;

#[derive(Default)]
pub struct InMemoryProjects {
    projects: Mutex<Vec<Project>>,
    deleted: Mutex<Vec<DeletedProject>>,
    next: Mutex<u32>,
}

impl InMemoryProjects {
    pub fn seed(&self, project: Project) {
        self.projects.lock().unwrap().push(project);
    }

    pub fn all(&self) -> Vec<Project> {
        self.projects.lock().unwrap().clone()
    }

    fn with<T>(&self, id: &str, f: impl FnOnce(&mut Project) -> T) -> Option<T> {
        self.projects
            .lock()
            .unwrap()
            .iter_mut()
            .find(|p| p.id == id)
            .map(f)
    }

    fn containing(&self, path: &str) -> Vec<Project> {
        self.all()
            .into_iter()
            .filter(|p| p.has_member(path))
            .collect()
    }
}

impl ProjectRepository for InMemoryProjects {
    fn list(&self) -> BoxFuture<'_, StoreResult<Vec<Project>>> {
        ready(Ok(self.all()))
    }
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<Option<Project>>> {
        ready(Ok(self.all().into_iter().find(|p| p.id == id)))
    }
    fn containing<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Vec<Project>>> {
        ready(Ok(InMemoryProjects::containing(self, path)))
    }
    fn create<'a>(&'a self, name: &'a str) -> BoxFuture<'a, StoreResult<Project>> {
        let mut next = self.next.lock().unwrap();
        *next += 1;
        let project = Project::named(format!("prj-{next}"), name);
        self.seed(project.clone());
        ready(Ok(project))
    }
    fn rename<'a>(&'a self, id: &'a str, name: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        self.with(id, |p| p.name = name.to_string());
        ready(Ok(()))
    }
    fn set_harness<'a>(
        &'a self,
        id: &'a str,
        harness: Option<&'a str>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        self.with(id, |p| p.harness = harness.map(str::to_string));
        ready(Ok(()))
    }
    fn set_auto_fail<'a>(
        &'a self,
        id: &'a str,
        auto_fail: Option<bool>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        self.with(id, |p| p.auto_fail = auto_fail);
        ready(Ok(()))
    }
    fn set_agent_choice<'a>(
        &'a self,
        id: &'a str,
        choice: Option<StoredSetting>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        self.with(id, |p| p.agent_choice = choice);
        ready(Ok(()))
    }
    fn set_skills<'a>(
        &'a self,
        id: &'a str,
        skills: Option<StoredSetting>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        self.with(id, |p| p.skills = skills);
        ready(Ok(()))
    }
    fn set_primary_repo<'a>(
        &'a self,
        id: &'a str,
        repo: Option<&'a str>,
    ) -> BoxFuture<'a, StoreResult<()>> {
        self.with(id, |p| p.primary_repo = repo.map(str::to_string));
        ready(Ok(()))
    }
    fn add_member<'a>(&'a self, id: &'a str, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        self.with(id, |p| p.members.push(path.to_string()));
        ready(Ok(()))
    }
    fn remove_member<'a>(&'a self, id: &'a str, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        self.with(id, |p| p.members.retain(|m| m != path));
        ready(Ok(()))
    }
    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, StoreResult<bool>> {
        let mut projects = self.projects.lock().unwrap();
        let Some(at) = projects.iter().position(|p| p.id == id) else {
            return ready(Ok(false));
        };
        let gone = projects.remove(at);
        self.deleted.lock().unwrap().insert(
            0,
            DeletedProject {
                id: gone.id,
                name: gone.name,
            },
        );
        ready(Ok(true))
    }
    fn deleted(&self) -> BoxFuture<'_, StoreResult<Vec<DeletedProject>>> {
        ready(Ok(self.deleted.lock().unwrap().clone()))
    }
}

#[derive(Default)]
pub struct InMemoryProvisioning {
    rules: Mutex<BTreeMap<(ProvisioningScope, String), ProvisioningRules>>,
}

impl InMemoryProvisioning {
    pub fn put(&self, scope: ProvisioningScope, id: &str, rules: ProvisioningRules) {
        self.rules
            .lock()
            .unwrap()
            .insert((scope, id.to_string()), rules);
    }
}

impl ProvisioningRulesRepository for InMemoryProvisioning {
    fn load<'a>(
        &'a self,
        scope: ProvisioningScope,
        scope_id: &'a str,
    ) -> BoxFuture<'a, StoreResult<ProvisioningRules>> {
        let rules = self.rules.lock().unwrap();
        ready(Ok(rules
            .get(&(scope, scope_id.to_string()))
            .cloned()
            .unwrap_or_default()))
    }
    fn save<'a>(
        &'a self,
        scope: ProvisioningScope,
        scope_id: &'a str,
        rules: &'a ProvisioningRules,
    ) -> BoxFuture<'a, StoreResult<()>> {
        self.put(scope, scope_id, rules.clone());
        ready(Ok(()))
    }
}

#[derive(Default)]
pub struct InMemoryRecent(Mutex<Vec<RecentRepository>>);

impl InMemoryRecent {
    pub fn set(&self, repos: Vec<RecentRepository>) {
        *self.0.lock().unwrap() = repos;
    }
}

impl RecentRepositories for InMemoryRecent {
    fn recent(&self) -> BoxFuture<'_, StoreResult<Vec<RecentRepository>>> {
        ready(Ok(self.0.lock().unwrap().clone()))
    }
}

#[derive(Default)]
pub struct RecordingEvents(Mutex<Vec<String>>);

impl RecordingEvents {
    pub fn changed(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl ProjectEvents for RecordingEvents {
    fn project_changed<'a>(&'a self, project_id: &'a str) -> BoxFuture<'a, ()> {
        self.0.lock().unwrap().push(project_id.to_string());
        ready(())
    }
}

#[derive(Default)]
pub struct RecordingDeletions(Mutex<Vec<String>>);

impl RecordingDeletions {
    pub fn deleted(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl ProjectDeletions for RecordingDeletions {
    fn project_deleted<'a>(&'a self, project_id: &'a str) -> BoxFuture<'a, ()> {
        self.0.lock().unwrap().push(project_id.to_string());
        ready(())
    }
}
