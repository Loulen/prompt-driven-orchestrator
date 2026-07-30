# Coût estimé d'un Run à partir des transcripts Claude Code locaux

## Contexte

Le panneau d'info d'un Run affiche un bloc de stats (Durée, Sessions de nœud lancées, LOC ;
cf. #100). #272 demande une **quatrième métrique** : le **coût** du Run. CONTEXT.md
(*Statistiques de Run*) l'avait jugé **hors-scope** au motif qu'« aucune télémétrie de coût
fiable n'existe côté machine utilisateur ». Ce motif était **factuellement faux** : Claude Code
écrit, pour chaque message d'une session, un enregistrement `usage` (compteurs de tokens) dans
un transcript local (`~/.claude/projects/<cwd-encodé>/*.jsonl`). La télémétrie n'est pas requise
— les transcripts portent l'usage. Le propriétaire a ratifié la réouverture sur l'issue
(2026-07-06, « yes reverse the context.md »).

Il n'y a pas de champ `costUSD` dans ces transcripts, seulement des compteurs de tokens : on est
en mode *calculate* (tokens × table de prix), pas *display* (coût pré-calculé). Un coût
**autoritatif** (facture) n'est donc pas atteignable localement ; un coût **estimé** l'est.

## Décision

Estimer le coût d'un Run à partir des transcripts Claude Code locaux : agréger les tokens
`usage` de toutes ses sessions × une **table de prix publics codée en dur** (par MTok, cache
dérivé 1.25× / 2× / 0.1× de l'input). Le calcul est **dérivé à la lecture** (comme LOC), jamais
persisté, et l'UI l'étiquette explicitement « est. ».

### Ce qu'on décide

- **Table de prix codée en dur (Rust `const`), pas de réseau.** Le daemon est *network-free*
  par conception (CONTEXT.md). Embarquer / shell-out `ccusage` (dépendance binaire + Node +
  réseau) ou fetcher LiteLLM au build (réseau + retard sur les ids récents — `claude-opus-4-8`
  peut ne pas encore y être) sont rejetés. Une table ~11 lignes suit le motif maison
  (cf. `USAGE_LIMIT_ANCHORS`, « UPDATE THIS LIST ») ; source = page de prix Anthropic
  (2026-07-06). À maintenir à la main quand Anthropic change ses prix ou sort un modèle.

- **Dérivé à la lecture (Shape A), pas de snapshot à la complétion.** LOC est « dérivé à la
  lecture, jamais persisté » ; le code refuse de figer les valeurs d'un Run vivant. Un snapshot
  à la complétion (Shape B) n'a **aucun précédent** (aucun `EventKind` ne fige une métrique
  disque), figerait un Run vivant, et sous-compterait après un `resume_run`. Shape A n'ajoute ni
  schéma ni changement de projection et survit à un re-drive par construction.

- **Déduplication obligatoire par `(message.id, requestId)`.** Claude Code rejoue le même
  message assistant sur reprise/compaction : dans un transcript réel le même message apparaît
  ~2.35× (mesuré : 181 lignes assistant → 77 `message.id` distincts). Sommer les lignes brutes
  sur-compte d'autant. L'`usage` est byte-identique au sein d'un groupe, donc garder-un est
  **exact** (identique à ccusage). Les lignes sans `message.id` sont toujours comptées.

- **Un seul total, toutes sessions confondues.** Le glob par préfixe sur `~/.claude/projects/`
  capture les nœuds, le Pipeline Manager (cwd = worktree pipeline), le merge-resolver **et** les
  subagents (`<uuid>/subagents/*.jsonl`, `isSidechain:true`). La dédup par `message.id` rend tout
  double-comptage impossible même si un message apparaît aussi dans le fichier parent. L'issue
  demande « une nouvelle ligne de stat » → un total unique.

- **Encodeur de chemin propre, isolé du bug partagé.** `stale_detector::encode_working_dir` est
  bogué (strippe le `/` initial, ne mappe pas `.`) → il renvoie `None` pour **tout** dossier
  PDO, ce qui laisse la sonde mtime de stale/auto-complete **morte** pour les Runs PDO. Le
  corriger **réactiverait** cette logique (changement de comportement réel, #251-adjacent) : à
  traiter séparément avec ses propres tests. Le coût utilise donc son propre
  `run_cost::cc_project_dirname` (tout non-alphanumérique → `-`, casse préservée) et **ne
  touche pas** à la fonction partagée ; un doc-comment croise les deux.

- **Modèle inconnu → $0 + drapeau « borne basse ».** Un modèle absent de la table ne contribue
  pas (0 $) et lève `partial: true`. `<synthetic>` (sentinelle locale sans coût de CC) est tarifé
  à $0 explicitement, **pas** traité comme inconnu — il ne lève pas `partial`. Parsing tolérant :
  une ligne JSON déchirée (un `clauclaude-opus-4-8` d'écriture entrelacée a été observé) est
  ignorée ligne-à-ligne, jamais propagée.

- **Étiquetage honnête (load-bearing).** Un nombre qui a l'air autoritatif mais dérive (prix de
  liste, pas de remise entreprise, modèles non tarifés à $0) est un piège. L'UI l'appelle
  « Est. cost », préfixe la valeur d'un `~`, expose un tooltip « estimate … not an invoice », et
  ajoute un « † » + « lower bound » quand `partial`. Conforme à la posture d'honnêteté (org) et à
  ADR-0001 (outil tranchant : montrer le nombre, l'étiqueter honnêtement, ne pas le cacher).

## Conséquences

- **Positif.** Le coût est visible sans dépendance binaire, et **son chemin de lecture est sans
  dépendance réseau** (amendement #427 : le *remplissage* de la table peut sortir sur Internet, la
  lecture jamais) ; byte-identique quand aucun
  transcript n'est trouvé (`None` → « — »). Il est **plus durable que LOC** : le cleanup supprime
  la branche (LOC → « — ») mais **pas** `~/.claude/projects/`, donc un Run **archivé** garde son
  coût — cohérent avec l'esprit d'ADR-0020.

- **Négatif / assumé.** Le nombre **dérive** de la facture réelle (prix de liste, remises
  ignorées, modèles récents non tarifés à $0) — assumé et étiqueté. La table de prix est à
  **maintenir à la main**. Le calcul parse les transcripts **à chaque** `GET /runs/:id` (comme
  LOC shelle `git` à chaque lecture) : médiane ~178 Ko, p90 ~800 Ko, mais un nœud « doc » long a
  atteint 14 Mo. MVP : parser à la lecture. Si la latence de polling régresse, suivi possible :
  mémoïser par `(run_id, mtime-max)` — une porte `stat` bon marché qui ne re-parse que si un
  transcript a changé. Borné, différable ; à ne construire que si le profilage le réclame.
  N'ajoute **pas** le coût au handler de **liste** (`GET /runs`) : il éviterait un scan de
  transcripts fan-out par poll.

## Amendement — Runs sandboxés (#408)

Le read-source `~/.claude/projects/` n'est plus figé : `compute_run_cost` (et son memo
`compute_run_cost_cached`) prennent un `transcripts_root` **injectable** — le seam
`sandbox_run::transcripts_root` (ADR-0030 pt 9). Un Run sandboxé **vivant** lit son *staged home*
(`~/.pdo/sandbox/<run-id>/claude-home/projects/`, alimenté en direct par l'identity mount) ; après
`cleanup_run`, `merge_back` a flushé les transcripts vers `~/.claude/projects/` et la racine
redevient le défaut hôte. Un Run `off` lit **toujours** `~/.claude/projects/` — invariant inchangé.
Le memo garde la **même** racine pour la clé (`max mtime`) et la valeur (agrégat), donc pas de désync.
La dédup `(message.id, requestId)` reste la garantie anti-double-comptage, quelle que soit la racine.

## Amendement — La table de prix a trois tiers et une source distante ; le `const` devient le plancher (#427)

Voir **ADR-0034** pour le contrat d'egress complet et le recensement des sources. Ce qui est amendé
ici, c'est la **puce 1** : « table de prix codée en dur (Rust `const`), pas de réseau » n'est plus
la description du système. Ce qui **survit** intact : le coût reste une **estimation** (prix de
liste, pas de facture), il reste **dérivé à la lecture**, et « à maintenir à la main » devient le
**tier manuel** — plus littéralement maintenu à la main qu'un `const`, puisqu'il ne demande plus
d'éditer du Rust, de bumper, de releaser et de lancer `make update`.

- **Trois tiers, résolus par clé de famille** : `manuel → fetché → embarquée`.
  `~/.pdo/prices/models.yaml` (écrit par l'humain seul) gagne sur `~/.pdo/prices/fetched.json`
  (écrit par le daemon seul depuis `models.dev/api.json`, **hors du chemin de lecture**), qui gagne
  sur `PRICES`. Les deux fichiers sont **absents par défaut** et rien n'est jamais seedé.
- **Fusion par clé, jamais remplacement.** Une clé absente d'un tier garde ce que le tier suivant en
  dit. Sous remplacement global, oublier `claude-opus-4-8` effacerait **79 941** lignes de
  transcript sur ~116 200 — un bug de prix faux converti en blackout — et **gèlerait** la table
  contre les releases futures.
- **Le `const` est un plancher, pas une amorce.** `claude-opus-4-0`, `claude-sonnet-4-0` et
  `claude-3-5-haiku` sont absentes de **toute** source distante examinée : le `const` en est le
  **seul** tarificateur. Un test l'épingle.
- **La clé du memo gagne un troisième composant** : `(run_id, mtime-max, empreinte de la table)`.
  Sans lui, un sync ne bougeant aucun mtime de transcript, `GET /stats/cost` (mémoïsé) servirait
  les anciens dollars **jusqu'au redémarrage** pendant que `GET /runs/:id` (non mémoïsé) dirait
  vrai — deux surfaces qui se contredisent sur les Runs terminés, c'est-à-dire précisément ceux
  qu'un sync existe pour réparer.
- **Absent : silencieux ; présent mais rejeté : dit une fois.** Un fichier absent est l'état normal
  de toute instance. Un fichier illisible, ou une ligne refusée (clé datée, sentinelle, prix négatif
  ou non fini), devient **inerte** — la clé retombe sur le tier suivant, elle ne détruit pas
  l'estimation — et le rejet est nommé **une fois** dans le journal et en `reason` sur
  `GET /settings`. Un `fetched.json` dont le `schema` n'est pas `prices-v1` est **entièrement**
  inerte.
- **Le seam #408 déplace la racine des transcripts, pas celle des prix.** Les prix sont un concept
  d'**instance** : même pour un Run sandboxé, la table se lit sous le home **hôte**.
- **Deux limites assumées.** (1) Le **mode fast** est invisible : `claude-opus-5` et
  `claude-opus-5-fast` s'écrivent du même id dans `message.model`, donc un nœud en mode fast est
  sous-facturé ×2 et aucune normalisation ne peut le rattraper. (2) Éditer la table **retarife tous
  les Runs historiques, archivés inclus** — conséquence directe du « dérivé à la lecture » ci-dessus.
  Cela satisfait la contrainte « un Run archivé s'ouvre et se chiffre », mais le chiffre d'un Run
  clos devient fonction d'un fichier modifiable, à assumer au titre de l'« Étiquetage honnête
  (load-bearing) » ci-dessus.

## Alternatives rejetées

- **Embarquer / shell-out `ccusage`** — dépendance binaire/Node + réseau ; daemon network-free.
  *(re-motivé par #427 — le rejet **tient**, mais « réseau » n'y est plus discriminant. Le motif
  survivant est la dépendance **binaire + Node**, et le fait que ccusage imposerait **sa** table
  plutôt que la nôtre.)*
- **Fetcher LiteLLM `model_prices_and_context_window.json`** (ce que fait ccusage) — réseau au
  build + retard amont sur les ids récents.
  *(toujours rejeté après #427, et la distinction est load-bearing : fetcher **au build** fige la
  table dans le binaire et **ramène** le couplage à la release qu'ADR-0034 supprime ; fetcher
  **out-of-band vers un cache disque** ne le fait pas. LiteLLM reste l'adaptateur de repli
  documenté si models.dev disparaît.)*
- **Snapshot à la complétion (Shape B)** — aucun précédent, fige un Run vivant, sous-compte au
  `resume_run`.
- **Figure autoritative / facture** — non atteignable localement (pas de `costUSD`, prix de liste
  seulement). Cadrage honnête : *estimation faisable, autoritatif non.*

## Hors-scope (suivis à filer)

- **Correction de `stale_detector::encode_working_dir`** (réactive la sonde mtime morte — #251).
- **Palier long-contexte > 200K** (surcharge input Sonnet-4.5, non vérifiée) — sous-compte
  seulement sur une requête isolée > 200K input ; PDO tourne opus-4-8 (pas de surcharge).
- **Prix d'intro daté de `claude-sonnet-5`** ($2/$10 jusqu'au 2026-08-31) — seulement si
  sonnet-5 commence à apparaître.
  *(depuis #427, c'est **l'édition d'une ligne** dans `~/.pdo/prices/models.yaml` au 2026-08-31 —
  ou rien du tout si un sync a tourné d'ici là. Aucune source ne porte de date d'effet, et la
  dimension de date par ligne reste hors-scope : voir ADR-0034.)*
- **Rafraîchissement de prix live (LiteLLM / models.dev)** — rejeté (daemon network-free).
  *(levé par #427 — voir **ADR-0034** : le fetch est **out-of-band** et le chemin de lecture reste
  strictement local. Le motif « daemon network-free » était déjà faux à l'écriture :
  `service_unit.rs:49-50` déclare `After=network-online.target`, le daemon shelle les guards de
  Trigger — `gh issue list` à chaque tick — et ADR-0030 fait un `docker pull`. Ce qui est vrai et
  reste protégé est plus étroit : **un `GET /runs/:id` ne dépend pas d'Internet**.)*
