# Statistiques d'instance agrégées : dérivées à la lecture + indexées, jamais matérialisées

## Contexte

Sans cette ADR, la modale de stats (#377 : runs/sessions/erreurs par période, fires de trigger, coût
estimé filtrable) se construit soit en matérialisant le coût, soit en le calculant au handler de
liste. Tout est aujourd'hui *par-run* ou *par-trigger* ; le daemon n'a **aucun index**, et le coût
(ADR-0022) est dérivé-à-la-lecture, sans cache, volontairement **exclu du handler de liste**
(anti-fan-out). Une modale « coût toutes runs » est exactement ce fan-out interdit : mesuré à
2 502 transcripts / 1,1 Go localement.

## Décision

- **Dérivé à la lecture, jamais matérialisé.** Pas de table de snapshot de coût, pas d'`EventKind`
  qui fige une métrique. **Préserve ADR-0022** (Shape B, snapshot à la complétion, explicitement
  rejeté) et ADR-0001 (étiquetage honnête).
- **Deux classes, deux endpoints.** `GET /stats/overview` = SQL bon marché, index-backed.
  `GET /stats/cost` = lourd, lazy, derrière un **memo RAM de contributions** indexé par le Run, les
  événements, les mtimes Claude/Copilot, l'empreinte tarifaire et les racines de stockage, borné à la
  période visible ; le chemin single-run reste inchangé.
- **Trois index idempotents au boot** : `events(kind, ts)` pour sélectionner les cohortes,
  `events(run_id, kind, id)` pour joindre seulement leurs démarrages de Node dans l'ordre, et
  `trigger_fires(ts)`. `CREATE INDEX IF NOT EXISTS` est nativement idempotent — pas de garde PRAGMA,
  contrairement aux `ALTER ADD COLUMN` de #239/#244.
- **`pipeline_id` porté par `RunStarted`** (fallback `pipeline_name`) pour que « par pipeline »
  survive un renommage (#230). Additif, rétro-compatible.
- **Axes catégoriels du coût pliés côté app (Rust).** La sélection des cohortes et des sessions reste
  du SQL indexé ; le pli hiérarchique reste en mémoire. « Par projet » = `effective_repo_root`.
- **Ventilation multi-harnais par exécution (#638).** La fenêtre sélectionne une cohorte de Runs par
  leur démarrage, puis attribue le coût complet de chaque exécution à son harnais, son Pipeline et
  son Node. Chaque harnais traduit sa propre source vers cette contribution commune ; une source
  moins précise reste `Non attribué`, **jamais imputée par supposition** à un Node ou à
  l'Infrastructure.
- **Étiquetage honnête agrégé (load-bearing).** Un bucket est une **somme de bornes basses** : tout
  run `partial` (modèle non tarifé) rend le bucket borne-basse (`†`). Les runs sans transcript sont
  exclus de la somme mais **comptés (`null`)** et exposés, jamais silencieusement sous-comptés.

## Conséquences

- **Zéro dépendance réseau sur le chemin de lecture** (#427 : le remplissage de la table de prix est
  out-of-band) ; aucune divergence possible avec l'event log (source de vérité unique) ; le coût
  reste consultable pour un run archivé.
- **Assumé.** Le memo vit en RAM (perdu au restart, reconstruit à la demande). Sa clé doit couvrir
  l'empreinte de la table de prix : sans elle, un sync resterait invisible ici jusqu'au redémarrage
  alors que `GET /runs/:id` dirait vrai. La table se résout **`manuel → fetché → embarquée`**
  (ADR-0022 amendement #427, ADR-0034). « Sessions/période » compte les *démarrages* de session,
  re-spawns et laps de boucle inclus (cohérent avec la stat par-run).

## Alternatives rejetées

- **Table de snapshot de coût / EventKind figeant une métrique** — viole ADR-0022, sous-compte au
  `resume_run`.
- **Coût au handler de liste / full-scan par ouverture** — le fan-out interdit.
- **INNER JOIN fires↔triggers** — perdrait les fires orphelins (pas de cascade au delete).
- **Bucket « Unassigned » pour les runs sans repo** — contredit #258 (`effective_repo`).
