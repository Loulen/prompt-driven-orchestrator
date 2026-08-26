//! Ce qu'un `node_retry` a réellement fait, et comment ça se projette sur le wire
//! (#487 — applique ADR-0037 + ADR-0035 §3, sans ADR neuve).
//!
//! Patron cloné de [`crate::restart_verdict`] (#489, ADR-0037), **pas le type** : la
//! coordination sur #487 est explicite là-dessus — les variantes de `RestartVerdict`
//! sont adossées à la surface *restart* (`SubWorktreeOccupied`, la sonde même-`iter`,
//! le `session_killed` d'un kill pré-spawn), et *retry* est un autre geste (table-rase :
//! `stop` + `invalidate_nodes` + re-spawn — à `iter+1` pour un nœud simple, au **même
//! `iter`** pour un membre de boucle bornée, dont l'`iter` EST l'index de lap).
//! L'invariant clonable n'est donc pas
//! le type mais celui que #489 a épinglé : **un spawn demandé qui n'a pas eu lieu ne se
//! projette jamais en `2xx`** — une projection totale, variante par variante.
//!
//! Avant #487 le handler renvoyait `200 {"ok":true}` inconditionnellement — y compris
//! quand il spawnait une session `claude` orpheline sur un Run terminal et n'appendait
//! **rien** au log. Ce type rend ce mensonge inexprimable.
//!
//! La forme de succès garde `iter` et `invalidated` **au niveau racine** : le contrat
//! historique de la route (`{ok, iter, invalidated}`) que le canvas et son client
//! lisent déjà. Les champs neufs (`spawned`, `reused_sub_worktree`, `base_sha`,
//! `interrupted_git_ops`) s'ajoutent à côté, jamais à la place.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};

