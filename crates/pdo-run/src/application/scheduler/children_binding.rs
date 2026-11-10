//! The strong binding of a parent node to its child runs (#724 / ADR-0064):
//! any node whose session created a child run (agent or `script`, Orchestrator
//! toggle or not) does not terminate while a child is live, and a watcher
//! completes it once they have all settled. The binding reads the real
//! children, never the toggle: the toggle only adds the `pdo-orchestrate`
//! skill and announces a parent node.

use crate::domain::{event, projection, state};
use tracing::{error, info, warn};

use crate::application::events::append_event;
use crate::application::events::load_all_run_ids;
use crate::application::events::load_events;
use crate::application::events::reload_run_state_with;
use crate::application::events::run_is_forgotten;
use crate::domain::completion_refusal;
use crate::Runs;

/// Le marqueur que la porte pose sur le NodeRun au refus : un `NodeAwaitingUser`
/// portant cette raison. C’est lui qui distingue « le nœud parent a rendu la
/// main et attend ses enfants » d’un agent encore au travail (auquel on ne
/// arrache jamais la main) et d’un nœud interactif simplement `AwaitingUser`.
pub const BINDING_MARKER_REASON: &str = "children_pending";

/// L’état des runs enfants d’UN nœud du Run parent, au moment de la lecture.
#[derive(Debug, Default)]
pub struct ChildrenSnapshot {
    pub total: usize,
    pub failed: Vec<String>,
    pub active: Vec<String>,
}

/// Le verdict de la liaison sur un [`ChildrenSnapshot`].
#[derive(Debug, PartialEq, Eq)]
pub enum ChildrenVerdict {
    /// Aucun enfant : la liaison ne dit rien — le nœud n’est pas (encore) un
    /// nœud parent, il se complète comme avant, toggle ou non.
    NoChildren,
    /// Tous les enfants sont terminaux, aucun n’est `failed` : le nœud peut
    /// se compléter.
    AllSettled,
    /// Au moins un enfant est non terminal : le nœud attend. `failed` compte
    /// les enfants `failed` — dès qu’il y en a un, l’arbitrage est à
    /// l’utilisateur.
    Pending { active: usize, failed: usize },
}

impl ChildrenSnapshot {
    /// Terminal pour la liaison = non-vivant (`Completed`/`Skipped`/`Halted`/
    /// `Archived`/`Failed`) ; `Failed` est le seul terminal qui parque le nœud
    /// en attente utilisateur. Un enfant `Running`/`AwaitingUser`/`Paused`
    /// retient le nœud — sauf s’il est tombstoné (forgotten) : son
    /// propriétaire l’a explicitement abandonné, la liaison ne retient pas un
    /// run qui n’existe plus.
    pub fn verdict(&self) -> ChildrenVerdict {
        if self.total == 0 {
            return ChildrenVerdict::NoChildren;
        }
        let active = self.active.len();
        if active == 0 && self.failed.is_empty() {
            return ChildrenVerdict::AllSettled;
        }
        ChildrenVerdict::Pending {
            active,
            failed: self.failed.len(),
        }
    }
}

