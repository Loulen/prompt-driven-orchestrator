# ADR-0027 — Le « Run now » d'un Trigger est un fire de première classe

Date : 2026-07-13 · Statut : accepté · Issue : #341

## Contexte

« Run now » sur un Trigger était un raccourci purement frontend : il ouvrait la modale New
Run pré-remplie et POSTait `/runs` directement. Conséquences : le guard n'était pas exécuté,
la gate d'overlap était contournée, aucune ligne `trigger_fires` n'était écrite, et le Run
créé ne portait pas `triggered_by`. Le choix initial (« le guard n'est pas exécuté », documenté
dans CONTEXT.md) sidestepait l'ambiguïté guard/overlap — mais l'historique mentait par
omission et « tester ce que fait ce Trigger » ne testait précisément pas le contrat du
Trigger. #341 renverse ce choix.

## Décision

Un fire manuel emprunte **exactement le chemin cron**, extrait en `fire_one_trigger(state,
trigger, now, source)` (lib.rs), partagé verbatim entre le tick du scheduler
(`FireSource::Cron`) et le nouvel endpoint `POST /triggers/{id}/fire` (`FireSource::Manual`).
Guard, gate d'overlap, `prompt_required`, création du Run avec `triggered_by`, audit
`trigger_fires` + broadcast WS `trigger_fired` : identiques. Le handler manuel se sérialise
avec le tick via `trigger_tick_lock` (pas de course sur la fenêtre d'overlap).

Contrat HTTP véridique (ADR-0025) :

| Cas | Réponse |
|---|---|
| Trigger inconnu | `404` |
| Trigger disabled | `409` nommant le trigger, **avant tout effet** — aucune ligne d'audit |
| Référence pipeline/repo cassée | `409` « broken reference: … » (le cron garde son outcome `error` audité) |
| Fire | `200 {ok:true, fired:true, run_id}` |
| Guard exit ≠ 0 / overlap atteint | `200 {ok:true, fired:false, outcome, reason}` + ligne d'audit — un noop légal est un 200 honnête |

Différences assumées entre manuel et cron :

1. **`due` est forcé** : le clic de l'utilisateur *est* le planning. Le skip silencieux de
   `decide()` (`!enabled || !due`) reste réservé au cron ; le handler vérifie `enabled` → 409
   avant d'atteindre le chemin partagé.
2. **`next_fire_at` intact** : le fire manuel ne recale jamais `set_next_fire` (gated sur
   `FireSource::Cron`). Un « Run now » à 14 h 32 ne décale pas le slot de 15 h 00.

Provenance dans l'historique : colonne additive **`source TEXT`** sur `trigger_fires`
(`manual` / `cron`, NULL legacy ≈ cron), migrée par le même `ALTER` gardé par
`pragma_table_info` que les colonnes #239/#244. **Pas de nouveaux outcomes** : l'origine est
une dimension orthogonale au résultat ; l'UI n'a aucun nouvel état à apprendre (badge
« manual » sur la ligne, c'est tout).

Frontend : le bouton Play appelle l'endpoint puis ouvre le **détail du trigger** via
`handleSelectTrigger` (seul chemin qui survit à la réconciliation #320), où la ligne
apparaît — le handler WS `trigger_fired` bumpe désormais un `refreshKey` qui refetch
l'historique du panneau ouvert (couvre aussi les fires cron tombant pendant la consultation).

## Alternatives rejetées

- **Fire quand même sur un trigger disabled** (« un clic humain est explicite ») : un état
  qui interdit l'action mérite un refus explicite, pas un contournement — cohérent ADR-0025.
  Réactiver puis cliquer reste à un clic.
- **Bump de `next_fire_at` après un fire manuel** : le planning cron appartient au heartbeat
  cron (invariant UTC #222) ; un fire manuel n'est pas un slot consommé.
- **Nouveaux outcomes `fired-manual`/…** : dimension provenance encodée dans l'outcome →
  explosion combinatoire et nouveaux status-dots à enseigner à l'UI. La colonne `source`
  suit le précédent #244 (colonnes additives descriptives).

## Conséquences

- L'ancienne modale « Run now » (mode `run` de `NewRunModal`) n'est plus appelée par le
  bouton Play ; le mode reste dans le code (inoffensif) tant qu'un autre appelant existe.
- Un guard lent (timeout dur ~30 s) rend la requête manuelle synchrone d'autant — acceptable
  pour un geste explicite ; le lock tick est tenu pendant ce temps, comme pour un tick cron.
