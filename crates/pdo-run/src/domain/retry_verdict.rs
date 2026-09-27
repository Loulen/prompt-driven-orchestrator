//! Ce qu'un `node_retry` a réellement fait, et comment ça se projette sur le wire
//! (#487 — applique ADR-0037 + ADR-0035 §3).
//!
//! Ne réutilisez pas `RestartVerdict` : ses variantes sont adossées à la surface
//! *restart* (`SubWorktreeOccupied`, la sonde même-`iter`, le `session_killed` d'un
//! kill pré-spawn), et *retry* est un autre geste (table-rase : `stop` +
//! `invalidate_nodes` + re-spawn — à `iter+1` pour un nœud simple, au **même `iter`**
//! pour un membre de boucle bornée, dont l'`iter` EST l'index de lap). Ce qui se clone
//! est l'invariant de #489 : **un spawn demandé qui n'a pas eu lieu ne se projette
//! jamais en `2xx`**.
//!
//! La forme de succès garde `iter` et `invalidated` **au niveau racine** : contrat
//! historique de la route que le canvas et son client lisent déjà. Les champs neufs
//! s'ajoutent à côté, jamais à la place.

/// Une variante par sortie observable du handler.
#[derive(Debug, Clone)]
pub enum RetryVerdict {
    /// Re-spawné : un `NodeStarted` est au log et une session tmux tourne.
    Spawned {
        node_id: String,
        iter: i64,
        /// Les nœuds **aval** dont ce retry a invalidé les artefacts, triés.
        /// L'auto-invalidation du nœud lui-même reste un événement au log, hors
        /// de cette liste : le canvas l'affiche telle quelle.
        invalidated: Vec<String>,
        /// Le sous-worktree existait déjà sur la bonne branche et a été réutilisé en
        /// place (#489-B). Toujours `false` pour un nœud sans sous-worktree.
        reused_sub_worktree: bool,
        /// La base de coupe du sous-worktree (#503 / ADR-0036). `None` sans sous-worktree.
        base_sha: Option<String>,
        /// Les opérations git interrompues trouvées dans un sous-worktree réutilisé,
        /// dans l'ordre du scan (#516). `[]` sur une coupe fraîche ou sans sous-worktree.
        interrupted_git_ops: Vec<String>,
    },
    /// Le cap d'admission a mis le nœud en file (`NodeWaiting` appendé,
    /// `retry_waiting_nodes` le reprend). **Reste `2xx`, et ce n'est pas un `noop`**
    /// (ADR-0037 §2) : une réservation qui a changé le statut du nœud n'est pas un
    /// « rien fait ».
    Waiting {
        reason: String,
        invalidated: Vec<String>,
    },
    Refused(RetryRefusal),
    /// `SpawnOutcome::Failed` : pas un refus, une panne → `500`.
    Broken {
        message: String,
        /// Un `RunFailed` est-il **déjà** au log ? On re-projette, on ne devine pas
        /// (mêmes divergences de producteurs que sur `restart_node`).
        run_failed: bool,
    },
}

#[derive(Debug, Clone)]
pub enum RetryRefusal {
    /// Refus du garde de transition (#212). Ne discriminez pas : **UN** slug
    /// (`retry_refused`), la prose du garde dans `message`. Deux points d'émission :
    /// - la **sonde de tête** `retry_run_precondition` (Run terminal/pausé → « resume
    ///   the run first ») — le seul point qui ferme l'incident production #496 ;
    /// - `spawn_node` sur la course (le Run est devenu terminal entre la sonde et le
    ///   spawn), après que le nœud vivant a été stoppé + invalidé — d'où `session_killed`.
    RetryRejected {
        message: String,
        session_killed: bool,
    },
    /// Run sandboxé dont le conteneur n'est pas prêt (#445). Sondé **avant** tout effet
    /// de bord par la tête (`session_killed:false`) ; `true` seulement si `spawn_node`
    /// le retombe sur la course, après le stop.
    SandboxPrepNotReady {
        message: String,
        session_killed: bool,
    },
    /// La cible n'existe pas dans le pipeline **du Run** (son snapshot). Sondé en tête,
    /// avant tout effet de bord.
    NodeNotFound { node_id: String },
}

impl RetryRefusal {
    /// Le slug stable sur lequel les clients discriminent. **Jamais** le statut.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::RetryRejected { .. } => "retry_refused",
            Self::SandboxPrepNotReady { .. } => "sandbox_prep_not_ready",
            Self::NodeNotFound { .. } => "node_not_found",
        }
    }

    /// La session tmux du nœud a-t-elle déjà été tuée quand ce refus est parti ?
    /// Le bit qui compte sur les routes de (re)spawn par nœud (ADR-0037 §5). Sur la
    /// route de retry il est presque toujours `false` : la sonde de tête est le
    /// premier geste, avant le stop — c'est tout l'objet de #487.
    pub fn session_killed(&self) -> bool {
        match self {
            Self::RetryRejected { session_killed, .. }
            | Self::SandboxPrepNotReady { session_killed, .. } => *session_killed,
            // Structurellement pré-kill : la sonde de cible est en tête du handler.
            Self::NodeNotFound { .. } => false,
        }
    }

    /// Le détail spécifique, fusionné à plat dans le corps par `retry_response` (dans `adapters::http::verdicts`).
    pub fn detail(&self) -> serde_json::Value {
        match self {
            Self::RetryRejected { message, .. } | Self::SandboxPrepNotReady { message, .. } => {
                serde_json::json!({ "message": message })
            }
            Self::NodeNotFound { node_id } => serde_json::json!({
                "node_id": node_id,
                "message": format!("node '{node_id}' not found in the run's pipeline"),
            }),
        }
    }

    pub fn reason(&self) -> String {
        match self {
            Self::RetryRejected { message, .. } | Self::SandboxPrepNotReady { message, .. } => {
                message.clone()
            }
            Self::NodeNotFound { node_id } => {
                format!("node '{node_id}' not found in the run's pipeline")
            }
        }
    }
}
