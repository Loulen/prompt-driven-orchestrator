//! The Pipeline Manager's sessions, observed through the session context's
//! tmux: one `has-session` per read.

use std::sync::Arc;

use pdo_session::application::ports::Tmux;
use pdo_session::domain::names::manager_session_name;

use crate::application::ports::ManagerSessions;

pub struct TmuxManagerSessions(pub Arc<dyn Tmux>);

impl ManagerSessions for TmuxManagerSessions {
    fn exists(&self, run_id: &str) -> bool {
        self.0.session_exists(&manager_session_name(run_id))
    }
}
