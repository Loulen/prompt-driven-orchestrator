# Statistiques d'instance agrégées : dérivées à la lecture + indexées, jamais matérialisées

## Contexte

#377 ajoute une **modale de stats** (cockpit d'observabilité opérateur) : runs/sessions/erreurs par
période, fires de trigger par pipeline, et **coût estimé** par période/pipeline/projet, filtrables.
Tout est aujourd'hui *par-run* (`load_events(run_id)`) ou *par-trigger* ; aucune requête transverse
par période n'existe, le daemon n'a **aucun index**, et le coût (ADR-0022) est dérivé-à-la-lecture,
sans cache, volontairement **exclu du handler de liste** (anti-fan-out). Une modale « coût toutes
runs » est exactement ce fan-out interdit : mesuré à 2 502 transcripts / 1,1 Go localement.

## Décision

- **Dérivé à la lecture, jamais matérialisé.** Pas de table de snapshot de coût, pas d'`EventKind`
  qui fige une métrique. **Préserve ADR-0022** (Shape B, snapshot à la complétion, explicitement
  rejeté) et ADR-0001 (outil tranchant, étiquetage honnête).
- **Deux classes, deux endpoints.** `GET /stats/overview` = SQL bon marché, index-backed
  (`GROUP BY strftime`). `GET /stats/cost` = lourd, lazy, derrière un **memo RAM `(run_id,
  mtime-max)`** (l'échappatoire sanctionnée par ADR-0022 lignes 84-85), borné à la période visible ;
  le chemin single-run de `get_run` reste inchangé.
- **Deux index idempotents** au boot : `events(kind, ts)` et `trigger_fires(ts)`
  (`CREATE INDEX IF NOT EXISTS`, nativement idempotent — pas de garde PRAGMA, contrairement aux
  `ALTER ADD COLUMN` de #239/#244).
- **`pipeline_id` porté par `RunStarted`** (fallback `pipeline_name`) pour que « par pipeline »
  survive un renommage (#230). Additif, rétro-compatible.
- **Axes catégoriels du coût pliés côté app (Rust).** Le coût est un scalaire par-run sans dimension
  pipeline/projet ; « par projet » = `effective_repo_root` (fallback runtime absent des tables). Les
  cinq séries *nommées* (runs/sessions/erreurs/fires-par-pipeline/triggers-ayant-créé-un-run) restent
  du SQL pur indexé.
- **Étiquetage honnête agrégé (load-bearing).** Un bucket est une **somme de bornes basses** : tout
  run `partial` (modèle non tarifé) rend le bucket borne-basse (`†`). Les runs sans transcript sont
  exclus de la somme mais **comptés (`null`)** et exposés, jamais silencieusement sous-comptés.

## Conséquences

- **Positif.** Réactif (SQL indexé + memo), zéro dépendance réseau, aucune divergence possible avec
  l'event log (source de vérité unique), le coût reste consultable pour un run archivé.
- **Négatif / assumé.** Le memo vit en RAM (perdu au restart, reconstruit à la demande). La table de
  prix reste à maintenir à la main (ADR-0022). « Sessions/période » compte les *démarrages* de
  session (`node_started`), re-spawns et laps de boucle inclus (cohérent avec la stat par-run).

## Alternatives rejetées

- **Table de snapshot de coût / EventKind figeant une métrique** — viole ADR-0022, sous-compte au
  `resume_run`.
- **Coût au handler de liste / full-scan par ouverture** — le fan-out interdit (2 502 fichiers).
- **INNER JOIN fires↔triggers** — perdrait les fires orphelins (pas de cascade au delete).
- **Bucket « Unassigned » pour les runs sans repo** — contredit §521/#258 (`effective_repo`).
