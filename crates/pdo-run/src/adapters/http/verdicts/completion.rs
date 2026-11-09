//! The HTTP projection of a [`CompletionRefusal`]: its status and its body.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};

use crate::domain::completion_refusal::*;

/// Statut HTTP, **jamais** `2xx`. Volontairement **sans joker** : ajouter une
/// variante ne compile plus tant qu'on n'a pas décidé de son statut.
fn status(r: &CompletionRefusal) -> StatusCode {
    match r {
        // ADR-0024 §3 : le tombstone garde son 410.
        CompletionRefusal::RunForgotten { .. } => StatusCode::GONE,
        CompletionRefusal::RunNotFound => StatusCode::NOT_FOUND,
        // Pannes, pas verdicts.
        CompletionRefusal::Internal { .. }
        | CompletionRefusal::DeliveryFailed { .. }
        | CompletionRefusal::AppendFailed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        CompletionRefusal::CompletionRejected { .. }
        | CompletionRefusal::CompletionNotReleased
        | CompletionRefusal::MergeConflict { .. }
        | CompletionRefusal::MissingOutputs { .. }
        | CompletionRefusal::ScriptValidationFailed { .. }
        | CompletionRefusal::FrontmatterRetryPending { .. }
        | CompletionRefusal::FrontmatterRetryExhausted { .. }
        | CompletionRefusal::SecondaryRepoDirtied { .. }
        | CompletionRefusal::ChildrenPending { .. } => StatusCode::CONFLICT,
    }
}

