//! Open the Agent de support of an Actor.

use pdo_extension_api::{SessionKind, SessionLaunch, SessionRef};
use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_kernel::authorization::authorize;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::{Support, SupportError};
use crate::domain::launch::{build_support_script, SupportTail};
use crate::domain::names::support_session_name_of;
use crate::domain::support::{
    first_message, question_text, support_dir, support_instructions_import_path,
    support_instructions_path, support_question_path, SupportAgent, SupportRecord,
    SUPERSEDED_PRIMER, SUPPORT_AGENT_PROMPT, SUPPORT_INSTRUCTIONS_IMPORT,
};

/// What opening answers: the session, and the agent it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportOpened {
    pub session: String,
    pub agent: SupportAgent,
}

impl Support {
    /// Open the Agent de support of `actor` on `choice`, `question` as its
    /// first message: the Operation `session` × `create` on its session. One
    /// per Actor: with one open already, refused, unless `replace`, which
    /// closes it once the new choice resolved. The choice resolves to an installed harness or is
    /// refused with its reason. Its folder is prepared (the skills PDO
    /// manages; its instructions, what the extensions add, the embedded
    /// prompt, then what this instance gives it, in the files its harness
    /// loads at start; the question, its first message), its launch hooks
    /// asked with `actor` as who starts it (their variables, its token among
    /// them, go into its environment), and the choice is remembered as the
    /// Actor's last.
    pub async fn open(
        &self,
        actor: &Actor,
        choice: &AgentChoice,
        question: Option<&str>,
        replace: bool,
    ) -> Result<SupportOpened, SupportError> {
        let session = support_session_name_of(actor);
        let operation = Operation::new(ResourceKind::Session, Verb::Create).on(session.clone());
        authorize(self.host.as_ref(), actor, &operation).map_err(SupportError::Refused)?;
        let mut record = self.record(&session);
        let open = self.tmux.session_exists(&session);
        if open && !replace {
            return Err(SupportError::AlreadyOpen(record.agent.map(Box::new)));
        }
        // Resolved before the open one is closed: a choice refused closes nothing.
        let resolved = self
            .agents
            .resolve(choice)
            .await
            .map_err(SupportError::Choice)?;
        if open {
            self.tmux.kill(&session);
            info!("Closed support agent {session} to open another");
        }

        let dir = support_dir(&self.launch.root, &session);
        std::fs::create_dir_all(&dir).map_err(|e| {
            SupportError::Folder(format!("failed to create {}: {e}", dir.display()))
        })?;
        let session_ref = SessionRef::new(&session, SessionKind::SupportAgent);
        let contribution = self
            .host
            .session_launch(&SessionLaunch::new(session_ref.clone()).started_by(actor.clone()));
        let instructions = format!(
            "{SUPPORT_AGENT_PROMPT}\n{}",
            self.instance_section(&dir).await?
        );
        let first_message = write_folder(&dir, &contribution.above(instructions), question)?;
        let env: Vec<(String, String)> = contribution.env.into_iter().collect();
        let script = build_support_script(
            &session,
            self.launch.daemon_port,
            &self.tmux.session_path(),
            SupportTail {
                harness: &resolved.harness,
                first_message: first_message.as_deref(),
                model: resolved.model.as_deref(),
                effort: resolved.effort.as_deref(),
            },
            self.launch.tmux_cmd_override.as_deref(),
            &env,
        );
        self.tmux
            .new_session(&session, &dir, &script, Some("support"))
            .map_err(|e| SupportError::Spawn(format!("failed to open the support agent: {e}")))?;
        info!(
            "Opened support agent {session} on {} in {}",
            resolved.harness.name,
            dir.display()
        );
        self.ledger.started(session_ref, Some(actor.clone()));

        let agent = SupportAgent {
            choice: choice.clone(),
            label: resolved.label,
            harness: resolved.harness.name.clone(),
            model: resolved.model,
            effort: resolved.effort,
            opened_at: pdo_kernel::clock::now_iso(),
        };
        record = SupportRecord {
            agent: Some(agent.clone()),
            last_choice: Some(choice.clone()),
        };
        if let Err(e) = self.write_record(&session, &record) {
            tracing::warn!("Support agent {session}: its record was not written: {e}");
        }
        Ok(SupportOpened { session, agent })
    }
}

/// Write the folder of an agent: its `instructions` (and the import claude
/// reads), and its first `question`, whose file it returns; without one, a
/// question left by an earlier opening is removed, as is the primer of an
/// earlier version.
fn write_folder(
    dir: &std::path::Path,
    instructions: &str,
    question: Option<&str>,
) -> Result<Option<std::path::PathBuf>, SupportError> {
    let write = |path: &std::path::Path, text: &str| {
        std::fs::write(path, text)
            .map_err(|e| SupportError::Folder(format!("failed to write {}: {e}", path.display())))
    };
    let _ = std::fs::remove_file(dir.join(SUPERSEDED_PRIMER));
    write(&support_instructions_path(dir), instructions)?;
    write(
        &support_instructions_import_path(dir),
        SUPPORT_INSTRUCTIONS_IMPORT,
    )?;
    let path = support_question_path(dir);
    match question.and_then(question_text) {
        Some(question) => {
            write(&path, &first_message(question))?;
            Ok(Some(path))
        }
        None => {
            let _ = std::fs::remove_file(&path);
            Ok(None)
        }
    }
}
