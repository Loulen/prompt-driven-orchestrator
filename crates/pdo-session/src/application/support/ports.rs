//! What the Agent de support needs from the outside world: which agent a
//! choice resolves to on this machine.

use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_harness::registry::HarnessDescriptor;

use crate::application::ports::BoxFuture;

/// The agent a choice resolves to: its harness (installed), its model and
/// effort, and the name the choice goes by.
#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub harness: HarnessDescriptor,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The profile's name, or `Custom`.
    pub label: String,
}

/// Why a choice resolves to no agent that can run here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceRefusal {
    /// `Inherit`: an Agent de support has no tier to inherit from.
    Inherit,
    /// No profile has this id.
    UnknownProfile(String),
    /// No harness has this name.
    UnknownHarness(String),
    /// The harness's binary is not installed on this machine.
    NotInstalled(String),
    /// The agentic profiles could not be read.
    Unreadable(String),
}

impl std::fmt::Display for ChoiceRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChoiceRefusal::Inherit => {
                write!(f, "a support agent needs a profile or a harness, not `inherit`")
            }
            ChoiceRefusal::UnknownProfile(id) => write!(f, "no agentic profile `{id}`"),
            ChoiceRefusal::UnknownHarness(name) => write!(f, "unknown harness `{name}`"),
            ChoiceRefusal::NotInstalled(name) => write!(
                f,
                "the harness `{name}` is not installed on this machine: install it, or choose another agent"
            ),
            ChoiceRefusal::Unreadable(why) => {
                write!(f, "the agentic profiles could not be read: {why}")
            }
        }
    }
}

/// Resolve a choice of agent. Implemented by the composition root over the
/// agentic profiles, the harness registry and the binary probe.
pub trait SupportAgents: Send + Sync {
    /// The agent `choice` runs: a profile's combination or the custom one,
    /// with the instance's default model for its harness when it names none.
    fn resolve<'a>(
        &'a self,
        choice: &'a AgentChoice,
    ) -> BoxFuture<'a, Result<ResolvedAgent, ChoiceRefusal>>;
}
