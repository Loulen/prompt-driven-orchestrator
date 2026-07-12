# ADR-0024 — Forget durable : tombstone des runs oubliés

- **Statut** : accepté
- **Date** : 2026-07-09
- **Issue** : #328

## Contexte

Les runs sont projetés depuis l'event log (`SELECT DISTINCT run_id FROM events`). Le `forget` (`DELETE /runs/<id>`, autorisé sur un run `archived`) faisait uniquement `DELETE FROM events WHERE run_id = ?`. Tout événement écrit **après** ce delete pour le même `run_id` recréait un `DISTINCT run_id` sans métadonnées : `project()` fabriquait alors un `RunState` fantôme (`status: running`, `pipeline_name: ""`, `started_at: null`), republié aussitôt par `GET /runs` — inarchivable et non re-forgettable (forget exige `Archived`).

Écrivains tardifs réels :

1. **Session manager/zombie tmux** : `pdo-mgr-<id>` ou une session orpheline qui POSTe `/runs/<id>/commands` — l'arm `extend_cycle` appendait `CommandIssued` avant tout check d'existence.
2. **Tail détaché post-#304 (ADR-0023)** : `detach_terminal_tail` est un `tokio::spawn` non traqué ; son reap + `advance_run` peut appender `NodeStarted`/`RunCompleted`/etc. après un forget concurrent.

## Décision

1. **Table `forgotten_runs`** (`run_id TEXT PRIMARY KEY, forgotten_at TEXT`), créée au boot (`CREATE TABLE IF NOT EXISTS`, pattern ADR-0015). `forget_run` insère le tombstone **et** purge les events dans **une même transaction sqlx** (première transaction du daemon) : aucun interleaving où les events sont partis mais le tombstone absent.
2. **Garde dans `append_event`, tous kinds** : l'INSERT unique devient `INSERT … SELECT … WHERE NOT EXISTS (SELECT 1 FROM forgotten_runs WHERE run_id = ?)` ; `rows_affected == 0` → `Err("run <id> has been forgotten")`. Un seul statement : pas de fenêtre TOCTOU face à un forget concurrent. Les émetteurs (tail détaché, stale detector, `let _ = append_event(...)`) loggent l'erreur et continuent — ni panic ni retry.
3. **410 Gone aux frontières HTTP** : `run_command` et `node_done` pré-vérifient le tombstone en tête de handler, avant tout side-effect (merge de sub-worktree en particulier).
4. **Projection durcie** : `project()` retourne `None` si le log ne contient aucun `RunStarted` (`started_at` jamais posé). Les 43 call sites production sont None-tolerants ; aucun run légitime n'existe sans `RunStarted` (appendé en premier à la création, avant worktree et scheduling).
5. **Kill best-effort des sessions** au forget : `pdo-mgr-<id>` et `pdo-shell-<id>` sont tuées (un run oublié n'a plus rien à récupérer — le « managers persist by design » ne vaut que pour un run dont le log existe).

Conséquence assumée : un `run_id` oublié n'est **jamais réutilisable** (le tombstone bloque aussi `RunStarted`). Les run_id sont horodatés ; la collision est impraticable.

## Alternatives rejetées

- **Projection-only** (ne durcir que `project()`) : les events orphelins s'accumulent silencieusement en base pour un run censé avoir « entièrement disparu ».
- **Event-tombstone conservé dans `events`** (ex. `RunForgotten`) : contredit le contrat de purge du forget et garde le `run_id` dans les `DISTINCT`.
- **`RunStatus::Invalid`** pour les fragments : churn d'exhaustive-match sur tous les consommateurs pour représenter un état qui ne devrait pas exister.

## Interactions

- **ADR-0023 (tail détaché)** : le tail est précisément le second écrivain tardif ; son `Err` d'append est loggé par le `catch_unwind`/logging existant, sans panic ni retry.
- **#212 (transition guard)** : `validate_transition(None, _) == Allow` — après durcissement de `project()`, c'est le tombstone qui bloque, pas le guard.
- **ADR-0020 (forget purge aussi `~/.pdo/runs/<id>`)** : inchangé, conservé après la transaction.
