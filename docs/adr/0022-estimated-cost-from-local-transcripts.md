# Coût estimé d'un Run à partir des transcripts Claude Code locaux

> **Amendé par ADR-0034 (#427)** : la table de prix n'est plus un `const` seul — elle a trois tiers
> (`manuel → fetché → embarquée`, fusion par clé) remplis par un fetch hors du chemin de lecture.
> Ce qui survit intact ici : le coût reste une **estimation**, **dérivée à la lecture**, jamais
> persistée, et le chemin de lecture reste strictement local.

## Contexte

Le panneau d'info d'un Run affiche un bloc de stats (Durée, Sessions de nœud lancées, LOC ;
cf. #100). #272 demande une **quatrième métrique** : le **coût** du Run. CONTEXT.md l'avait jugé
hors-scope au motif qu'« aucune télémétrie de coût fiable n'existe côté machine utilisateur ». Ce
motif était **factuellement faux** : Claude Code écrit, pour chaque message d'une session, un
enregistrement `usage` (compteurs de tokens) dans un transcript local sous `~/.claude/projects/`.
Le propriétaire a ratifié la réouverture sur l'issue (2026-07-06).

Il n'y a pas de champ de coût pré-calculé dans ces transcripts, seulement des compteurs de tokens :
on est en mode *calculate* (tokens × table de prix), pas *display*. Un coût **autoritatif**
(facture) n'est donc pas atteignable localement ; un coût **estimé** l'est.

## Décision

Estimer le coût d'un Run à partir des transcripts Claude Code locaux : agréger les tokens `usage`
de toutes ses sessions × une table de prix (par MTok, prix de cache dérivés 1.25× / 2× / 0.1× de
l'input). Le calcul est **dérivé à la lecture** (comme LOC), jamais persisté, et l'UI l'étiquette
explicitement « est. ».

### Ce qu'on décide

- **Table de prix locale, aucune dépendance réseau sur le chemin de lecture.** À l'origine une
  table embarquée en Rust (~11 lignes, source : page de prix Anthropic) ; depuis #427 elle est le
  **tier plancher** d'un empilement à trois tiers dont le contrat complet (sources, fusion par clé,
  déclencheurs) vit dans **ADR-0034**. Embarquer / shell-out `ccusage` et fetcher LiteLLM au build
  restent rejetés (cf. Alternatives).

- **Dérivé à la lecture, pas de snapshot à la complétion.** LOC est déjà « dérivé à la lecture,
  jamais persisté ». Un snapshot à la complétion n'a aucun précédent (aucun événement ne fige une
  métrique disque), figerait un Run vivant, et sous-compterait après un `resume_run`. Le dérivé
  n'ajoute ni schéma ni changement de projection et survit à un re-drive par construction.

- **Déduplication obligatoire par `(message.id, requestId)`.** Claude Code rejoue le même message
  assistant sur reprise/compaction : dans un transcript réel le même message apparaît ~2.35×
  (mesuré : 181 lignes assistant → 77 `message.id` distincts). Sommer les lignes brutes sur-compte
  d'autant. L'`usage` est byte-identique au sein d'un groupe, donc garder-un est **exact**
  (identique à ccusage). Les lignes sans `message.id` sont toujours comptées.

- **Un seul total, toutes sessions confondues.** Le glob par préfixe capture les nœuds, le Pipeline
  Manager, le merge-resolver **et** les subagents. La dédup par `message.id` rend tout
  double-comptage impossible même si un message apparaît aussi dans le fichier parent. L'issue
  demande « une nouvelle ligne de stat » → un total unique.

- **Encodeur de chemin propre, isolé du bug partagé.** L'encodeur de working-dir du détecteur de
  staleness est bogué (strippe le `/` initial, ne mappe pas `.`) et renvoie « inconnu » pour tout
  dossier PDO. Le corriger **réactiverait** la sonde mtime morte de stale/auto-complete (changement
  de comportement réel, #251-adjacent) : à traiter séparément. Le coût utilise donc son propre
  encodeur (tout non-alphanumérique → `-`, casse préservée) et ne touche pas à la fonction
  partagée.

- **Modèle inconnu → $0 + drapeau « borne basse » (`partial`).** Un modèle absent de la table ne
  contribue pas et lève `partial: true`. `<synthetic>` (sentinelle locale sans coût) est tarifé $0
  explicitement, **pas** traité comme inconnu. Parsing tolérant : une ligne JSON déchirée (écriture
  entrelacée observée) est ignorée ligne-à-ligne, jamais propagée.

- **Étiquetage honnête (load-bearing).** Un nombre qui a l'air autoritatif mais dérive (prix de
  liste, pas de remise entreprise, modèles non tarifés à $0) est un piège. L'UI l'appelle
  « Est. cost », préfixe la valeur d'un `~`, expose un tooltip « estimate … not an invoice », et
  ajoute un « † » + « lower bound » quand `partial`. Conforme à ADR-0001 (outil tranchant :
  montrer le nombre, l'étiqueter honnêtement, ne pas le cacher).

## Conséquences

- **Positif.** Le coût est visible sans dépendance binaire, et son chemin de lecture est sans
  dépendance réseau. Il est **plus durable que LOC** : le cleanup supprime la branche (LOC → « — »)
  mais pas les transcripts, donc un Run **archivé** garde son coût — cohérent avec l'esprit
  d'ADR-0020.

- **Négatif / assumé.** Le nombre **dérive** de la facture réelle (prix de liste, remises ignorées,
  modèles non tarifés à $0) — assumé et étiqueté. Le calcul parse les transcripts à chaque lecture
  du Run (médiane ~178 Ko, p90 ~800 Ko, un nœud « doc » long a atteint 14 Mo) ; l'échappatoire
  sanctionnée si la latence régresse est un memo par `(run_id, mtime-max)` — porte `stat` bon
  marché, à ne construire que si le profilage le réclame *(construit depuis, cf. ADR-0029/0034)*.
  Le coût n'est **pas** ajouté au handler de **liste** : cela éviterait un scan de transcripts
  fan-out par poll.

## Amendement — Runs sandboxés (#408)

La racine des transcripts n'est plus figée : le calcul de coût prend un `transcripts_root`
injectable (seam d'ADR-0030 pt 9). Un Run sandboxé **vivant** lit son *staged home* ; après
`cleanup_run`, le merge-back a flushé les transcripts vers `~/.claude/projects/` et la racine
redevient le défaut hôte. Un Run `off` lit toujours le défaut hôte — invariant inchangé. Le memo
garde la même racine pour la clé et la valeur (pas de désync) ; la dédup reste la garantie
anti-double-comptage quelle que soit la racine.

## Amendement — Trois tiers et une source distante (#427)

Voir **ADR-0034**, source autoritaire : trois tiers `manuel → fetché → embarquée` résolus par clé
de famille (fusion par clé, jamais remplacement), le `const` devenant le plancher. Ce qui survit
ici : estimation, dérivé à la lecture, chemin de lecture local. Deux limites assumées héritées du
« dérivé à la lecture » : le mode fast est invisible (même id dans les transcripts → sous-facturé),
et éditer la table retarife tous les Runs historiques, archivés inclus.

## Alternatives rejetées

- **Embarquer / shell-out `ccusage`** — dépendance binaire + Node, et ccusage imposerait **sa**
  table plutôt que la nôtre (« réseau » n'est plus discriminant depuis #427, le rejet tient).
- **Fetcher LiteLLM au build** — fige la table dans le binaire et ramène le couplage à la release
  qu'ADR-0034 supprime ; fetcher out-of-band vers un cache disque ne le fait pas. LiteLLM reste
  l'adaptateur de repli documenté si models.dev disparaît (ADR-0034).
- **Snapshot à la complétion** — aucun précédent, fige un Run vivant, sous-compte au `resume_run`.
- **Figure autoritative / facture** — non atteignable localement (prix de liste seulement).
  Cadrage honnête : *estimation faisable, autoritatif non.*

## Hors-scope (suivis à filer)

- **Correction de l'encodeur de working-dir partagé** (réactive la sonde mtime morte — #251).
- **Palier long-contexte > 200K** — sous-compte seulement sur une requête isolée > 200K input.
- **Prix d'intro datés / rafraîchissement live** — traités par ADR-0034 (le fetch est out-of-band,
  la dimension de date par ligne reste hors-scope).
