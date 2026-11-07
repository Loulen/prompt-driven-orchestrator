//! Pourquoi une complétion a été refusée, et comment ce refus se projette sur le
//! wire (#490, ADR-0035).
//!
//! Le type ne porte **aucun statut** : la projection (`refusal_response` (dans `adapters::http::verdicts`)) en est
//! la seule propriétaire, donc « un refus qui répond 2xx » est inexprimable.
//!
//! N'attachez pas le garde à `CompletionAttempt` : le bras `mark_node_done` de
//! `POST /runs/:id/commands` n'en construit jamais, et l'invariant ne couvrirait
//! que `POST …/nodes/:id/done`. Les deux appelants passent par la projection.

/// `Clone` parce que le corps partagé rend la variante à ses deux appelants et que
/// l'un d'eux la loge avant de la projeter.
#[derive(Debug, Clone)]
pub enum CompletionRefusal {
    /// Run tombstoné (#328 / ADR-0024). Garde son `410` : « jamais 2xx » ne veut
    /// pas dire « toujours 409 ».
    RunForgotten {
        run_id: String,
    },
    RunNotFound,
    /// Panne interne **avant** tout verdict (lecture du log, contrôle de tombstone).
    /// `500` : ce n'est pas un refus argumenté, c'est un daemon qui n'a pas pu
    /// décider — et c'est le seul cas où `pdo fail` reste le bon conseil.
    Internal {
        error: String,
    },
    /// Garde de transition (#212 / #354). La prose part dans `message`, `error`
    /// porte le slug : c'est ce qui rend le refus lisible côté client, qui relisait
    /// jusqu'ici *tout* `409` comme `missing_outputs`.
    CompletionRejected {
        message: String,
    },
    /// An interactive node remains under human control until the user releases
    /// this exact session. No terminal event is written.
    CompletionNotReleased,
    /// La livraison (#654 / ADR-0060) a échoué — staging, commit ou merge. Panne,
    /// pas verdict, donc `500`. Un `NodeInterrupted` est déjà appendé et le Run
    /// est parqué `AwaitingUser` : le travail reste sur disque, intact.
    DeliveryFailed {
        node_id: String,
        error: String,
    },
    /// Conflit de merge ; `MergeConflictDetected` + `RunFailed` déjà appendés.
    MergeConflict {
        node_id: String,
    },
    /// Ports de sortie déclarés sans artefact. Le node reste vivant.
    MissingOutputs {
        missing: Vec<String>,
    },
    /// Fail-fast d'un node `script` (ADR-0017) ; `NodeFailed` + `RunFailed`
    /// appendés. Le `detail` reste **imbriqué** : aplatir rendrait la trace
    /// d'audit indistinguable d'un échec après retry (ADR-0035 §5).
    ScriptValidationFailed {
        detail: serde_json::Value,
    },
    /// Mismatch de frontmatter, message correctif envoyé, node toujours `running`.
    FrontmatterRetryPending {
        violations: Vec<serde_json::Value>,
    },
    /// Un nœud a modifié un fichier **suivi** d'un dépôt secondaire read-only
    /// (#465, ADR-0042). Garde de complétion, **pas** une panne : aucun événement
    /// terminal n'est appendé (contrairement à `DeliveryFailed`), le nœud reste
    /// vivant, l'agent nettoie le secondaire (`git checkout`) et re-complète.
    /// `recoverable:false` par choix de #465 — le read-only d'un secondaire est un
    /// contrat, non un retry de sortie ; le slug (jamais le seul statut) discrimine.
    SecondaryRepoDirtied {
        alias: String,
        message: String,
    },
    /// Mismatch après l'unique retry ; `NodeFailed` + `RunFailed` appendés.
    FrontmatterRetryExhausted {
        violations: Vec<serde_json::Value>,
    },
    /// L'événement terminal n'a pas pu être appendé — panne, `500`.
    AppendFailed {
        error: String,
    },
    /// Liaison forte orchestrateur ↔ enfants (#724 / ADR-0064) : le nœud est
    /// orchestrator (toggle gelé au démarrage du Run) et ses runs enfants (mêmes
    /// `parent_run_id` + `parent_node_id`) ne sont pas tous terminaux. Le nœud
    /// reste vivant : aucun événement terminal n'est appendé, la livraison n'a
    /// pas lieu. Avec au moins un enfant `failed`, le NodeRun est parqué
    /// `AwaitingUser` (le `NodeAwaitingUser` porte `reason: "children_pending"`)
    /// et la décision appartient à l'utilisateur — retry de l'enfant ou forçage
    /// (`mark_node_done`), jamais à l'agent.
    ChildrenPending {
        node_id: String,
        /// Enfants encore non terminaux (running / awaiting_user / paused).
        active: usize,
        /// Enfants `failed` — ceux que l'utilisateur doit trancher.
        failed: usize,
    },
}

