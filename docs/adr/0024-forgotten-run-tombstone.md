# ADR-0024 — Forget durable : tombstone des runs oubliés

- **Statut** : accepté
- **Date** : 2026-07-09
- **Issue** : #328

**Un `forget` ne peut pas être un simple `DELETE FROM events` : il faut un tombstone durable, sinon tout écrivain tardif ressuscite le run.** Les runs sont projetés depuis l'event log (`SELECT DISTINCT run_id`) ; un event écrit après la purge recréait un `DISTINCT run_id` sans métadonnées, et la projection fabriquait un `RunState` fantôme (`running`, pas de `pipeline_name`, pas de `started_at`) — inarchivable **et** non re-forgettable, puisque forget exige `Archived`. Écrivains tardifs réels : une session manager/zombie tmux qui POSTe `/commands`, et le **tail détaché** d'ADR-0023 (`tokio::spawn` non traqué, dont le reap peut appender après un forget concurrent).

## Décision

1. **Table `forgotten_runs`.** `forget_run` insère le tombstone **et** purge les events dans **une même transaction** : aucun interleaving où les events sont partis mais le tombstone absent.
2. **Garde dans `append_event`, tous kinds**, en **un seul statement** (`INSERT … SELECT … WHERE NOT EXISTS`) : pas de fenêtre TOCTOU face à un forget concurrent. Les émetteurs loggent l'erreur et continuent — **ni panic ni retry**.
3. **410 Gone aux frontières HTTP** : `run_command`, `node_done` et `PATCH /runs/{id}/repos` pré-vérifient le tombstone **en tête de handler, avant tout side-effect** (le merge de sub-worktree en particulier). Toute autre surface répond 404 sur un run oublié — le log est purgé, il n'y a rien à distinguer.
4. **Projection durcie** : `project()` retourne `None` si le log ne contient aucun `RunStarted`. Tout call site production est None-tolerant ; aucun run légitime n'existe sans `RunStarted`, appendé avant worktree et scheduling.
5. **Kill best-effort des sessions** au forget : le « managers persist by design » ne vaut que pour un run dont le log existe.

Conséquence assumée : un `run_id` oublié n'est **jamais réutilisable** (le tombstone bloque aussi `RunStarted`). Les run_id sont horodatés, la collision est impraticable.

## Alternatives rejetées

- **Projection-only** (ne durcir que `project()`) : les events orphelins s'accumulent silencieusement en base pour un run censé avoir entièrement disparu.
- **Event-tombstone dans `events`** (un `RunForgotten`) : contredit le contrat de purge et garde le `run_id` dans les `DISTINCT`.
- **`RunStatus::Invalid`** : churn d'exhaustive-match sur tous les consommateurs pour représenter un état qui ne devrait pas exister.

## Interactions

- **#212 (transition guard)** : `validate_transition(None, _) == Allow` — après durcissement de `project()`, c'est le tombstone qui bloque, pas le guard.
- **ADR-0020** (le forget purge aussi `~/.pdo/runs/<id>`) : inchangé, conservé **après** la transaction.
