# La table de prix a une source distante, fetchée hors du chemin de lecture

Sans cette ADR, on refuserait tout egress au daemon au nom du « network-free » d'ADR-0022, ou bien on
fetcherait les prix paresseusement sur le chemin de lecture d'un Run. Les deux sont faux : ce qui doit
rester vrai est plus étroit — **la lecture d'un Run ne doit pas dépendre d'Internet**. Le daemon a déjà
trois egress ratifiés (unité systemd, guards de Trigger shellés, `docker pull` d'ADR-0030), et chaque
nœud est une session `claude` : le produit ne fonctionne pas hors ligne.

> **Amendé par ADR-0052** : la table résolue cesse d'être le passage obligé de tout coût. Un coût
> **rapporté** par le harnais se convertit par une constante publiée et ne la consulte pas — elle reste
> donc la table des modèles **tarifés par famille Anthropic**, sans avoir à s'étendre aux catalogues
> des autres harnais.

## Contexte

La table codée en dur d'ADR-0022 dérive plus vite qu'elle ne se corrige : au 2026-07-30, ~30 % des
lignes de transcript (dont `claude-opus-5` et `claude-fable-5`, le plus **cher** de tous à $10/$50)
n'étaient tarifées par aucune ligne du `const`, donc lues à $0. Corriger exigeait d'éditer du Rust, de
releaser et de mettre à jour le daemon de production — un chemin qui, de fait, n'est pas pris.

## Décision

Le daemon peut sortir sur Internet, **hors du chemin de lecture uniquement**, pour remplir un cache de
prix sur disque. La table devient un empilement à trois tiers résolu **par clé de famille** :
`manuel → fetché → embarquée`. La lecture reste strictement locale : deux lectures de fichiers et une
constante.

- **Trois tiers, deux fichiers, un seul écrivain par fichier.** Le tier **manuel**
  (`~/.pdo/prices/models.yaml`) : l'humain l'écrit, PDO n'y touche **jamais**. Le tier **fetché**
  (`~/.pdo/prices/fetched.json`) : le daemon le réécrit **intégralement**, et personne d'autre. Un
  fichier unique réécrit par le sync effacerait une correction à la main ; un fichier à deux sections
  condamnerait le daemon à réécrire partiellement un fichier que l'humain édite, sans précédent de
  réécriture partielle sûre dans ce codebase.

- **Fusion par clé, jamais remplacement.** Une clé présente dans un tier gagne ; une clé absente garde
  ce que le tier suivant en dit. Sous remplacement global, une clé oubliée convertirait un prix faux en
  blackout total, et le fichier **gèlerait** la table contre les releases futures.

- **La table embarquée est un plancher, jamais une amorce.** Elle tarife ce qu'aucun remote ne porte
  (`claude-opus-4-0`, `claude-sonnet-4-0`, `claude-3-5-haiku` ont été purgées des trois sources
  examinées) **et** les familles de la génération courante, toujours surchargées par un sync.

- **models.dev est la source ; OpenRouter est rejeté.** `GET https://models.dev/api.json`, namespace
  `anthropic` seulement. Le critère décisif n'est ni la fraîcheur ni la licence : **ses clés sont déjà
  le vocabulaire de PDO** (l'id de l'API Anthropic), donc la normalisation se réduit à un dé-datage et
  le risque de *mauvais mapping* — pire qu'une ligne manquante, il produit un nombre plausible et faux —
  tombe à zéro. Anthropic n'expose aucun champ tarifaire dans son API : sa page de pricing est le juge
  d'appel, pas une source machine. **Une seule source, un seul parseur** : l'URL est surchargeable mais
  le parseur est de forme models.dev, donc la pointer ailleurs produit un refus explicite, pas un silence.

- **Le dé-datage est asymétrique, et c'est voulu.** Sur le tier **manuel**, une clé datée est **refusée**
  en imprimant la forme correcte : stripper collapserait silencieusement deux lignes que l'auteur voulait
  distinctes. Sur le chemin **fetché**, l'identifiant est **dé-daté** : la source expose des ids datés, et
  c'est la forme dé-datée que les transcripts écrivent. Une collision à prix divergents après dé-datage
  fait **tomber la clé entière** du tier fetché, nommée — c'est un défaut de source, pas un cas à arbitrer
  par heuristique. Le suffixe `-fast` n'est **jamais** stripé : le stripper créerait la collision.

- **Une moisson vide est un échec, pas un résultat.** Une dérive de schéma chez la source écrirait sinon
  un cache vide qui **détruirait la dernière table connue** — le seul chemin par lequel cette feature
  pourrait *détruire* quelque chose.

- **Un sync qui ne change rien n'écrit rien.** Lignes normalisées identiques → pas de réécriture, réponse
  noop + raison (ADR-0025), empreinte inchangée, memo de coût toujours chaud. Prix payé : le millésime
  n'avance pas sur un noop, donc un GET par démarrage au pire.

- **Le garde numérique s'applique aux DEUX tiers disque.** Un prix négatif ou non fini est refusé aussi
  dans le tier fetché : rien d'autre que le nom du fichier n'en interdit l'édition à la main, et un NaN
  empoisonne le total **et** sérialise en JSON `null` vers un frontend qui attend un nombre.

- **Deux déclencheurs, deux postures d'échec.** Le **bouton** échoue en **502 nommant l'URL** (l'effet a
  été explicitement demandé) ; le **rafraîchissement au démarrage** échoue en warn, jamais fatal, à cause
  des courses d'ordonnancement du boot.

- **Le démarrage rafraîchit, il n'amorce pas.** Le fetch de boot ne se déclenche que si le cache fetché
  existe déjà et a plus de 24 heures : **aucun egress avant le premier clic « Sync coûts »** — le clic
  **est** le consentement (ADR-0001, ADR-0012).

- **L'empreinte de la table entre dans la clé du memo de coût** (`(run_id, mtime-max, empreinte)`). Sans
  ce troisième composant, un sync ne bougerait aucun mtime de transcript : les stats agrégées mémoïsées
  serviraient les anciens dollars jusqu'au redémarrage pendant que le détail d'un Run dirait vrai. Le sync
  **ne vide pas** le memo — sous la nouvelle clé une entrée périmée est simplement inatteignable.

- **La lecture reste locale, par requête, sans cache global figé.** Un singleton figé au boot rendrait le
  redémarrage obligatoire pour changer un prix, ce qui annule l'objet de la décision.

- **Absent : silencieux. Présent mais rejeté : dit une fois, et lisible dans l'UI.** Une ligne refusée
  devient **inerte** — la clé retombe sur le tier suivant — et le rejet est nommé une fois au journal et
  en raison consultative sur les réglages : « une valeur que l'utilisateur a posée ne doit jamais cesser
  de compter en silence » (ADR-0015), et `journalctl` seul est un motif de panne récurrent de ce produit.
  Un cache fetché au marqueur de schéma inattendu est **entièrement** inerte.

- **Aucun des deux fichiers n'est seedé.** Seeder les lignes embarquées créerait un **instantané**
  (ADR-0031 §2) qu'une release future ajoutant une ligne au plancher se verrait masquer. La découvrabilité
  passe par les réglages, qui nomment les deux chemins même absents et affichent le millésime.

- **Le chemin est injecté, jamais le home lu globalement.** Corollaire : les prix sont un concept
  d'**instance** — l'hôte les porte même pour un Run sandboxé, contrairement à la racine des transcripts.

- **Deux seams d'exploitation** : `PDO_PRICE_SOURCE_URL` et `PDO_PRICE_SYNC=off`. Ce dernier est le seul
  env d'**opt-out** du crate, assumé : une feature qui doit marcher d'emblée ne peut pas être armée par
  variable d'environnement — l'opt-in réel est le premier clic.

## Conséquences

Un nouveau modèle se tarife sans intervention dès qu'un sync a tourné : la fenêtre d'ops passe d'un cycle
de release à un clic. Le chemin de lecture ne change pas de nature, donc les lectures répondent à
l'identique hors ligne. Une remise entreprise reste réparable par le tier manuel, et cette correction
**survit** au sync.

En contrepartie, une source tierce devient une dépendance de **correctitude des chiffres affichés** —
d'où le millésime visible. models.dev est communautaire et ne promet aucun versioning de schéma ; c'est
la raison pour laquelle le repli LiteLLM n'est pas décoratif. Le coût étant dérivé à la lecture, un sync
**retarife tous les Runs historiques, archivés inclus** : le chiffre d'un Run clos devient fonction d'un
fichier modifiable, à assumer au titre de l'étiquetage honnête d'ADR-0022.

**Limites connues, à ne pas confondre avec des bugs.** Le **mode fast est invisible** : il s'écrit du même
id de modèle dans les transcripts, donc un nœud en mode fast est sous-facturé ×2 et aucune normalisation
ne peut le rattraper. Sans dimension de date, un prix d'intro expiré surestime les lignes antérieures. Les
prix de cache restent **dérivés** (1.25× / 2× / 0.1× de l'input).

## Alternatives rejetées

- **OpenRouter comme source** (suggéré par le propriétaire, mesuré, écarté) : 9 collisions à prix
  divergents après normalisation, un `canonical_slug` dont l'ordre des mots s'inverse selon la génération,
  un SKU fantôme `claude-opus-4.7-fast` à $30/$150 qu'un normalisateur *last-wins* ferait gonfler ×6, et
  des CGU §7 qui interdisent de copier les informations du service — ce que fait littéralement un daemon
  qui persiste puis réaffiche des prix.
- **LiteLLM comme source primaire** : zéro collision aussi, MIT aussi. Écarté en v1 parce que ses clés ne
  sont pas le vocabulaire de PDO (alias à ordre inversé) et qu'il faut filtrer des centaines de clés
  `claude-*` d'autres providers. **Reste le repli documenté** : un adaptateur, pas une refonte.
- **Fetcher au build.** La distinction est load-bearing et sera confondue : fetcher **au build** fige la
  table dans le binaire et **ramène** le couplage à la release qu'on supprime ; fetcher **out-of-band vers
  un cache disque** ne le fait pas.
- **Fetcher sur le chemin de lecture** : ADR-0030 refuse déjà un aller-retour réseau dans un handler
  d'écriture, et une lecture de Run qui dépend d'Internet est exactement ce qu'ADR-0022 protégeait.
- **Inférer le prix du nom du modèle.** Faux, pas seulement fragile : `claude-opus-4-1` est à $15/$75 et
  `claude-opus-4-5` à $5/$25 — même famille, 3× l'écart. Il n'y a **rien à calculer**, seulement quelque
  chose à *savoir*.
- **Embarquer / shell-out ccusage.** Le motif « réseau » ne discrimine plus ; subsistent la dépendance
  binaire + Node et le fait que ccusage imposerait *sa* table plutôt que la nôtre.
- **Requête conditionnelle (etag)** : le fetch est manuel ou une fois par 24 h — conditionner n'achète rien.

## Hors-scope

- **Nommer le modèle non tarifé** dans l'UI (#425) : sans elle, l'utilisateur ne peut pas apprendre
  **quel** modèle manque — c'est ainsi que `claude-fable-5` est resté invisible.
- **Le mode fast** et le **palier long-contexte > 200K** : des **paliers**, pas des prix de famille ; et le
  premier n'a aucun signal côté transcript.
- **Une dimension de date par ligne.** La bonne horloge n'est pas l'heure courante : gater sur elle
  **retariferait l'histoire** et serait invisible de la clé du memo. Purement additif plus tard.

## Amendements

- **#527 — la génération courante entre au plancher embarqué.** Sur un install neuf jamais synchronisé, un
  Run sur un modèle de génération 5 (celui que le produit fait tourner **par défaut**) lisait `~$0.0000 †`.
  Le *principe de membership* du plancher s'élargit ; le mécanisme ne bouge pas. Ce n'est pas « amorcer »
  au sens interdit : amorcer = **matérialiser le `const` sur un fichier disque**, ce qui figerait un
  instantané. `sonnet-5` y est gravé à **$3/$15** (post-intro), pas au prix d'intro : le `const` ne peut
  pas être daté, et graver le prix d'intro serait faux pour toute la vie post-cutover de chaque release.
- **#528 — la table résolue est exposée en lecture.** Juxtaposer les tiers ne **rend** pas le tier
  **gagnant**, qui est un calcul de précédence, pas une lecture. Un tableau `resolved` (une entrée par
  famille : tier gagnant + `$/MTok`) est porté par `GET /stats/cost` — champ additif sur un endpoint déjà
  consommé, pas de route dédiée. Il lit **la même** `PriceTable` que le fold de coût, donc la vue ne peut
  jamais énumérer un ensemble que le tarificateur chiffrerait autrement.
