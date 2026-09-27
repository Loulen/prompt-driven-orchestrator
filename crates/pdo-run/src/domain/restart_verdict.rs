//! Ce qu'un `restart_node` a réellement fait, et comment ça se projette sur le
//! wire (#489, ADR-0037).
//!
//! Le type ne porte **aucun statut** : la projection (`restart_response` (dans `adapters::http::verdicts`)) en est
//! la seule propriétaire, donc « un spawn demandé qui n'a pas eu lieu répond 2xx »
//! est inexprimable. Avant #489 le bras jetait le `SpawnOutcome` de `spawn_node`
//! — pas même un `let _ =` — et répondait `200 {"ok":true}` sur les cinq issues,
//! y compris `Failed`, y compris un `node_id` absent du pipeline, et y compris le
//! cas où le sous-worktree existait déjà (100 % des nœuds isolated).
//!
//! Contrairement à `completion_refusal` (type tout-refus, invariant « jamais 2xx »),
//! celui-ci mélange succès, sursis et refus : l'invariant est **une projection
//! totale, variante par variante**.

/// Le verdict d'un `restart_node`. Une variante par sortie observable du bras.
#[derive(Debug, Clone)]
pub enum RestartVerdict {
    /// Le nœud a été re-spawné : un `NodeStarted` est au log et une session tmux a
    /// été lancée.
    Spawned {
        node_id: String,
        iter: i64,
        /// Le sous-worktree existait déjà sur la bonne branche et a été réutilisé
        /// **en place** : le travail non commité de la session morte est toujours là
        /// (#489-B).
        reused_sub_worktree: bool,
        /// La base de coupe du sous-worktree (#503 / ADR-0036), reportée telle quelle
        /// sur une réutilisation.
        base_sha: Option<String>,
        /// Toutes les opérations git interrompues trouvées dans le gitdir privé du
        /// sous-worktree réutilisé, dans l'ordre du scan (#516). Signalées, jamais
        /// supprimées. `[]` sur une coupe fraîche ou un nœud sans sous-worktree.
        interrupted_git_ops: Vec<String>,
    },
    /// Le cap d'admission a mis le nœud en file : un `NodeWaiting` **a** été appendé,
    /// il flippe le statut du nœud à `Waiting`, et `retry_waiting_nodes` reprend
    /// réellement ce nœud-là.
    ///
    /// **Reste `2xx`, et n'est pas un `noop`** (ADR-0037 §2) : ne pas le requalifier
    /// en no-op, une réservation a changé le statut du nœud.
    Waiting { reason: String },
    /// Le garde a rendu `NoOp` : rien à faire, rien fait. **Défensif** —
    /// `validate_start` ne rend jamais `NoOp` aujourd'hui ; la variante existe pour
    /// la parité avec `force_spawn_node`, qui traite les deux à l'identique.
    NoOp { reason: String },
    /// Refus argumenté. Le statut appartient au refus.
    Refused(RestartRefusal),
    /// `SpawnOutcome::Failed` : ce n'est pas un refus, c'est une panne → `500`.
    Broken {
        message: String,
        /// Un `RunFailed` est-il **déjà** au log ? Les producteurs de `Failed`
        /// divergent sur ce point : ne pas le deviner, le re-projeter.
        run_failed: bool,
    },
}

/// Pourquoi un `restart_node` a été refusé.
#[derive(Debug, Clone)]
pub enum RestartRefusal {
    /// Refus du garde de transition (#212 / #196). Les trois raisons du garde (Run
    /// non vivant, itération concurrente vivante, itération déjà complétée) sont
    /// **aplaties** sur un seul slug (`restart_refused`), la prose dans `message` —
    /// bien que `Verdict::Reject` porte une cause typée depuis #515. Discriminer sur
    /// la route de retry est #487. Ne jamais sniffer la prose au `contains()`.
    RestartRejected {
        message: String,
        session_killed: bool,
    },
    /// La cible n'existe pas dans le pipeline **du Run** (son snapshot, pas la
    /// bibliothèque).
    NodeNotFound { node_id: String },
    /// Run sandboxé dont le conteneur n'est pas prêt (#445). Sondé **avant** le kill ;
    /// `session_killed` n'est `true` que sur la course (la précondition est passée à
    /// la sonde de tête et retombée dans `spawn_node`).
    SandboxPrepNotReady {
        message: String,
        session_killed: bool,
    },
    /// Le sous-worktree est tenu par quelqu'un d'autre (branche checkoutée dans un
    /// autre worktree vivant, ou répertoire non-worktree non vide). On refuse et on
    /// nomme ce qui le tient : le reaper détruirait précisément le travail que #489
    /// existe pour sauver.
    SubWorktreeOccupied { message: String },
}

impl RestartRefusal {
    /// Le slug stable sur lequel les clients discriminent. **Jamais** le statut.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::RestartRejected { .. } => "restart_refused",
            Self::NodeNotFound { .. } => "node_not_found",
            Self::SandboxPrepNotReady { .. } => "sandbox_prep_not_ready",
            Self::SubWorktreeOccupied { .. } => "sub_worktree_occupied",
        }
    }

    /// La session tmux du nœud a-t-elle déjà été tuée quand ce refus est parti ?
    ///
    /// **C'est le bit qui compte sur cette route** (ADR-0037 §5) : `false` n'a touché
    /// à rien (retry sûr) ; `true` signifie que la session est morte et que **rien ne
    /// l'a remplacée** — il faut un autre levier. Discriminé dans le corps, jamais en
    /// tordant un statut (ADR-0035 §3).
    pub fn session_killed(&self) -> bool {
        match self {
            Self::RestartRejected { session_killed, .. }
            | Self::SandboxPrepNotReady { session_killed, .. } => *session_killed,
            // Structurellement pré-kill : les deux sondes sont en tête du bras.
            Self::NodeNotFound { .. } | Self::SubWorktreeOccupied { .. } => false,
        }
    }

    /// Le détail spécifique, fusionné à plat dans le corps par `restart_response` (dans `adapters::http::verdicts`).
    pub fn detail(&self) -> serde_json::Value {
        match self {
            Self::RestartRejected { message, .. }
            | Self::SandboxPrepNotReady { message, .. }
            | Self::SubWorktreeOccupied { message } => serde_json::json!({ "message": message }),
            Self::NodeNotFound { node_id } => serde_json::json!({
                "node_id": node_id,
                "message": format!("node '{node_id}' not found in the run's pipeline"),
            }),
        }
    }

    pub fn reason(&self) -> String {
        match self {
            Self::RestartRejected { message, .. }
            | Self::SandboxPrepNotReady { message, .. }
            | Self::SubWorktreeOccupied { message } => message.clone(),
            Self::NodeNotFound { node_id } => {
                format!("node '{node_id}' not found in the run's pipeline")
            }
        }
    }
}
