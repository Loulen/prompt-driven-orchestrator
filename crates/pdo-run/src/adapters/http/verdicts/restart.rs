//! The HTTP projection of a [`RestartRefusal`]: its status and its body.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};

use crate::domain::restart_verdict::*;

/// Statut HTTP. Sans joker : ajouter une variante ne compile plus tant qu'on n'a
/// pas décidé de son statut.
fn status(r: &RestartRefusal) -> StatusCode {
    match r {
        // Une cible que le Run ne connaît pas : `404`, comme sur tous les gestes
        // d'un node (le client en fait un « introuvable »).
        RestartRefusal::NodeNotFound { .. } => StatusCode::NOT_FOUND,
        RestartRefusal::RestartRejected { .. }
        | RestartRefusal::SandboxPrepNotReady { .. }
        | RestartRefusal::SubWorktreeOccupied { .. } => StatusCode::CONFLICT,
    }
}

/// L'**unique** projection d'un verdict de restart vers HTTP.
///
/// Prend une **référence** : le verdict est aussi logué par son appelant. Ne pas
/// renvoyer la `Response` par valeur dans un `Result::Err` — `clippy::result_large_err`,
/// traité en `-D warnings` par la CI.
pub fn restart_response(v: &RestartVerdict) -> Response {
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
            (status(r), Json(body)).into_response()
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
                StatusCode::NOT_FOUND,
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