impl CompletionRefusal {
    /// Le slug stable sur lequel les clients discriminent — **jamais** le statut,
    /// qui n'a pas assez de bits pour toutes les causes.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::RunForgotten { .. } => "run_forgotten",
            Self::RunNotFound => "run_not_found",
            Self::Internal { .. } => "internal_error",
            Self::CompletionRejected { .. } => "completion_rejected",
            Self::CompletionNotReleased => "completion_not_released",
            Self::DeliveryFailed { .. } => "delivery_failed",
            Self::MergeConflict { .. } => "merge_conflict",
            Self::MissingOutputs { .. } => "missing_outputs",
            Self::ScriptValidationFailed { .. } => "script_validation_failed",
            Self::FrontmatterRetryPending { .. } => "frontmatter_retry_pending",
            Self::FrontmatterRetryExhausted { .. } => "frontmatter_retry_exhausted",
            Self::SecondaryRepoDirtied { .. } => "secondary_repo_dirtied",
            Self::AppendFailed { .. } => "append_failed",
            Self::ChildrenPending { .. } => "children_pending",
        }
    }

    /// Est-ce encore le tour de l'appelant ? `false` ⇒ le daemon a déjà enregistré
    /// une issue terminale : l'appelant ne doit **rien** appender de plus, et
    /// surtout pas enchaîner `pdo fail`.
    pub fn recoverable(&self) -> bool {
        matches!(
            self,
            Self::MissingOutputs { .. }
                | Self::CompletionNotReleased
                | Self::FrontmatterRetryPending { .. }
                // Des enfants simplement en cours : l'agent garde la main — il
                // attend qu'ils se terminent et re-complète. Avec un enfant
                // `failed`, c'est l'utilisateur qui tranche (exit 4).
                | Self::ChildrenPending { failed: 0, .. }
        )
    }

    /// Le détail spécifique, **verbatim celui d'avant #490** : ne renommez et
    /// n'aplatissez aucun champ. Fusionné à plat par `refusal_response` (dans `adapters::http::verdicts`).
    pub fn detail(&self) -> serde_json::Value {
        match self {
            Self::RunForgotten { run_id } => {
                serde_json::json!({ "message": format!("run {run_id} has been forgotten") })
            }
            Self::RunNotFound => serde_json::json!({ "message": "run not found" }),
            Self::Internal { error } => serde_json::json!({ "message": error }),
            Self::CompletionRejected { message } => serde_json::json!({ "message": message }),
            Self::CompletionNotReleased => serde_json::json!({
                "message": "this interactive node has not been released; ask the user to choose \"Auto-approve when ready\" in the Approve menu (or to click \"Approve\"), then retry"
            }),
            Self::AppendFailed { error } => serde_json::json!({ "message": error }),
            Self::DeliveryFailed { node_id, error } => serde_json::json!({
                "message": format!("failed to deliver {node_id}'s work: {error}")
            }),
            Self::MergeConflict { node_id } => {
                serde_json::json!({ "message": format!("merge conflict on {node_id}") })
            }
            Self::MissingOutputs { missing } => serde_json::json!({ "missing": missing }),
            Self::ScriptValidationFailed { detail } => serde_json::json!({ "detail": detail }),
            Self::FrontmatterRetryPending { violations }
            | Self::FrontmatterRetryExhausted { violations } => {
                serde_json::json!({ "violations": violations })
            }
            Self::SecondaryRepoDirtied { alias, message } => serde_json::json!({
                "alias": alias,
                "message": message,
            }),
            Self::ChildrenPending {
                node_id,
                active,
                failed,
            } => serde_json::json!({
                "node_id": node_id,
                "active": active,
                "failed": failed,
                "message": children_pending_message(node_id, *active, *failed),
            }),
        }
    }

    /// Raison lisible pour le log et pour la veille de vivacité, qui lit la
    /// variante et jamais le statut (#469 / ADR-0032 — c'est pourquoi elle est
    /// indemne de ce lot).
    pub fn reason(&self) -> String {
        match self {
            Self::RunForgotten { run_id } => format!("run {run_id} has been forgotten"),
            Self::RunNotFound => "run not found".into(),
            Self::Internal { error } => error.clone(),
            Self::CompletionRejected { message } => message.clone(),
            Self::CompletionNotReleased => {
                "interactive node completion has not been released by the user".into()
            }
            Self::DeliveryFailed { node_id, error } => {
                format!("failed to deliver {node_id}'s work: {error}")
            }
            Self::MergeConflict { node_id } => format!("merge conflict on {node_id}"),
            Self::MissingOutputs { missing } => {
                format!("missing declared outputs: {}", missing.join(", "))
            }
            Self::ScriptValidationFailed { .. } => {
                "script node failed output validation — fail-fast".into()
            }
            Self::FrontmatterRetryPending { .. } => {
                "output frontmatter mismatch — corrective message sent, awaiting retry".into()
            }
            Self::FrontmatterRetryExhausted { .. } => "output validation failed after retry".into(),
            Self::SecondaryRepoDirtied { alias, message } => {
                format!("secondary repo '{alias}' dirtied: {message}")
            }
            Self::AppendFailed { error } => {
                format!("failed to append the terminal event: {error}")
            }
            Self::ChildrenPending {
                node_id,
                active,
                failed,
            } => children_pending_message(node_id, *active, *failed),
        }
    }
}

/// Le message « avec compteur » du refus de liaison (#724) : l'agent (et la
/// surface) voient COMBIEN d'enfants retiennent encore le nœud, et lesquels
/// réclament l'utilisateur.
fn children_pending_message(node_id: &str, active: usize, failed: usize) -> String {
    if failed > 0 {
        format!(
            "orchestrator node `{node_id}` is held by its child runs: {active} active, \
             {failed} failed — the node is awaiting the user: retry the failed child run or \
             force-complete the node"
        )
    } else {
        format!(
            "orchestrator node `{node_id}` is held by its child runs: {active} active — \
             complete again once every child run is terminal"
        )
    }
}