/// Lit les runs enfants du nœud `node_id` du Run `run_id` : tout Run dont le
/// `RunStarted` mécanique porte ces deux valeurs de provenance. Recharge tout
/// à chaque appel — c’est ce qui rend la ré-adoption au restart gratuite.
///
/// La porte lit les enfants de TOUT nœud à chaque complétion : la lecture part
/// donc des seuls candidats (les `RunStarted` qui portent cette provenance, une
/// requête), de sorte qu’un nœud sans enfant, le cas courant, ne rejoue aucun
/// Run. Si la requête échoue, repli sur la liste de tous les Runs : plus lent,
/// jamais faux.
pub async fn children_snapshot(state: &Runs, run_id: &str, node_id: &str) -> ChildrenSnapshot {
    let mut snapshot = ChildrenSnapshot::default();
    let run_ids = match state.store().child_run_ids(run_id, node_id).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("binding: indexed child lookup failed for {node_id} in {run_id} ({e}); scanning");
            let Ok(ids) = load_all_run_ids(state).await else {
                error!("binding: failed to list runs for children of {node_id} in {run_id}");
                return snapshot;
            };
            ids
        }
    };
    for child_id in run_ids {
        if child_id == run_id {
            continue;
        }
        let Ok(events) = load_events(state, &child_id).await else {
            continue;
        };
        let Some(child) = projection::project(&events) else {
            continue;
        };
        if child.parent_run_id.as_deref() != Some(run_id)
            || child.parent_node_id.as_deref() != Some(node_id)
        {
            continue;
        }
        snapshot.total += 1;
        match child.status {
            state::RunStatus::Failed => snapshot.failed.push(child.run_id),
            status if status.is_live() => {
                // Un enfant oublié (tombstoné, ADR-0024) ne retient pas la
                // liaison : il ne projetera jamais d’état terminal.
                match run_is_forgotten(state, &child.run_id).await {
                    Ok(true) => {}
                    _ => snapshot.active.push(child.run_id),
                }
            }
            _ => {}
        }
    }
    snapshot
}

/// La porte de complétion de la liaison forte, pour tout **nœud parent** : elle
/// ne lit pas le toggle Orchestrator mais les enfants réels du nœud, de sorte
/// qu’un nœud sans enfant (ou dont tous les enfants sont oubliés, ou terminaux
/// sans échec) passe. Elle tient toutes les sources de complétion qui passent
/// par le corps commun : `pdo complete`, auto-complétion de fin de tour, fin
/// d’un `script`. `None` ⇒ la complétion peut continuer ; `Some(refusal)` ⇒
/// refus avec compteur, marqueur d’attente posé (nœud parqué `AwaitingUser`)
/// et pilote détaché (le watcher).
pub async fn parent_node_binding_refusal(
    state: &std::sync::Arc<Runs>,
    events: &[event::Event],
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Option<completion_refusal::CompletionRefusal> {
    let snapshot = children_snapshot(state, run_id, node_id).await;
    let ChildrenVerdict::Pending { active, failed } = snapshot.verdict() else {
        return None;
    };
    // Marque l’attente de la liaison sur le NodeRun (idempotent : la porte est
    // re-frappée à chaque tentative de complétion, l’événement ne s’empile pas).
    // Le statut projeté passe à `AwaitingUser` — la surface montre un nœud qui
    // attend — et le pilote ([`spawn_children_settled_watcher`]) saura que
    // l’agent a rendu la main.
    if !binding_marker_present(events, node_id, iter) {
        let marker = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::NodeAwaitingUser,
            node_id: Some(node_id.to_string()),
            iter: Some(iter),
            payload: Some(serde_json::json!({
                "reason": BINDING_MARKER_REASON,
                "active": active,
                "failed": failed,
            })),
        };
        if let Err(e) = append_event(state, &marker).await {
            error!(
                "binding: failed to append the children_pending marker for {node_id} \
                 iter-{iter} in {run_id}: {e}"
            );
        }
    }
    // Le pilote part à CHAQUE refus : idempotent par construction (le watcher
    // sort au premier verdict, et une complétion déjà posée est un NoOp), il
    // couvre aussi le cas « un watcher précédent est sorti sur un refus ».
    spawn_children_settled_watcher(
        std::sync::Arc::clone(state),
        run_id.to_string(),
        node_id.to_string(),
        iter,
    );
    Some(completion_refusal::CompletionRefusal::ChildrenPending {
        node_id: node_id.to_string(),
        active,
        failed,
    })
}

