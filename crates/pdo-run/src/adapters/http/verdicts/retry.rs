//! The HTTP projection of a [`RetryRefusal`]: its status and its body.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};

use crate::domain::retry_verdict::*;

/// Statut HTTP. Sans joker : ajouter une variante ne compile plus tant qu'on n'a
/// pas décidé de son statut.
fn status(r: &RetryRefusal) -> StatusCode {
    match r {
        // Requête malformée, pas un conflit d'état.
        RetryRefusal::NodeNotFound { .. } => StatusCode::BAD_REQUEST,
        RetryRefusal::RetryRejected { .. } | RetryRefusal::SandboxPrepNotReady { .. } => {
            StatusCode::CONFLICT
        }
    }
}

/// L'**unique** projection d'un verdict vers HTTP. Prend une **référence** :
/// le verdict est aussi logué par l'appelant, et rendre `Response` par valeur dans un
/// `Result::Err` déclencherait `clippy::result_large_err` (traité `-D warnings` en CI).
pub fn retry_response(v: &RetryVerdict) -> Response {
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
                // Vocabulaire ADR-0025, aligné sur `restart_response` : une liste
                // de paires, pas un booléen.
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
                // aucun refus n'enregistre d'issue terminale. Le champ ne redevient
                // informatif que sur le 500.
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
    /// compile plus.
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

    /// Pas « jamais 2xx » (`RetryVerdict` mêle succès, sursis et pannes) mais la
    /// **totalité** de la projection.
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

    /// Un client discrimine sur le slug et affiche la prose (ADR-0035 §3, contrat
    /// consommé par le frontend) : le slug ne doit donc pas porter « resume ».
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

    /// Un client pré-#487 lisant `body.iter` / `body.invalidated` doit continuer de
    /// marcher.
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

    /// Il porte quand même `invalidated` : les artefacts ont été purgés avant que
    /// le cap ne mette le nœud en file (ADR-0037 §2).
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

    /// Le seul endroit de la route où `recoverable` porte un bit.
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
