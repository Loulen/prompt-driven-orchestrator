# ADR-0027 — Le « Run now » d'un Trigger est un fire de première classe

Sans cet ADR, un agent réimplémenterait « Run now » comme un raccourci frontend qui POSTe `/runs`
directement — contournant le guard, la gate d'overlap, l'audit `trigger_fires` et `triggered_by`.

Date : 2026-07-13 · Statut : accepté · Issue : #341

## Décision

Un fire manuel emprunte **exactement le chemin cron**, extrait en `fire_one_trigger(state, trigger,
now, source)`, partagé verbatim entre le tick du scheduler (`FireSource::Cron`) et `POST
/triggers/{id}/fire` (`FireSource::Manual`). Guard, gate d'overlap, `prompt_required`, création du
Run avec `triggered_by`, audit `trigger_fires` + broadcast WS : identiques. Le handler manuel se
sérialise avec le tick via `trigger_tick_lock` (pas de course sur la fenêtre d'overlap).

Contrat HTTP véridique (ADR-0025) : `404` sur trigger inconnu ; `409` nommant le trigger s'il est
disabled, **avant tout effet** — aucune ligne d'audit ; `409 broken reference` sur référence
pipeline/repo cassée (le cron garde son outcome `error` audité) ; `200 {fired:true, run_id}` sur
fire ; `200 {fired:false, outcome, reason}` + ligne d'audit sur guard non nul ou overlap atteint —
un noop légal est un 200 honnête.

Différences assumées entre manuel et cron :

1. **`due` est forcé** : le clic de l'utilisateur *est* le planning. Le skip silencieux de `decide()`
   reste réservé au cron ; le handler vérifie `enabled` → 409 avant d'atteindre le chemin partagé.
2. **`next_fire_at` intact** : le fire manuel ne recale jamais `set_next_fire`. Un « Run now » à
   14 h 32 ne décale pas le slot de 15 h 00.

Provenance : colonne additive **`source TEXT`** sur `trigger_fires` (`manual` / `cron`, NULL legacy ≈
cron), migrée par le même `ALTER` gardé par `pragma_table_info` que les colonnes #239/#244. **Pas de
nouveaux outcomes** : l'origine est une dimension orthogonale au résultat ; l'UI n'a aucun nouvel
état à apprendre.

## Alternatives rejetées

- **Fire quand même sur un trigger disabled** : un état qui interdit l'action mérite un refus
  explicite, pas un contournement (ADR-0025). Réactiver puis cliquer reste à un clic.
- **Bump de `next_fire_at` après un fire manuel** : le planning cron appartient au heartbeat cron
  (invariant UTC #222) ; un fire manuel n'est pas un slot consommé.
- **Nouveaux outcomes `fired-manual`/…** : explosion combinatoire et nouveaux status-dots à
  enseigner à l'UI. La colonne `source` suit le précédent #244.

## Conséquences

- Un guard lent (timeout dur ~30 s) rend la requête manuelle synchrone d'autant — acceptable pour un
  geste explicite ; le lock tick est tenu pendant ce temps, comme pour un tick cron.

## Addendum (2026-07-18, #350) — le pôle opposé : « Tester le guard (dry-run) »

Le même axe porte un second pôle, symétrique : tester le guard **sans le moindre effet de bord**.
`POST /triggers/guard/test` exécute la commande de guard *telle qu'en cours de saisie* (le Trigger
n'est pas forcément sauvegardé) via un **seam pur** (`run_guard`, sans `Trigger` ni `AppState`), puis
s'arrête au verdict : **aucun Run spawné, aucune ligne `trigger_fires`, aucun recalcul de
`next_fire_at`, aucun lock de tick, aucune gate d'overlap**. L'endpoint est **guard-faithful** ; le
verdict would-fire / would-reject est composé **côté client**.

Pourquoi *ne pas* réutiliser `fire_one_trigger` avec un flag `dry_run` : ce chemin *est défini par
ses effets* (audit + Run + provenance + lock) ; un flag qui les court-circuiterait tous ferait mentir
son nom et rouvrirait la fenêtre de course que `trigger_tick_lock` ferme.

La sécurité est **net-neutre** : le même sink `run_guard` → `sh -c` est déjà atteignable, sans auth,
via `POST /triggers` + `POST /triggers/{id}/fire` (ADR-0017 : « même surface, aucune nouvelle
frontière de confiance »).

Ce pôle n'est **pas** un ADR distinct : additif, trivialement réversible. Il se distingue aussi du
**rejet de champ vide** à la création d'un Trigger (`prompt_required` sans input résolu) — un refus
de *config*, pas un verdict de *guard*.
