# Coût estimé d'un Run à partir des transcripts Claude Code locaux

Sans cet ADR, un agent chercherait une télémétrie de coût côté serveur (ou la déclarerait
impossible) au lieu de dériver le coût des transcripts locaux, et il persisterait le résultat.

> Le coût dérivé décrit ici n'est pas la seule forme : un harnais qui compte lui-même son coût
> en fournit un **rapporté**, converti par une **constante publiée** et qui **ne passe pas par la
> table de prix** (les buckets de cache ne se mappent pas, et le total d'input d'un autre harnais
> peut déjà inclure le cache). Un total de Run reste sommable mais se **dit ventilé par harnais** ;
> un harnais sans capacité de coût rend « — ». Voir ADR-0045 et ADR-0052.

## Décision

Estimer le coût d'un Run à partir des transcripts Claude Code locaux (`~/.claude/projects/`) :
agréger les tokens `usage` de toutes ses sessions × une table de prix (par MTok, prix de cache
dérivés 1.25× / 2× / 0.1× de l'input). Il n'y a pas de champ de coût pré-calculé dans ces
transcripts : on est en mode *calculate*, pas *display*. Un coût **autoritatif** (facture) n'est pas
atteignable localement ; un coût **estimé** l'est.

### Ce qu'on décide

- **Table de prix locale, aucune dépendance réseau sur le chemin de lecture.** Depuis #427 la table
  embarquée est le **tier plancher** d'un empilement dont le contrat vit dans **ADR-0034**.

- **Dérivé à la lecture, pas de snapshot à la complétion.** Un snapshot figerait un Run vivant et
  sous-compterait après un `resume_run` ; le dérivé survit à un re-drive par construction.

- **Déduplication obligatoire par `(message.id, requestId)`.** Claude Code rejoue le même message
  assistant sur reprise/compaction : dans un transcript réel le même message apparaît ~2.35×
  (mesuré : 181 lignes assistant → 77 `message.id` distincts). L'`usage` est byte-identique au sein
  d'un groupe, donc garder-un est **exact**. Les lignes sans `message.id` sont toujours comptées.

- **Un seul total, toutes sessions confondues.** Le glob par préfixe capture les nœuds, le Pipeline
  Manager, le merge-resolver **et** les subagents ; la dédup rend tout double-comptage impossible.

- **Encodeur de chemin propre, isolé du bug partagé.** L'encodeur de working-dir du détecteur de
  staleness est bogué et renvoie « inconnu » pour tout dossier PDO. Le corriger **réactiverait** la
  sonde mtime morte de stale/auto-complete (changement de comportement réel) : à traiter séparément.
  Le coût utilise donc son propre encodeur et ne touche pas à la fonction partagée.

- **Modèle inconnu → $0 + drapeau « borne basse » (`partial`).** `<synthetic>` (sentinelle locale
  sans coût) est tarifé $0 explicitement, **pas** traité comme inconnu. Parsing tolérant : une ligne
  JSON déchirée (écriture entrelacée observée) est ignorée ligne-à-ligne.

- **Étiquetage honnête (load-bearing).** Un nombre qui a l'air autoritatif mais dérive (prix de
  liste, pas de remise entreprise, modèles non tarifés à $0) est un piège : « Est. cost », préfixe
  `~`, tooltip « estimate … not an invoice », « † lower bound » quand `partial` (ADR-0001).

- **Racine des transcripts injectable, pas figée.** Un Run sandboxé vivant lit son *staged home* ;
  après `cleanup_run`, le merge-back a flushé les transcripts vers `~/.claude/projects/` et la racine
  redevient le défaut hôte (un Run `off` lit toujours le défaut hôte). Le memo garde la même racine
  pour la clé et la valeur — sinon une racine changée en cours de vie du Run casserait le cache.

## Conséquences

- Le coût est **plus durable que LOC** : le cleanup supprime la branche (LOC → « — ») mais pas les
  transcripts, donc un Run **archivé** garde son coût — cohérent avec ADR-0020.
- Le nombre **dérive** de la facture réelle — assumé et étiqueté. Le calcul parse les transcripts à
  chaque lecture du Run (médiane ~178 Ko, p90 ~800 Ko, un nœud « doc » a atteint 14 Mo) ; le memo par
  `(run_id, mtime-max)` a depuis été construit (ADR-0029/0034). Le coût n'est **pas** ajouté au
  handler de **liste** : cela ferait un scan de transcripts fan-out par poll.
- Deux limites héritées du « dérivé à la lecture » : le mode fast est invisible (même id dans les
  transcripts → sous-facturé), et éditer la table retarife tous les Runs historiques, archivés
  inclus.

## Alternatives rejetées

- **Embarquer / shell-out `ccusage`** — dépendance binaire + Node, et ccusage imposerait **sa** table
  plutôt que la nôtre.
- **Fetcher LiteLLM au build** — fige la table dans le binaire et ramène le couplage à la release
  qu'ADR-0034 supprime. LiteLLM reste l'adaptateur de repli si models.dev disparaît.
- **Snapshot à la complétion** — fige un Run vivant, sous-compte au `resume_run`.
- **Figure autoritative / facture** — non atteignable localement. *Estimation faisable, autoritatif
  non.*

## Hors-scope

- **Correction de l'encodeur de working-dir partagé** (réactive la sonde mtime morte — #251).
- **Palier long-contexte > 200K** — sous-compte seulement sur une requête isolée > 200K input.