/// Le verdict d'un `node_retry`. Une variante par sortie observable du handler.
#[derive(Debug, Clone)]
pub(crate) enum RetryVerdict {
    /// Le nœud a été re-spawné (à `iter+1` pour un nœud simple, au même `iter` pour un
    /// membre de boucle bornée) : un `NodeStarted` est au log et une session tmux a été
    /// lancée (via la primitive de référence `node_spawn::spawn_node`).
    Spawned {
        node_id: String,
        iter: i64,
        /// Les nœuds aval dont ce retry a invalidé les artefacts — la liste que le
        /// canvas a toujours affichée. Triée. (L'auto-invalidation du nœud lui-même
        /// est un événement au log, pas une entrée de cette liste, comme avant #487.)
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
    /// Le cap d'admission a mis le nœud en file : `spawn_node` a appendé un
    /// `NodeWaiting` qui a flippé le nœud à `Waiting`, et `retry_waiting_nodes` le
    /// reprend. **Reste `2xx`, et ce n'est pas un `noop`** (ADR-0037 §2) : une
    /// réservation qui a changé le statut du nœud n'est pas un « rien fait ».
    Waiting {
        reason: String,
        invalidated: Vec<String>,
    },
    /// Refus argumenté. Le statut appartient au refus.
    Refused(RetryRefusal),
    /// `SpawnOutcome::Failed` : pas un refus, une panne → `500`.
    Broken {
        message: String,
        /// Un `RunFailed` est-il **déjà** au log ? On re-projette, on ne devine pas
        /// (mêmes divergences de producteurs que sur `restart_node`).
        run_failed: bool,
    },
}

/// Pourquoi un `node_retry` a été refusé.
#[derive(Debug, Clone)]
pub(crate) enum RetryRefusal {
    /// Refus du garde de transition (#212). **UN** slug (`retry_refused`), la prose du
    /// garde dans `message`, jamais discriminé (même posture que
    /// `RestartRefusal::RestartRejected`). Deux points d'émission :
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
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            Self::RetryRejected { .. } => "retry_refused",
            Self::SandboxPrepNotReady { .. } => "sandbox_prep_not_ready",
            Self::NodeNotFound { .. } => "node_not_found",
        }
    }

    /// Statut HTTP. Sans joker : ajouter une variante ne compile plus tant qu'on n'a
    /// pas décidé de son statut.
    fn status(&self) -> StatusCode {
        match self {
            // Une cible absente du pipeline est une requête malformée, pas un conflit
            // d'état.
            Self::NodeNotFound { .. } => StatusCode::BAD_REQUEST,
            Self::RetryRejected { .. } | Self::SandboxPrepNotReady { .. } => StatusCode::CONFLICT,
        }
    }

    /// La session tmux du nœud a-t-elle déjà été tuée quand ce refus est parti ?
    /// Le bit qui compte sur les routes de (re)spawn par nœud (ADR-0037 §5). Sur la
    /// route de retry il est presque toujours `false` : la sonde de tête est le
    /// premier geste, avant le stop — c'est tout l'objet de #487.
    fn session_killed(&self) -> bool {
        match self {
            Self::RetryRejected { session_killed, .. }
            | Self::SandboxPrepNotReady { session_killed, .. } => *session_killed,
            // Structurellement pré-kill : la sonde de cible est en tête du handler.
            Self::NodeNotFound { .. } => false,
        }
    }

    /// Le détail spécifique, fusionné à plat dans le corps par [`retry_response`].
    fn detail(&self) -> serde_json::Value {
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

    /// Raison lisible pour le log.
    pub(crate) fn reason(&self) -> String {
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

/// L'**unique** projection d'un verdict de retry vers HTTP. Prend une **référence** :
/// le verdict est aussi logué par l'appelant, et rendre `Response` par valeur dans un
/// `Result::Err` déclencherait `clippy::result_large_err` (traité `-D warnings` en CI).
pub(crate) fn retry_response(v: &RetryVerdict) -> Response {
    match v {
        RetryVerdict::Spawned {
            node_id,
            iter,
            invalidated,
            reused_sub_worktree,
            base_sha,
            interrupted_git_ops,
        } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                // Contrat historique de la route, préservé pour le canvas + son client.
                "iter": iter,
                "invalidated": invalidated,
                // Vocabulaire ADR-0025, aligné sur `restart_response` : une liste de
                // paires, pas un booléen.
                "spawned": [{ "node_id": node_id, "iter": iter }],
                "reused_sub_worktree": reused_sub_worktree,
                "base_sha": base_sha,
                "interrupted_git_ops": interrupted_git_ops,
            })),
        )
            .into_response(),
        RetryVerdict::Waiting {
            reason,
            invalidated,
        } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "waiting": true,
                "reason": reason,
                "invalidated": invalidated,
            })),
        )
            .into_response(),
        RetryVerdict::Refused(r) => {
            let mut body = serde_json::json!({
                "error": r.slug(),
                // Uniformément `true` sur les refus de cette route (ADR-0037 §4) :
                // aucun refus n'enregistre d'issue terminale. La forme est déclarée
                // transversale par ADR-0035 §3 ; le champ redevient informatif sur le 500.
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
        RetryVerdict::Broken {
            message,
            run_failed,
        } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "spawn_failed",
                // Un 500 route la CLI vers `pdo fail`, conseil catastrophique si
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
    /// joker** : ajouter une variante à `RetryVerdict` sans l'échantillonner ici ne
    /// compile plus. Même garde-fou que `every_restart_verdict()` (#489).
    fn every_retry_verdict() -> Vec<RetryVerdict> {
        let all = vec![
            RetryVerdict::Spawned {
                node_id: "worker".into(),
                iter: 2,
                invalidated: vec!["reviewer".into()],
                reused_sub_worktree: true,
                base_sha: Some("abc123".into()),
                interrupted_git_ops: vec!["index.lock".into()],
            },
            RetryVerdict::Waiting {
                reason: "session cap reached (20/20 live)".into(),
                invalidated: vec!["reviewer".into()],
            },
            RetryVerdict::Refused(RetryRefusal::RetryRejected {
                message:
                    "run r is Failed: no scheduling on a non-running run — resume the run first"
                        .into(),
                session_killed: false,
            }),
            RetryVerdict::Refused(RetryRefusal::SandboxPrepNotReady {
                message: "sandbox prep is still building".into(),
                session_killed: false,
            }),
            RetryVerdict::Refused(RetryRefusal::NodeNotFound {
                node_id: "ghost".into(),
            }),
            RetryVerdict::Broken {
                message: "spawn aborted before start".into(),
                run_failed: true,
            },
        ];

        // Plancher de couverture : le `match` sans joker force à nommer chaque variante,
        // et le compte force l'échantillon à exister vraiment.
        let mut seen = std::collections::BTreeSet::new();
        for v in &all {
            let key = match v {
                RetryVerdict::Spawned { .. } => "Spawned",
                RetryVerdict::Waiting { .. } => "Waiting",
                RetryVerdict::Refused(r) => match r {
                    RetryRefusal::RetryRejected { .. } => "Refused/RetryRejected",
                    RetryRefusal::SandboxPrepNotReady { .. } => "Refused/SandboxPrepNotReady",
                    RetryRefusal::NodeNotFound { .. } => "Refused/NodeNotFound",
                },
                RetryVerdict::Broken { .. } => "Broken",
            };
            seen.insert(key);
        }
        assert_eq!(
            seen.len(),
            all.len(),
            "every_retry_verdict() must hold exactly one sample per variant"
        );
        all
    }

    /// **L'invariant du ticket.** Pas « jamais 2xx » (`RetryVerdict` mêle succès,
    /// sursis et pannes) mais la **totalité de la projection** : `Spawned`/`Waiting`
    /// sont `2xx`, tout le reste ne l'est jamais.
    #[test]
    fn a_spawn_that_did_not_happen_never_projects_to_a_2xx() {
        for v in every_retry_verdict() {
            let status = retry_response(&v).status();
            let spawn_happened = matches!(
                v,
                RetryVerdict::Spawned { .. } | RetryVerdict::Waiting { .. }
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

    #[test]
    fn every_variant_maps_to_its_exact_status() {
        let cases = [
            (
                RetryVerdict::Spawned {
                    node_id: "n".into(),
                    iter: 2,
                    invalidated: vec![],
                    reused_sub_worktree: false,
                    base_sha: None,
                    interrupted_git_ops: vec![],
                },
                StatusCode::OK,
            ),
            (
                RetryVerdict::Waiting {
                    reason: "r".into(),
                    invalidated: vec![],
                },
                StatusCode::OK,
            ),
            (
                RetryVerdict::Refused(RetryRefusal::RetryRejected {
                    message: "m".into(),
                    session_killed: false,
                }),
                StatusCode::CONFLICT,
            ),
            (
                RetryVerdict::Refused(RetryRefusal::SandboxPrepNotReady {
                    message: "m".into(),
                    session_killed: false,
                }),
                StatusCode::CONFLICT,
            ),
            (
                RetryVerdict::Refused(RetryRefusal::NodeNotFound {
                    node_id: "g".into(),
                }),
                StatusCode::BAD_REQUEST,
            ),
            (
                RetryVerdict::Broken {
                    message: "m".into(),
                    run_failed: false,
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (v, want) in cases {
            assert_eq!(retry_response(&v).status(), want, "{v:?}");
        }
    }

    async fn body_of(v: &RetryVerdict) -> serde_json::Value {
        let bytes = axum::body::to_bytes(retry_response(v).into_body(), usize::MAX)
            .await
            .expect("retry body");
        serde_json::from_slice(&bytes).expect("retry body is JSON")
    }

    /// Chaque refus porte son slug exact, `recoverable`, et `session_killed`.
    #[tokio::test]
    async fn every_refusal_body_carries_slug_recoverable_and_session_killed() {
        for v in every_retry_verdict() {
            let RetryVerdict::Refused(ref r) = v else {
                continue;
            };
            let body = body_of(&v).await;
            assert_eq!(body["error"].as_str(), Some(r.slug()), "{body}");
            assert_eq!(body["recoverable"], true, "{body}");
            assert_eq!(body["session_killed"], r.session_killed(), "{body}");
        }
    }

    /// La prose du garde part dans `message`, jamais dans `error` — et le slug
    /// `retry_refused` ne contient PAS « resume » : un client discrimine sur le slug,
    /// affiche la prose. C'est le contrat ADR-0035 §3 que le volet frontend consomme.
    #[tokio::test]
    async fn the_head_probe_prose_lands_in_message_not_error() {
        let body = body_of(&RetryVerdict::Refused(RetryRefusal::RetryRejected {
            message: "run r is Failed: no scheduling on a non-running run — resume the run first"
                .into(),
            session_killed: false,
        }))
        .await;
        assert_eq!(body["error"], "retry_refused");
        assert!(!body["error"].as_str().unwrap().contains("resume"));
        assert!(body["message"]
            .as_str()
            .unwrap()
            .contains("resume the run first"));
    }

    /// `Spawned` préserve le contrat racine `{iter, invalidated}` **et** ajoute les
    /// champs neufs — un client pré-#487 lisant `body.iter` / `body.invalidated`
    /// continue de marcher.
    #[tokio::test]
    async fn spawned_keeps_the_root_iter_and_invalidated_contract() {
        let body = body_of(&RetryVerdict::Spawned {
            node_id: "worker".into(),
            iter: 3,
            invalidated: vec!["reviewer".into(), "shipit".into()],
            reused_sub_worktree: true,
            base_sha: Some("deadbeef".into()),
            interrupted_git_ops: vec!["MERGE_HEAD".into()],
        })
        .await;
        assert_eq!(body["ok"], true);
        assert_eq!(body["iter"], 3);
        assert_eq!(
            body["invalidated"],
            serde_json::json!(["reviewer", "shipit"])
        );
        assert_eq!(body["spawned"][0]["node_id"], "worker");
        assert_eq!(body["spawned"][0]["iter"], 3);
        assert_eq!(body["reused_sub_worktree"], true);
        assert_eq!(body["base_sha"], "deadbeef");
        assert_eq!(
            body["interrupted_git_ops"],
            serde_json::json!(["MERGE_HEAD"])
        );
    }

    /// `Waiting` est un `2xx` et n'est pas un `noop` (ADR-0037 §2) — et il porte
    /// quand même la liste `invalidated` (les artefacts ont bien été purgés avant que
    /// le cap ne mette le nœud en file).
    #[tokio::test]
    async fn waiting_is_a_2xx_and_not_a_noop() {
        let body = body_of(&RetryVerdict::Waiting {
            reason: "session cap reached".into(),
            invalidated: vec!["reviewer".into()],
        })
        .await;
        assert_eq!(body["ok"], true);
        assert_eq!(body["waiting"], true);
        assert!(body["reason"].is_string());
        assert_eq!(body["invalidated"], serde_json::json!(["reviewer"]));
        assert!(
            body.get("noop").is_none(),
            "a reservation that flipped the node's status is not a no-op: {body}"
        );
    }

    /// `recoverable` dérive de `run_failed` sur la panne — le seul endroit de la route
    /// où le champ porte un bit.
    #[tokio::test]
    async fn broken_derives_recoverable_from_run_failed() {
        for run_failed in [true, false] {
            let body = body_of(&RetryVerdict::Broken {
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

    /// `node_not_found` est un 400 (requête malformée), pas un 409, et son corps nomme
    /// la cible.
    #[tokio::test]
    async fn node_not_found_is_a_400_naming_the_target() {
        let v = RetryVerdict::Refused(RetryRefusal::NodeNotFound {
            node_id: "ghost".into(),
        });
        assert_eq!(retry_response(&v).status(), StatusCode::BAD_REQUEST);
        let body = body_of(&v).await;
        assert_eq!(body["error"], "node_not_found");
        assert_eq!(body["node_id"], "ghost");
        assert!(body["message"].as_str().unwrap().contains("ghost"));
    }
}