/// Le **pilote** de la liaison — une tâche détachée (le « watcher ») posée par
/// la porte à chaque refus. Tant que le nœud attend (marqueur `children_pending`,
/// itération courante), il revérifie périodiquement le verdict : dès que tous
/// les enfants sont terminaux sans échec, il complète le nœud de lui-même —
/// « le nœud se complète » — par la voie commune
/// ([`crate::application::complete_node::complete_node_iteration`], source `ChildrenSettled`) : livraison,
/// validation, événement terminal et advance sont exactement ceux d’un
/// `pdo complete`. Ré-entrance bénigne : une complétion déjà posée est un NoOp
/// (garde de transition), deux watchers qui s’évitent ne produisent qu’un
/// `NodeCompleted`.
///
/// Pourquoi un watcher plutôt qu’un réveil à l’événement enfant : la liaison
/// relit TOUT à chaque tick (provenance gelée, jamais mise en cache), ce qui
/// rend la ré-adoption au restart gratuite et résiste à n’importe quel chemin
/// par lequel un enfant se termine (`pdo complete`, `pdo fail`, forçage,
/// sweep, restart de l’enfant).
pub fn spawn_children_settled_watcher(
    state: std::sync::Arc<Runs>,
    run_id: String,
    node_id: String,
    iter: i64,
) {
    tokio::spawn(async move {
        // Backoff : réactif au début (les tests comme les humains attendent un
        // enfant qui vient de se terminer), économe ensuite (un nœud parent
        // peut attendre des enfants pendant des heures — c’est le contrat).
        let mut delay = std::time::Duration::from_millis(250);
        loop {
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(std::time::Duration::from_secs(5));

            // Un run oublié (ADR-0024) ne pilote plus rien.
            if run_is_forgotten(&state, &run_id).await.unwrap_or(true) {
                return;
            }
            let Some((parent_events, parent_state)) = reload_run_state_with(&state, &run_id).await
            else {
                return;
            };
            let Some(node) = parent_state.nodes.get(&node_id) else {
                return;
            };
            // Une itération plus récente possède désormais l’attente : ce
            // watcher est périmé (restart du nœud → ré-adoption par la porte
            // elle-même, qui relit les enfants vivants).
            if node.iter != iter {
                return;
            }
            if !matches!(
                node.status,
                state::NodeStatus::Running | state::NodeStatus::AwaitingUser
            ) {
                return; // déjà terminal, stoppé ou incident : plus rien à piloter
            }
            // Sans le marqueur, l’agent n’a pas encore rendu la main : on ne
            // lui arrache jamais la complétion parce que ses enfants ont fini
            // tôt — il re-complètera quand il voudra.
            if !binding_marker_present(&parent_events, &node_id, iter) {
                return;
            }

            let snapshot = children_snapshot(&state, &run_id, &node_id).await;
            if snapshot.verdict() != ChildrenVerdict::AllSettled {
                continue; // enfants encore actifs ou échoués : la liaison attend
            }

            info!(
                "binding: every child run of parent node {node_id} iter-{iter} in run \
                 {run_id} is terminal — completing the node"
            );
            match crate::application::complete_node::complete_node_iteration(
                &state,
                run_id.clone(),
                node_id.clone(),
                iter,
                crate::application::complete_node::CompletionSource::ChildrenSettled,
            )
            .await
            {
                crate::application::complete_node::CompletionAttempt::Completed => {
                    info!(
                        "binding: parent node {node_id} completed in run {run_id} \
                         (children settled)"
                    );
                }
                crate::application::complete_node::CompletionAttempt::NoOp { reason } => {
                    info!("binding: auto-completion was a no-op: {reason}");
                }
                crate::application::complete_node::CompletionAttempt::Refused { refusal } => {
                    // La porte commune a refusé (livraison, validation) : on
                    // rend la main — une prochaine tentative de complétion
                    // re-posera un watcher, et l’agent garde sa session.
                    warn!(
                        "binding: auto-completion of {node_id} in {run_id} refused ({}): {}",
                        refusal.slug(),
                        refusal.reason()
                    );
                }
            }
            return;
        }
    });
}

/// Le marqueur `children_pending` est-il déjà posé sur cette itération ?
pub(crate) fn binding_marker_present(events: &[event::Event], node_id: &str, iter: i64) -> bool {
    events.iter().any(|e| {
        e.kind == event::EventKind::NodeAwaitingUser
            && e.node_id.as_deref() == Some(node_id)
            && e.iter == Some(iter)
            && e.payload
                .as_ref()
                .and_then(|p| p.get("reason"))
                .and_then(|v| v.as_str())
                == Some(BINDING_MARKER_REASON)
    })
}