/// L'**unique** projection d'un refus vers HTTP : les deux appelants du chemin de
/// complétion passent par elle.
///
/// Prend une **référence** : le refus est aussi logué par son appelant, et
/// `Response` (128 octets) rendu par valeur dans un `Result::Err` déclencherait
/// `clippy::result_large_err`, que la CI traite en `-D warnings`. Toute la chaîne
/// en amont circule donc en `Option<CompletionRefusal>` (~40 octets pour la plus
/// grosse variante), jamais en `Result<_, Response>`.
pub fn refusal_response(r: &CompletionRefusal) -> Response {
    let mut body = serde_json::json!({
        "error": r.slug(),
        "recoverable": r.recoverable(),
    });
    if let (Some(obj), Some(extra)) = (body.as_object_mut(), r.detail().as_object()) {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    (status(r), Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violation() -> serde_json::Value {
        serde_json::json!({ "port": "review", "field": "verdict", "reason": "not in enum" })
    }

    /// Un échantillon par variante, produit derrière un `match` **exhaustif sans
    /// joker** : ajouter une variante à `CompletionRefusal` sans l'échantillonner
    /// ici ne compile plus.
    fn every_refusal() -> Vec<CompletionRefusal> {
        let all = vec![
            CompletionRefusal::RunForgotten {
                run_id: "r1".into(),
            },
            CompletionRefusal::RunNotFound,
            CompletionRefusal::Internal {
                error: "failed to load events: db is locked".into(),
            },
            CompletionRefusal::CompletionRejected {
                message: "resume the run first".into(),
            },
            CompletionRefusal::CompletionNotReleased,
            CompletionRefusal::DeliveryFailed {
                node_id: "impl".into(),
                error: "git add -A failed".into(),
            },
            CompletionRefusal::MergeConflict {
                node_id: "impl".into(),
            },
            CompletionRefusal::MissingOutputs {
                missing: vec!["review".into()],
            },
            CompletionRefusal::ScriptValidationFailed {
                detail: serde_json::json!({ "kind": "missing_outputs", "missing": ["out"] }),
            },
            CompletionRefusal::FrontmatterRetryPending {
                violations: vec![violation()],
            },
            CompletionRefusal::FrontmatterRetryExhausted {
                violations: vec![violation()],
            },
            CompletionRefusal::SecondaryRepoDirtied {
                alias: "repoB".into(),
                message: "tracked files modified in the read-only snapshot".into(),
            },
            CompletionRefusal::AppendFailed {
                error: "disk full".into(),
            },
            CompletionRefusal::ChildrenPending {
                node_id: "orch".into(),
                active: 2,
                failed: 1,
            },
        ];

        // Plancher de couverture : le `match` sans joker force à nommer chaque
        // variante, et le compte force l'échantillon à exister vraiment.
        let mut seen = std::collections::BTreeSet::new();
        for r in &all {
            let key = match r {
                CompletionRefusal::RunForgotten { .. } => "RunForgotten",
                CompletionRefusal::RunNotFound => "RunNotFound",
                CompletionRefusal::Internal { .. } => "Internal",
                CompletionRefusal::CompletionRejected { .. } => "CompletionRejected",
                CompletionRefusal::CompletionNotReleased => "CompletionNotReleased",
                CompletionRefusal::DeliveryFailed { .. } => "DeliveryFailed",
                CompletionRefusal::MergeConflict { .. } => "MergeConflict",
                CompletionRefusal::MissingOutputs { .. } => "MissingOutputs",
                CompletionRefusal::ScriptValidationFailed { .. } => "ScriptValidationFailed",
                CompletionRefusal::FrontmatterRetryPending { .. } => "FrontmatterRetryPending",
                CompletionRefusal::FrontmatterRetryExhausted { .. } => "FrontmatterRetryExhausted",
                CompletionRefusal::SecondaryRepoDirtied { .. } => "SecondaryRepoDirtied",
                CompletionRefusal::AppendFailed { .. } => "AppendFailed",
                CompletionRefusal::ChildrenPending { .. } => "ChildrenPending",
            };
            seen.insert(key);
        }
        assert_eq!(
            seen.len(),
            all.len(),
            "every_refusal() must hold exactly one sample per variant"
        );
        all
    }

    #[test]
    fn a_refusal_never_projects_to_a_2xx() {
        for r in every_refusal() {
            let status = refusal_response(&r).status();
            assert!(
                !status.is_success(),
                "{} projected to a 2xx ({status})",
                r.slug()
            );
            assert!(
                status.is_client_error() || status.is_server_error(),
                "{} projected to a non-error status ({status})",
                r.slug()
            );
        }
    }

    /// Une variante qui oublierait `recoverable` ferait sortir `pdo complete` en
    /// `1` au lieu de `3`/`4`, **silencieusement** — d'où l'assertion de type.
    #[tokio::test]
    async fn every_refusal_body_carries_error_and_recoverable() {
        for r in every_refusal() {
            let resp = refusal_response(&r);
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .expect("refusal body");
            let body: serde_json::Value =
                serde_json::from_slice(&bytes).expect("refusal body is JSON");
            assert!(
                body["error"].is_string(),
                "{} has no string `error`: {body}",
                r.slug()
            );
            assert_eq!(body["error"].as_str(), Some(r.slug()));
            assert!(
                body["recoverable"].is_boolean(),
                "{} has no boolean `recoverable`: {body}",
                r.slug()
            );
        }
    }

    /// `recoverable` arbitre entre exit `3` et exit `4` de `pdo complete`.
    #[test]
    fn only_still_your_turn_refusals_are_recoverable() {
        for r in every_refusal() {
            let expected = matches!(
                r,
                CompletionRefusal::MissingOutputs { .. }
                    | CompletionRefusal::CompletionNotReleased
                    | CompletionRefusal::FrontmatterRetryPending { .. }
                    // La liaison forte laisse la main à l'agent tant qu'aucun
                    // enfant n'a échoué (#724).
                    | CompletionRefusal::ChildrenPending { failed: 0, .. }
                    // A dirtied read-only secondary leaves the node alive and
                    // nothing recorded: revert, then complete again.
                    | CompletionRefusal::SecondaryRepoDirtied { .. }
            );
            assert_eq!(r.recoverable(), expected, "{} recoverable()", r.slug());
        }
        // Avec un enfant `failed`, la décision est à l'utilisateur (exit 4).
        let failed_child = CompletionRefusal::ChildrenPending {
            node_id: "orch".into(),
            active: 1,
            failed: 1,
        };
        assert!(!failed_child.recoverable());
    }

    /// Le refus de liaison compte : le message porte les deux compteurs (#724).
    #[test]
    fn children_pending_message_counts_active_and_failed() {
        let r = CompletionRefusal::ChildrenPending {
            node_id: "orch".into(),
            active: 2,
            failed: 0,
        };
        let reason = r.reason();
        assert!(reason.contains("2 active"), "{reason}");
        assert!(!reason.contains("failed"), "{reason}");

        let r = CompletionRefusal::ChildrenPending {
            node_id: "orch".into(),
            active: 1,
            failed: 3,
        };
        let reason = r.reason();
        assert!(reason.contains("1 active"), "{reason}");
        assert!(reason.contains("3 failed"), "{reason}");
    }

    /// Le refus vaut pour tout nœud parent : il ne dit jamais « orchestrator »
    /// et indique lui-même le geste d'attente, pour un agent qui n'a pas le
    /// skill `pdo-orchestrate`. La variante `failed` renvoie à l'utilisateur,
    /// pas à `pdo run wait`.
    #[test]
    fn children_pending_message_is_generic_and_names_pdo_run_wait() {
        let active = CompletionRefusal::ChildrenPending {
            node_id: "worker".into(),
            active: 2,
            failed: 0,
        };
        assert_eq!(
            active.reason(),
            "node `worker` is held by its child runs: 2 active — wait for them with \
             `pdo run wait --all`, then complete again"
        );
        // The HTTP body carries the same sentence.
        assert_eq!(
            active.detail()["message"],
            serde_json::json!(active.reason())
        );

        let failed = CompletionRefusal::ChildrenPending {
            node_id: "worker".into(),
            active: 0,
            failed: 1,
        }
        .reason();
        assert_eq!(
            failed,
            "node `worker` is held by its child runs: 0 active, 1 failed — the node is \
             awaiting the user: retry the failed child run or force-complete the node"
        );
        for message in [active.reason(), failed] {
            assert!(!message.contains("orchestrator"), "{message}");
        }
    }

    /// « Jamais `2xx` » **≠** « toujours `409` ».
    #[test]
    fn non_conflict_statuses_are_preserved() {
        let cases = [
            (
                CompletionRefusal::RunForgotten { run_id: "r".into() },
                StatusCode::GONE,
            ),
            (CompletionRefusal::RunNotFound, StatusCode::NOT_FOUND),
            (
                CompletionRefusal::Internal { error: "e".into() },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                CompletionRefusal::DeliveryFailed {
                    node_id: "n".into(),
                    error: "e".into(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                CompletionRefusal::AppendFailed { error: "e".into() },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (r, want) in cases {
            assert_eq!(refusal_response(&r).status(), want, "{}", r.slug());
        }
    }

    /// Le détail est **verbatim celui d'avant #490** : `missing` reste une liste
    /// plate, `detail` reste imbriqué (ADR-0035 §5), `violations` garde sa forme.
    #[tokio::test]
    async fn detail_fields_keep_their_pre_490_shape() {
        async fn body_of(r: CompletionRefusal) -> serde_json::Value {
            let bytes = axum::body::to_bytes(refusal_response(&r).into_body(), usize::MAX)
                .await
                .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        }

        let missing = body_of(CompletionRefusal::MissingOutputs {
            missing: vec!["review".into()],
        })
        .await;
        assert_eq!(missing["missing"], serde_json::json!(["review"]));

        let script = body_of(CompletionRefusal::ScriptValidationFailed {
            detail: serde_json::json!({ "kind": "missing_outputs", "missing": ["out"] }),
        })
        .await;
        assert_eq!(script["detail"]["kind"], "missing_outputs");
        assert_eq!(script["detail"]["missing"], serde_json::json!(["out"]));
        // Pas aplati : une trace d'audit de fail-fast reste distinguable d'un
        // échec après retry.
        assert!(script.get("missing").is_none());

        let exhausted = body_of(CompletionRefusal::FrontmatterRetryExhausted {
            violations: vec![violation()],
        })
        .await;
        assert_eq!(exhausted["violations"][0]["field"], "verdict");
    }

    /// Sinon le client relit le `409` comme `missing_outputs` à liste vide et
    /// n'affiche **rien**.
    #[tokio::test]
    async fn the_transition_guard_prose_lands_in_message() {
        let r = CompletionRefusal::CompletionRejected {
            message: "run r1 is failed; resume the run first".into(),
        };
        let resp = refusal_response(&r);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "completion_rejected");
        assert_eq!(body["recoverable"], false);
        assert!(body["message"]
            .as_str()
            .unwrap()
            .contains("resume the run first"));
    }
}
