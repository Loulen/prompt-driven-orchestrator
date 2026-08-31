//! Pourquoi une complétion a été refusée, et comment ce refus se projette sur le
//! wire (#490, ADR-0035).
//!
//! Le type ne porte **aucun statut** : la projection ([`refusal_response`]) en est
//! la seule propriétaire, donc « un refus qui répond 2xx » est inexprimable. C'est
//! l'invariant du ticket, posé comme propriété du type et non comme liste de bras
//! à relire — huit des dix-neuf sorties du chemin de complétion répondaient `200`
//! sur un refus, dont quatre après avoir appendé `RunFailed`.
//!
//! Les **deux** appelants du corps de complétion (`POST …/nodes/:id/done` et
//! `POST /runs/:id/commands` `kind=mark_node_done`) passent par la même
//! projection : c'est ce qui fait que l'invariant couvre les deux surfaces, là où
//! un garde posé sur `CompletionAttempt` n'aurait couvert que la première (le bras
//! `mark_node_done` n'en construit jamais).

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};

/// La cause d'un refus de complétion, avec son détail verbatim d'avant #490.
///
/// `Clone` parce que le corps partagé rend la variante à ses deux appelants et que
/// l'un d'eux la loge avant de la projeter.
#[derive(Debug, Clone)]
pub(crate) enum CompletionRefusal {
    /// Run tombstoné (#328 / ADR-0024). Garde son `410` : « jamais 2xx » ne veut
    /// pas dire « toujours 409 ».
    RunForgotten { run_id: String },
    /// La projection ne rend aucun Run pour cet id — cible inconnue, `404`.
    RunNotFound,
    /// Panne interne **avant** tout verdict (lecture du log, contrôle de tombstone).
    /// `500` : ce n'est pas un refus argumenté, c'est un daemon qui n'a pas pu
    /// décider — et c'est le seul cas où `pdo fail` reste le bon conseil.
    Internal { error: String },
    /// Garde de transition (#212 / #354). La prose part dans `message`, `error`
    /// porte le slug : c'est ce qui rend le refus lisible côté client, qui relisait
    /// jusqu'ici *tout* `409` comme `missing_outputs`.
    CompletionRejected { message: String },
    /// La livraison (#654 / ADR-0060) a échoué — staging, commit ou merge. Panne,
    /// pas verdict, donc `500`. Un `NodeInterrupted` est déjà appendé et le Run
    /// est parqué `AwaitingUser` : le travail reste sur disque, intact.
    DeliveryFailed { node_id: String, error: String },
    /// Conflit de merge ; `MergeConflictDetected` + `RunFailed` déjà appendés.
    MergeConflict { node_id: String },
    /// Ports de sortie déclarés sans artefact. Le node reste vivant.
    MissingOutputs { missing: Vec<String> },
    /// Fail-fast d'un node `script` (ADR-0017) ; `NodeFailed` + `RunFailed`
    /// appendés. Le `detail` reste **imbriqué** : aplatir rendrait la trace
    /// d'audit indistinguable d'un échec après retry (ADR-0035 §5).
    ScriptValidationFailed { detail: serde_json::Value },
    /// Mismatch de frontmatter, message correctif envoyé, node toujours `running`.
    FrontmatterRetryPending { violations: Vec<serde_json::Value> },
    /// Un nœud a modifié un fichier **suivi** d'un dépôt secondaire read-only
    /// (#465, ADR-0042). Garde de complétion, **pas** une panne : aucun événement
    /// terminal n'est appendé (contrairement à `DeliveryFailed`), le nœud reste
    /// vivant, l'agent nettoie le secondaire (`git checkout`) et re-complète.
    /// `recoverable:false` par choix de #465 — le read-only d'un secondaire est un
    /// contrat, non un retry de sortie ; le slug (jamais le seul statut) discrimine.
    SecondaryRepoDirtied { alias: String, message: String },
    /// Mismatch après l'unique retry ; `NodeFailed` + `RunFailed` appendés.
    FrontmatterRetryExhausted { violations: Vec<serde_json::Value> },
    /// L'événement terminal n'a pas pu être appendé — panne, `500`.
    AppendFailed { error: String },
    /// Résolveur de merge : la résolution n'est pas valide ; `MergeResolverFailed`
    /// + `RunFailed` appendés.
    MergeResolutionFailed { reason: String },
    /// Résolveur de merge spawné. **INATTEIGNABLE** en production :
    /// `MergeResult::ConflictPendingResolution` n'est construit que sous
    /// `keep_conflict == true`, qu'aucun appelant de production ne passe, et
    /// ADR-0006 a retiré le résolveur automatique. Conservé sous le type le temps
    /// de la retombée d'ADR-0006, qui supprimera le sous-système entier — coût de
    /// test nul ici, et le supprimer sous un fix de bug mélangerait deux
    /// intentions (ADR-0035 §6).
    MergeResolverSpawned { node_id: String },
    /// Résolveur de merge : le spawn a échoué. **INATTEIGNABLE**, cf. ci-dessus.
    MergeResolverFailed { reason: String },
}

