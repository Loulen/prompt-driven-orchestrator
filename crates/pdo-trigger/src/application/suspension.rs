//! The suspension of a Trigger by an extension (API 0.8): asked at every read
//! and before every fire, never stored, so it lifts as soon as the extension
//! stops suspending.

use pdo_extension_api::TriggerRef;

use crate::domain::trigger::{Suspension, Trigger};
use crate::Triggers;

impl Triggers {
    /// Why an extension suspends `trigger`, if one does.
    pub fn suspension_of(&self, trigger: &Trigger) -> Option<Suspension> {
        let mut target = TriggerRef::new(&trigger.id, &trigger.name, trigger.last_editor.clone());
        // 0.26: judged in its Projet, like every Operation on it.
        if let Some(project) = &trigger.project_id {
            target = target.in_project(project);
        }
        self.extensions()
            .trigger_suspension(&target)
            .map(|s| Suspension {
                extension: s.extension,
                reason: s.reason,
            })
    }

    /// `trigger`, carrying its suspension as the API returns it.
    pub(crate) fn with_suspension(&self, mut trigger: Trigger) -> Trigger {
        trigger.suspension = self.suspension_of(&trigger);
        trigger
    }
}
