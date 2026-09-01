//! Ce qu'un `restart_node` a réellement fait, et comment ça se projette sur le
//! wire (#489, ADR-0037).
//!
//! Le type ne porte **aucun statut** : la projection ([`restart_response`]) en est
//! la seule propriétaire, donc « un spawn demandé qui n'a pas eu lieu répond 2xx »
//! est inexprimable. Avant #489 le bras jetait le `SpawnOutcome` de `spawn_node`
//! — pas même un `let _ =` — et répondait `200 {"ok":true}` sur les cinq issues,
//! y compris `Failed`, y compris un `node_id` absent du pipeline, et y compris le
//! cas où le sous-worktree existait déjà (100 % des nœuds isolated).
//!
//! Contrairement à `completion_refusal` (type tout-refus, invariant « jamais 2xx »),
//! celui-ci mélange succès, sursis et refus : l'invariant est **une projection
//! totale, variante par variante**.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};

/// Le verdict d'un `restart_node`. Une variante par sortie observable du bras.
#[derive(Debug, Clone)]
pub(crate) enum RestartVerdict {
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
pub(crate) enum RestartRefusal {
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
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            Self::RestartRejected { .. } => "restart_refused",
            Self::NodeNotFound { .. } => "node_not_found",
            Self::SandboxPrepNotReady { .. } => "sandbox_prep_not_ready",
            Self::SubWorktreeOccupied { .. } => "sub_worktree_occupied",
        }
    }

    /// Statut HTTP. Sans joker : ajouter une variante ne compile plus tant qu'on n'a
    /// pas décidé de son statut.
    fn status(&self) -> StatusCode {
        match self {
            // Une cible qui n'existe pas dans le pipeline est une requête malformée,
            // pas un conflit d'état.
            Self::NodeNotFound { .. } => StatusCode::BAD_REQUEST,
            Self::RestartRejected { .. }
            | Self::SandboxPrepNotReady { .. }
            | Self::SubWorktreeOccupied { .. } => StatusCode::CONFLICT,
        }
    }

    /// La session tmux du nœud a-t-elle déjà été tuée quand ce refus est parti ?
    ///
    /// **C'est le bit qui compte sur cette route** (ADR-0037 §5) : `false` n'a touché
    /// à rien (retry sûr) ; `true` signifie que la session est morte et que **rien ne
    /// l'a remplacée** — il faut un autre levier. Discriminé dans le corps, jamais en
    /// tordant un statut (ADR-0035 §3).
    fn session_killed(&self) -> bool {
        match self {
            Self::RestartRejected { session_killed, .. }
            | Self::SandboxPrepNotReady { session_killed, .. } => *session_killed,
            // Structurellement pré-kill : les deux sondes sont en tête du bras.
            Self::NodeNotFound { .. } | Self::SubWorktreeOccupied { .. } => false,
        }
    }

    /// Le détail spécifique, fusionné à plat dans le corps par [`restart_response`].
    fn detail(&self) -> serde_json::Value {
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

    pub(crate) fn reason(&self) -> String {
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

/// L'**unique** projection d'un verdict de restart vers HTTP.
///
/// Prend une **référence** : le verdict est aussi logué par son appelant. Ne pas
/// renvoyer la `Response` par valeur dans un `Result::Err` — `clippy::result_large_err`,
/// traité en `-D warnings` par la CI.
pub(crate) fn restart_response(v: &RestartVerdict) -> Response {
    match v {
        RestartVerdict::Spawned {
            node_id,
            iter,
            reused_sub_worktree,
            base_sha,
            interrupted_git_ops,
        } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                // Même vocabulaire que les commandes de pilotage de boucle
                // (ADR-0025) : une liste de paires, pas un booléen.
                "spawned": [{ "node_id": node_id, "iter": iter }],
                "reused_sub_worktree": reused_sub_worktree,
                "base_sha": base_sha,
                // #516: toujours un tableau, jamais `null` ni absent — un client
                // (futur #492) lit `body.interrupted_git_ops.length` sans garde.
                "interrupted_git_ops": interrupted_git_ops,
            })),
        )
            .into_response(),
        RestartVerdict::Waiting { reason } => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "waiting": true, "reason": reason })),
        )
            .into_response(),
        RestartVerdict::NoOp { reason } => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "noop": true, "reason": reason })),
        )
            .into_response(),
        RestartVerdict::Refused(r) => {
            let mut body = serde_json::json!({
                "error": r.slug(),
                // Uniformément `true` sur tous les refus de cette route, et c'est
                // intentionnel (ADR-0037 §4) : sa définition (ADR-0035) est « le
                // daemon a-t-il DÉJÀ enregistré l'issue terminale ? », et aucun refus
                // de restart n'enregistre quoi que ce soit. Le champ redevient
                // informatif sur le `500`.
                "recoverable": true,
                "session_killed": r.session_killed(),
            });
            if let (Some(obj), Some(extra)) = (body.as_object_mut(), r.detail().as_object()) {
                for (k, val) in extra {
                    obj.insert(k.clone(), val.clone());
                }
            }
            (r.status(), Json(body)).into_response()
        }
        RestartVerdict::Broken {
            message,
            run_failed,
        } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "spawn_failed",
                // Un `500` route la CLI vers `pdo fail`, conseil catastrophique si
                // `RunFailed` est déjà inscrit.
                "recoverable": !run_failed,
                "run_failed": run_failed,
                "session_killed": true,
                "message": message,
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un échantillon par variante, produit derrière un `match` **exhaustif sans
    /// joker** : ajouter une variante à `RestartVerdict` sans l'échantillonner ici ne
    /// compile plus.
    fn every_restart_verdict() -> Vec<RestartVerdict> {
        let all = vec![
            RestartVerdict::Spawned {
                node_id: "impl-1".into(),
                iter: 1,
                reused_sub_worktree: true,
                base_sha: Some("abc123".into()),
                interrupted_git_ops: vec!["index.lock".into(), "MERGE_HEAD".into()],
            },
            RestartVerdict::Waiting {
                reason: "session cap reached (20/20 live)".into(),
            },
            RestartVerdict::NoOp {
                reason: "nothing to do".into(),
            },
            RestartVerdict::Refused(RestartRefusal::RestartRejected {
                message: "node worker iter 2 is still live: refusing concurrent iter 1".into(),
                session_killed: false,
            }),
            RestartVerdict::Refused(RestartRefusal::NodeNotFound {
                node_id: "ghost".into(),
            }),
            RestartVerdict::Refused(RestartRefusal::SandboxPrepNotReady {
                message: "sandbox prep is still building".into(),
                session_killed: true,
            }),
            RestartVerdict::Refused(RestartRefusal::SubWorktreeOccupied {
                message: "branch pdo/sub-r-impl-1-iter-1 is checked out in another worktree".into(),
            }),
            RestartVerdict::Broken {
                message: "failed to ensure sub-worktree".into(),
                run_failed: true,
            },
        ];

        let mut seen = std::collections::BTreeSet::new();
        for v in &all {
            let key = match v {
                RestartVerdict::Spawned { .. } => "Spawned",
                RestartVerdict::Waiting { .. } => "Waiting",
                RestartVerdict::NoOp { .. } => "NoOp",
                RestartVerdict::Refused(r) => match r {
                    RestartRefusal::RestartRejected { .. } => "Refused/RestartRejected",
                    RestartRefusal::NodeNotFound { .. } => "Refused/NodeNotFound",
                    RestartRefusal::SandboxPrepNotReady { .. } => "Refused/SandboxPrepNotReady",
                    RestartRefusal::SubWorktreeOccupied { .. } => "Refused/SubWorktreeOccupied",
                },
                RestartVerdict::Broken { .. } => "Broken",
            };
            seen.insert(key);
        }
        assert_eq!(
            seen.len(),
            all.len(),
            "every_restart_verdict() must hold exactly one sample per variant"
        );
        all
    }

    /// L'invariant n'est PAS « jamais 2xx » (un tel test échouerait sur `Spawned`)
    /// mais la **totalité de la projection** : `Spawned`/`Waiting`/`NoOp` en `2xx`,
    /// tout le reste jamais.
    #[test]
    fn a_spawn_that_did_not_happen_never_projects_to_a_2xx() {
        for v in every_restart_verdict() {
            let status = restart_response(&v).status();
            let spawn_happened = matches!(
                v,
                RestartVerdict::Spawned { .. }
                    | RestartVerdict::Waiting { .. }
                    | RestartVerdict::NoOp { .. }
            );
            assert_eq!(
                status.is_success(),
                spawn_happened,
                "{v:?} projected to {status}"
            );
            if !spawn_happened {
                assert!(
                    status.is_client_error() || status.is_server_error(),
                    "{v:?} projected to a non-error status ({status})"
                );
            }
        }
    }

    /// Le manager route sur le statut exact, pas sur la classe. Table close.
    #[test]
    fn every_variant_maps_to_its_exact_status() {
        let cases = [
            (
                RestartVerdict::Spawned {
                    node_id: "n".into(),
                    iter: 1,
                    reused_sub_worktree: false,
                    base_sha: None,
                    interrupted_git_ops: vec![],
                },
                StatusCode::OK,
            ),
            (
                RestartVerdict::Waiting { reason: "r".into() },
                StatusCode::OK,
            ),
            (RestartVerdict::NoOp { reason: "r".into() }, StatusCode::OK),
            (
                RestartVerdict::Refused(RestartRefusal::RestartRejected {
                    message: "m".into(),
                    session_killed: false,
                }),
                StatusCode::CONFLICT,
            ),
            (
                RestartVerdict::Refused(RestartRefusal::NodeNotFound {
                    node_id: "g".into(),
                }),
                StatusCode::BAD_REQUEST,
            ),
            (
                RestartVerdict::Refused(RestartRefusal::SandboxPrepNotReady {
                    message: "m".into(),
                    session_killed: false,
                }),
                StatusCode::CONFLICT,
            ),
            (
                RestartVerdict::Refused(RestartRefusal::SubWorktreeOccupied {
                    message: "m".into(),
                }),
                StatusCode::CONFLICT,
            ),
            (
                RestartVerdict::Broken {
                    message: "m".into(),
                    run_failed: false,
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (v, want) in cases {
            assert_eq!(restart_response(&v).status(), want, "{v:?}");
        }
    }

    async fn body_of(v: &RestartVerdict) -> serde_json::Value {
        let bytes = axum::body::to_bytes(restart_response(v).into_body(), usize::MAX)
            .await
            .expect("restart body");
        serde_json::from_slice(&bytes).expect("restart body is JSON")
    }

    /// Les trois champs sur lesquels un client route.
    #[tokio::test]
    async fn every_refusal_body_carries_slug_recoverable_and_session_killed() {
        for v in every_restart_verdict() {
            let RestartVerdict::Refused(ref r) = v else {
                continue;
            };
            let body = body_of(&v).await;
            assert_eq!(body["error"].as_str(), Some(r.slug()), "{body}");
            assert_eq!(body["recoverable"], true, "{body}");
            assert!(
                body["session_killed"].is_boolean(),
                "{} has no boolean session_killed: {body}",
                r.slug()
            );
            assert_eq!(body["session_killed"], r.session_killed(), "{body}");
        }
    }

    /// La prose du garde part dans `message`, jamais dans `error` : un client qui
    /// sniffait `body["error"].contains("live")` doit tomber.
    #[tokio::test]
    async fn the_transition_guard_prose_lands_in_message() {
        let v = RestartVerdict::Refused(RestartRefusal::RestartRejected {
            message: "node worker iter 2 is still live: refusing concurrent iter 1".into(),
            session_killed: false,
        });
        let body = body_of(&v).await;
        assert_eq!(body["error"], "restart_refused");
        assert!(!body["error"].as_str().unwrap().contains("live"));
        assert!(body["message"].as_str().unwrap().contains("still live"));
    }

    /// `2xx` **et** pas un `noop` : un `NodeWaiting` a été appendé et a changé le
    /// statut du nœud (ADR-0037 §2). Ne pas « corriger » en no-op.
    #[tokio::test]
    async fn waiting_is_a_2xx_and_is_not_a_noop() {
        let body = body_of(&RestartVerdict::Waiting {
            reason: "session cap reached".into(),
        })
        .await;
        assert_eq!(body["ok"], true);
        assert_eq!(body["waiting"], true);
        assert!(body["reason"].is_string());
        assert!(
            body.get("noop").is_none(),
            "a reservation that flipped the node's status is not a no-op: {body}"
        );
    }

    /// Le seul endroit de la route où `recoverable` porte réellement un bit.
    #[tokio::test]
    async fn broken_derives_recoverable_from_run_failed() {
        for run_failed in [true, false] {
            let body = body_of(&RestartVerdict::Broken {
                message: "boom".into(),
                run_failed,
            })
            .await;
            assert_eq!(body["error"], "spawn_failed");
            assert_eq!(body["run_failed"], run_failed);
            assert_eq!(body["recoverable"], !run_failed);
            assert_eq!(body["session_killed"], true);
        }
    }

    /// Le succès dit ce qu'il a fait du sous-worktree, et `interrupted_git_ops`
    /// remonte **tous** les marqueurs dans l'ordre, jamais un seul (#516).
    #[tokio::test]
    async fn spawned_reports_the_sub_worktree_it_reused() {
        let body = body_of(&RestartVerdict::Spawned {
            node_id: "impl-1".into(),
            iter: 3,
            reused_sub_worktree: true,
            base_sha: Some("deadbeef".into()),
            interrupted_git_ops: vec!["index.lock".into(), "MERGE_HEAD".into()],
        })
        .await;
        assert_eq!(body["ok"], true);
        assert_eq!(body["spawned"][0]["node_id"], "impl-1");
        assert_eq!(body["spawned"][0]["iter"], 3);
        assert_eq!(body["reused_sub_worktree"], true);
        assert_eq!(body["base_sha"], "deadbeef");
        assert_eq!(
            body["interrupted_git_ops"],
            serde_json::json!(["index.lock", "MERGE_HEAD"])
        );

        // Un nœud sans sous-worktree : `interrupted_git_ops` reste `[]`, jamais
        // absent ni `null` — un client (#492) lit `.length` sans garde.
        let plain = body_of(&RestartVerdict::Spawned {
            node_id: "worker".into(),
            iter: 1,
            reused_sub_worktree: false,
            base_sha: None,
            interrupted_git_ops: vec![],
        })
        .await;
        assert_eq!(plain["reused_sub_worktree"], false);
        assert!(plain["base_sha"].is_null());
        assert_eq!(plain["interrupted_git_ops"], serde_json::json!([]));
        assert!(
            !plain["interrupted_git_ops"].is_null(),
            "jamais `null` : toujours un tableau"
        );
    }
}