impl CompletionRefusal {
    /// Le slug stable sur lequel les clients discriminent. **Jamais** le statut :
    /// un statut n'a pas assez de bits pour neuf causes.
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            Self::RunForgotten { .. } => "run_forgotten",
            Self::RunNotFound => "run_not_found",
            Self::Internal { .. } => "internal_error",
            Self::CompletionRejected { .. } => "completion_rejected",
            Self::DeliveryFailed { .. } => "delivery_failed",
            Self::MergeConflict { .. } => "merge_conflict",
            Self::MissingOutputs { .. } => "missing_outputs",
            Self::ScriptValidationFailed { .. } => "script_validation_failed",
            Self::FrontmatterRetryPending { .. } => "frontmatter_retry_pending",
            Self::FrontmatterRetryExhausted { .. } => "frontmatter_retry_exhausted",
            Self::SecondaryRepoDirtied { .. } => "secondary_repo_dirtied",
            Self::AppendFailed { .. } => "append_failed",
            Self::MergeResolutionFailed { .. } => "merge_resolution_failed",
            Self::MergeResolverSpawned { .. } => "merge_resolver_spawned",
            Self::MergeResolverFailed { .. } => "merge_resolver_failed",
        }
    }

    /// Est-ce encore le tour de l'appelant ? `false` ⇒ le daemon a déjà enregistré
    /// une issue terminale : l'appelant ne doit **rien** appender de plus, et
    /// surtout pas enchaîner `pdo fail`.
    ///
    /// Deux variantes seulement laissent le node vivant, et ce sont les deux bras
    /// du même `match` sur `ValidationError` qui n'appendent aucun événement
    /// d'échec.
    pub(crate) fn recoverable(&self) -> bool {
        matches!(
            self,
            Self::MissingOutputs { .. } | Self::FrontmatterRetryPending { .. }
        )
    }

    /// Statut HTTP. Énumération fermée, **jamais** `2xx` — c'est ici que
    /// l'invariant se tient, et `a_refusal_never_projects_to_a_2xx` le prouve
    /// variante par variante.
    ///
    /// Volontairement **sans joker** : ajouter une variante ne compile plus tant
    /// qu'on n'a pas décidé de son statut (patron du dispatch d'`event_log`).
    fn status(&self) -> StatusCode {
        match self {
            // ADR-0024 §3 : le tombstone garde son 410.
            Self::RunForgotten { .. } => StatusCode::GONE,
            Self::RunNotFound => StatusCode::NOT_FOUND,
            // Pannes, pas verdicts.
            Self::Internal { .. } | Self::DeliveryFailed { .. } | Self::AppendFailed { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::CompletionRejected { .. }
            | Self::MergeConflict { .. }
            | Self::MissingOutputs { .. }
            | Self::ScriptValidationFailed { .. }
            | Self::FrontmatterRetryPending { .. }
            | Self::FrontmatterRetryExhausted { .. }
            | Self::SecondaryRepoDirtied { .. }
            | Self::MergeResolutionFailed { .. }
            | Self::MergeResolverSpawned { .. }
            | Self::MergeResolverFailed { .. } => StatusCode::CONFLICT,
        }
    }

    /// Le détail spécifique, **verbatim celui d'avant #490** : aucun champ renommé,
    /// aucun champ aplati. Fusionné à plat dans le corps par [`refusal_response`].
    fn detail(&self) -> serde_json::Value {
        match self {
            Self::RunForgotten { run_id } => {
                serde_json::json!({ "message": format!("run {run_id} has been forgotten") })
            }
            Self::RunNotFound => serde_json::json!({ "message": "run not found" }),
            Self::Internal { error } => serde_json::json!({ "message": error }),
            Self::CompletionRejected { message } => serde_json::json!({ "message": message }),
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
            Self::MergeResolutionFailed { reason } | Self::MergeResolverFailed { reason } => {
                serde_json::json!({ "reason": reason })
            }
            Self::MergeResolverSpawned { node_id } => serde_json::json!({
                "message": format!("merge conflict on {node_id}: resolver spawned")
            }),
        }
    }

    /// Raison lisible pour le log et pour la veille de vivacité, qui lit la
    /// variante et jamais le statut (#469 / ADR-0032 — c'est pourquoi elle est
    /// indemne de ce lot).
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::RunForgotten { run_id } => format!("run {run_id} has been forgotten"),
            Self::RunNotFound => "run not found".into(),
            Self::Internal { error } => error.clone(),
            Self::CompletionRejected { message } => message.clone(),
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
            Self::MergeResolutionFailed { reason } => format!("merge resolution failed: {reason}"),
            Self::MergeResolverSpawned { node_id } => {
                format!("merge conflict on {node_id}: resolver spawned")
            }
            Self::MergeResolverFailed { reason } => {
                format!("merge resolver spawn failed: {reason}")
            }
        }
    }
}

/// L'**unique** projection d'un refus vers HTTP.
///
/// Les deux appelants du chemin de complétion passent par elle — c'est ce qui fait
/// que l'invariant couvre `POST …/done` *et* `POST /runs/:id/commands`.
///
/// Prend une **référence** : le refus est aussi logué par son appelant, et
/// `Response` (128 octets) rendu par valeur dans un `Result::Err` déclencherait
/// `clippy::result_large_err`, que la CI traite en `-D warnings`. Toute la chaîne
/// en amont circule donc en `Option<CompletionRefusal>` (~40 octets pour la plus
/// grosse variante), jamais en `Result<_, Response>`.
pub(crate) fn refusal_response(r: &CompletionRefusal) -> Response {
    let mut body = serde_json::json!({
        "error": r.slug(),
        "recoverable": r.recoverable(),
    });
    if let (Some(obj), Some(extra)) = (body.as_object_mut(), r.detail().as_object()) {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    (r.status(), Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violation() -> serde_json::Value {
        serde_json::json!({ "port": "review", "field": "verdict", "reason": "not in enum" })
    }

    /// Un échantillon par variante, produit derrière un `match` **exhaustif sans
    /// joker** : ajouter une variante à `CompletionRefusal` sans l'échantillonner
    /// ici ne compile plus. Même garde-fou que le dispatch d'`event_log`.
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
            CompletionRefusal::MergeResolutionFailed {
                reason: "still conflicted".into(),
            },
            CompletionRefusal::MergeResolverSpawned {
                node_id: "impl".into(),
            },
            CompletionRefusal::MergeResolverFailed {
                reason: "spawn failed".into(),
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
                CompletionRefusal::DeliveryFailed { .. } => "DeliveryFailed",
                CompletionRefusal::MergeConflict { .. } => "MergeConflict",
                CompletionRefusal::MissingOutputs { .. } => "MissingOutputs",
                CompletionRefusal::ScriptValidationFailed { .. } => "ScriptValidationFailed",
                CompletionRefusal::FrontmatterRetryPending { .. } => "FrontmatterRetryPending",
                CompletionRefusal::FrontmatterRetryExhausted { .. } => "FrontmatterRetryExhausted",
                CompletionRefusal::SecondaryRepoDirtied { .. } => "SecondaryRepoDirtied",
                CompletionRefusal::AppendFailed { .. } => "AppendFailed",
                CompletionRefusal::MergeResolutionFailed { .. } => "MergeResolutionFailed",
                CompletionRefusal::MergeResolverSpawned { .. } => "MergeResolverSpawned",
                CompletionRefusal::MergeResolverFailed { .. } => "MergeResolverFailed",
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

    /// **L'invariant du ticket, en une boucle.** Et il couvre les deux surfaces,
    /// puisque les deux appelants passent par `refusal_response`.
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

    /// `recoverable: true` veut dire « rien de terminal n'a été enregistré ». Les
    /// deux seules variantes qui laissent le node vivant sont épinglées ici, parce
    /// que c'est la valeur dont dépend le choix entre exit `3` et exit `4`.
    #[test]
    fn only_the_two_still_your_turn_refusals_are_recoverable() {
        for r in every_refusal() {
            let expected = matches!(
                r,
                CompletionRefusal::MissingOutputs { .. }
                    | CompletionRefusal::FrontmatterRetryPending { .. }
            );
            assert_eq!(r.recoverable(), expected, "{} recoverable()", r.slug());
        }
    }

    /// « Jamais `2xx` » **≠** « toujours `409` » : le tombstone d'ADR-0024, la
    /// cible inconnue et les deux pannes gardent leur statut historique.
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

    /// La prose du garde de transition part dans `message`, jamais dans `error` :
    /// c'est ce qui laissait le client relire *tout* `409` comme `missing_outputs`
    /// avec une liste vide, donc n'afficher **rien**.
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
