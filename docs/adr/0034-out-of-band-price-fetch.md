# La table de prix a une source distante, fetchée hors du chemin de lecture

## Contexte

ADR-0022 estime le coût d'un Run en multipliant les compteurs de tokens des transcripts locaux par
une **table de prix codée en dur** (11 lignes, source : page de prix Anthropic du 2026-07-06). Son
premier point de décision motive ce choix par « pas de réseau », et son hors-scope rejette le
rafraîchissement de prix live au motif « daemon network-free ». Le motif ne tient plus, pour deux
raisons distinctes.

**1. La table dérive plus vite qu'elle ne se corrige.** Recensement du 2026-07-30 sur les
2 189 transcripts de la machine de référence :

| modèle | lignes | tarifé par le `const` ? |
|---|---:|---|
| `claude-opus-4-8` | 79 941 | oui |
| **`claude-opus-5`** | **28 427** | **non → $0** |
| **`claude-fable-5`** | **6 587** | **non → $0** |
| `claude-haiku-4-5-20251001` | 591 | oui (dé-datage) |
| **`claude-sonnet-5`** | **607** | **non → $0** |
| `claude-opus-4-6` | 2 | oui |
| `<synthetic>` | 86 | tarifé $0 exprès |

**35 621 lignes sur ~116 200, soit ~30 %, ne sont pas tarifées** — le coût affiché est une borne
basse sur près d'un tiers de la dépense, et les graphes de #377 s'en trouvent faux. Le modèle le
plus **cher** de la liste, `claude-fable-5` à $10/$50, est celui que personne n'avait identifié : il
n'est nommé dans aucune issue, parce que le produit ne le dit nulle part. Corriger la table exige
d'éditer du Rust, de bumper, de releaser, et de mettre à jour le daemon de production — un chemin
qui, de fait, n'est pas pris (le retard de version du daemon de prod est structurel et récidivant).

**2. « Network-free » n'a jamais été littéral.** Trois egress préexistent, tous ratifiés : l'unité
systemd du daemon déclare une dépendance au réseau ; le daemon shelle les guards de Trigger sous son
propre environnement d'auth (l'exemple canonique documenté est `gh issue list`, un appel authentifié
à chaque tick cron) ; et la sandbox fait un `docker pull` vers GHCR (ADR-0030). Au-dessus de tout
ça, chaque nœud est une session `claude` : le produit ne fonctionne pas hors ligne. Ce qui était
vrai et doit rester vrai est plus étroit que « pas de réseau » : **la lecture d'un Run ne doit pas
dépendre d'Internet**.

Le propriétaire a ratifié la réouverture sur #427 (2026-07-30) : « Je veux que la table soit remplie
depuis le remote. […] soit appel au démarrage, soit bouton "sync coûts" depuis les stats. Il faut
regarder les sources disponibles, si possible utiliser OpenRouter. »

## Décision

Le daemon peut sortir sur Internet, **hors du chemin de lecture uniquement**, pour remplir un cache
de prix sur disque. La table de prix devient un empilement à trois tiers résolu **par clé de
famille** : `manuel → fetché → embarquée`. La lecture reste strictement locale : deux lectures de
fichiers et une constante.

```
fetch out-of-band (bouton, ou rafraîchissement au démarrage)
   → écrit le cache fetché sur disque
       → le calcul de coût lit le disque   (manuel → fetché → embarquée, FUSION PAR CLÉ)
```

### Ce qu'on décide

- **Trois tiers, deux fichiers, un seul écrivain par fichier.** Le tier **manuel**
  (`~/.pdo/prices/models.yaml`) : l'humain l'écrit, PDO n'y touche **jamais** et ne le seed jamais.
  Le tier **fetché** (`~/.pdo/prices/fetched.json`) : le daemon le réécrit **intégralement**
  (écriture atomique tmp + rename), et personne d'autre. La table embarquée reste le tier
  plancher. Deux formes écartées : un fichier unique réécrit par le sync **rejoue le défaut que
  l'issue combat** (il effacerait une correction à la main) ; un fichier à deux sections
  condamnerait le daemon à réécrire partiellement le fichier que l'humain édite, alors que ce
  codebase n'a aucun précédent de réécriture partielle sûre et deux précédents qui érigent le
  writer unique en règle (« un fait, un propriétaire », #447). C'est aussi la forme homomorphe à
  ADR-0015 : ajouter un tier, c'est ajouter un stockage, pas cohabiter dans celui d'un autre.

- **La table embarquée est un plancher, jamais une amorce.** `claude-opus-4-0`, `claude-sonnet-4-0`
  et `claude-3-5-haiku` sont dans le `const` et **absentes des trois sources distantes examinées**
  (models.dev et LiteLLM les ont purgées de leur namespace `anthropic`, OpenRouter a délisté
  3.5-haiku). Le `const` est donc le **seul** tarificateur de ces familles, pas un jeu de données
  jetable qu'un sync remplacerait. **Amendé par #527** (voir l'amendement en fin d'ADR) : le `const`
  tarife désormais **aussi** les familles de la génération courante (gen-5), en plancher, toujours
  surchargées par un sync — le *principe de membership* s'élargit, le mécanisme (fusion par clé, rien
  de seedé sur disque) ne bouge pas.

- **Fusion par clé, jamais remplacement.** Une clé présente dans un tier gagne ; une clé absente
  garde ce que le tier suivant en dit. Sous remplacement global, oublier `claude-opus-4-8`
  effacerait **79 941** lignes sur ~116 200 : un bug de prix faux converti en blackout total. Et le
  fichier **gèlerait** la table — une release ajoutant une ligne deviendrait invisible. Analogie
  maison : ADR-0031 §2, « un profil est un **diff**, jamais un instantané ».

- **models.dev est la source, et OpenRouter est rejeté.** `GET https://models.dev/api.json`, en ne
  lisant que le namespace `anthropic`. Le critère décisif n'est ni la fraîcheur (les trois sources
  ont publié `claude-opus-5` dans l'heure de sa sortie) ni la licence : c'est que **les clés de
  models.dev sont déjà le vocabulaire de PDO** — l'id de l'API Anthropic, tirets compris — donc la
  normalisation se réduit à un dé-datage, et le risque de *mauvais mapping* (bien pire qu'une ligne
  manquante : il produit un nombre plausible et faux) tombe à zéro. Les prix y sont déjà en $/MTok
  et en nombre, ce qui supprime la classe d'erreur du facteur 10⁶.
  OpenRouter, suggéré par le propriétaire, a été **mesuré et écarté** : normaliser ses ids vers une
  clé de famille produit **9 collisions à prix divergents** (`claude-opus-4-8` sort à la fois
  $2.5/$12.5, $5/$25 et $10/$50 selon la variante `:batch` / `-fast` / alias), son `canonical_slug`
  est inutilisable (l'ordre des mots s'inverse selon la génération — `claude-4.8-opus-20260528`
  mais `claude-opus-5-20260723`), et il expose un SKU fantôme `claude-opus-4.7-fast` à **$30/$150**
  pour un mode que la doc Anthropic déclare indisponible sur ce modèle — un normalisateur
  *last-wins* gonflerait opus-4-7 **×6**. models.dev et LiteLLM produisent **zéro** collision. En
  prime, les CGU §7 d'OpenRouter interdisent de « scrape or copy any information on the Site or the
  Services », ce que fait littéralement un daemon qui persiste puis réaffiche des prix ; models.dev
  et LiteLLM sont MIT.
  **Anthropic n'expose pas ses prix** : son API de modèles ne rend aucun champ tarifaire. La page
  de pricing reste le **juge d'appel**, pas une source machine.
  **Une seule source, un seul parseur en v1.** L'URL est surchargeable, mais le parseur est de
  forme models.dev : pointer l'URL sur LiteLLM produit une moisson vide, donc un refus explicite,
  pas un silence.

- **Le dé-datage est asymétrique, et c'est voulu.** Sur le tier **manuel**, une clé datée est
  **refusée** en imprimant la forme correcte : stripper collapserait silencieusement deux lignes
  que l'auteur voulait distinctes, et le refus enseigne. Sur le chemin **fetché**, l'identifiant
  est **dé-daté** : la source expose des ids datés, les refuser jetterait `claude-haiku-4-5` —
  591 lignes, et c'est justement la forme que les transcripts écrivent. On contrôle la
  transformation, et l'invariant est vérifié : **0 collision** après dé-datage sur la source. Une
  collision à prix divergents fait **tomber la clé entière** du tier fetché, nommée — c'est un
  défaut de source, pas un cas à arbitrer par heuristique (posture de #395 : jamais de faux verdict
  « synchronisé »). Le suffixe `-fast` n'est **jamais** stripé : le stripper créerait la collision,
  le garder produit une clé qui ne matche aucun transcript — coût nul, aucune fausse déflation.

- **Un sync qui ne change rien n'écrit rien.** Si les lignes normalisées sont identiques à celles
  déjà sur disque, le fichier n'est pas réécrit : réponse noop + raison (ADR-0025) et l'empreinte
  de la table ne bouge pas, donc le memo de coût reste chaud pour tous les Runs. Prix payé : le
  millésime n'avance pas sur un noop, donc le rafraîchissement au démarrage re-demandera la source
  après 24 h pour ne rien écrire — un GET par démarrage au pire, contre une invalidation complète
  du memo à chaque sync. L'égalité des lignes est la preuve que rien n'avait à changer.

- **Le garde numérique s'applique aux DEUX tiers disque.** Le tier fetché est écrit par le daemon,
  qui a validé à l'écriture — mais le nom du fichier est la seule chose qui en interdit l'édition à
  la main. Un prix négatif ou non fini y est refusé exactement comme dans le tier manuel (ligne
  inerte, clé retombant sur le tier suivant), parce qu'un NaN empoisonne le total **et** sérialise
  en JSON `null` vers un frontend qui attend un nombre. Les règles de **clé** (dé-datage,
  sentinelle), elles, restent asymétriques comme décrit ci-dessus.

- **Une moisson vide est un échec, pas un résultat.** Une dérive de schéma chez la source écrirait
  sinon un cache vide qui **détruirait la dernière table connue**. Le garde « zéro ligne Anthropic
  → on n'écrit rien » est principiel ; tout autre plancher serait un nombre magique. C'est le seul
  chemin par lequel cette feature pourrait *détruire* quelque chose.

- **Deux déclencheurs, deux postures d'échec.** Le contrat d'egress d'ADR-0030 se transpose :
  (1) le local précède toujours le réseau — *table sur disque avant fetch*, comme l'inspection
  d'image locale précède le pull ; (2) un échec réseau retombe sur un chemin qui produit la même
  chose — *les tiers déjà présents*, comme le fallback build ; (3) **sauf** quand l'effet a été
  explicitement demandé, et là c'est une erreur **dure qui nomme** la source. D'où : le **bouton**
  échoue en **502 nommant l'URL** ; le **rafraîchissement au démarrage** échoue en un warn, jamais
  fatal — même régime que la boot recovery, à cause des courses d'ordonnancement du boot.

- **Le démarrage rafraîchit, il n'amorce pas.** Même armé, le fetch de boot ne se déclenche **que**
  si le cache fetché existe déjà et a plus de 24 heures. **Aucun egress avant que l'utilisateur ait
  cliqué « Sync coûts » une première fois** — le clic **est** le consentement. Motif : ADR-0001
  (« défaut réversible et additif — relâcher plus tard ne surprend personne, l'inverse oui ») et
  ADR-0012 (l'autonomie se gagne), dont le seul précédent de polarité — l'auto-complétion sur fin
  de tour, ADR-0032 — est un opt-in. La tâche est détachée et posée après le démarrage du serveur :
  elle ne retarde jamais la première requête acceptée.

- **La lecture reste locale, par requête, sans cache global figé.** La table se charge une fois par
  requête au bord, jamais dans la boucle par Run. Un singleton figé au boot rendrait le redémarrage
  obligatoire pour changer un prix, ce qui annule l'objet de la décision ; et deux lectures de
  quelques Ko n'achètent pas un cache TTL.

- **L'empreinte de la table entre dans la clé du memo de coût** (`(run_id, mtime-max, empreinte)`).
  Sans ce troisième composant, un sync ne bougerait **aucun** mtime de transcript, donc l'endpoint
  de stats agrégées (mémoïsé) servirait les anciens dollars **jusqu'au redémarrage** pendant que le
  détail d'un Run (non mémoïsé) dirait vrai. Deux surfaces qui se contredisent est pire que l'une
  des deux fausse — et les Runs concernés, terminés et aux transcripts figés, sont exactement ceux
  qu'un sync existe pour réparer. Le sync **ne vide pas** le memo : sous la nouvelle clé une entrée
  périmée devient inatteignable, et vider invaliderait aussi les Runs dont les prix n'ont pas
  bougé.

- **Absent : silencieux. Présent mais rejeté : dit une fois, et lisible dans l'UI.** Un fichier
  absent est l'état normal de toute instance — pas même une ligne de log. Un fichier présent mais
  illisible, ou une ligne refusée (clé datée, sentinelle, prix invalide), devient **inerte** — la
  clé retombe sur le tier suivant, elle ne détruit pas l'estimation — et le rejet est nommé **une
  fois** dans le journal et en raison consultative sur les réglages. Principe : « une valeur que
  l'utilisateur a posée ne doit jamais cesser de compter en silence » (ADR-0015 #471), et
  `journalctl` seul est le motif de panne récurrent de ce produit (#497, #485). Un cache fetché
  dont le marqueur de schéma n'est pas celui attendu est **entièrement** inerte — jamais de lignes
  lues sous un schéma non reconnu (précédent : le marqueur d'algo de hash de la bibliothèque).

- **Une ligne rejetée retombe sur le tier suivant, elle ne détruit pas l'estimation.** Un typo sur
  `claude-opus-4-8` ne doit pas effondrer 79 941 lignes. Si aucun tier ne connaît la clé, le modèle
  reste non tarifé — $0 + `partial`, comportement ADR-0022, rien de neuf.

- **Aucun des deux fichiers n'est seedé.** Seeder les lignes embarquées créerait un **instantané**
  (ADR-0031 §2) : le jour où une release ajoute une ligne au plancher, un fichier seedé la
  masquerait. La découvrabilité passe entièrement par les réglages, qui nomment les deux chemins
  même quand les fichiers sont absents, et affichent le millésime du dernier fetch.

- **Le chemin est injecté, jamais le home lu globalement** — le régime global a un coût chiffré
  ailleurs dans le crate (un verrou de test partagé par des dizaines de tests), et une slice a déjà
  été payée pour en sortir le calcul de coût (#408). Corollaire : le seam #408 déplace la racine
  des **transcripts**, pas celle des **prix** — les prix sont un concept d'**instance**, l'hôte les
  porte même pour un Run sandboxé.

- **Deux seams d'exploitation, contrats publics** : l'URL de la source est surchargeable par
  l'environnement (`PDO_PRICE_SOURCE_URL`), et le rafraîchissement de boot est désarmable par
  `PDO_PRICE_SYNC=off` — le seul env d'**opt-out** du crate, assumé comme tel : une feature qui
  doit marcher d'emblée ne peut pas être armée par variable d'environnement, l'opt-in réel est le
  premier clic.

## Conséquences

- **Positif.** Un nouveau modèle se tarife **sans intervention** dès qu'un sync a tourné, et la
  fenêtre d'ops passe d'un cycle de release à un clic. Le chemin de lecture ne change pas de
  nature, donc les lectures répondent à l'identique hors ligne comme en ligne. Une remise
  entreprise, ou un modèle qu'aucune source ne publie, reste réparable par le tier manuel — et
  cette correction **survit** au sync.

- **Négatif / assumé.** Le daemon a un egress de plus, et une source tierce devient une dépendance
  de **correctitude des chiffres affichés**. Doctrine applicable : celle d'ADR-0013 (« la version
  fait partie de la frontière de sécurité ») — sauf qu'ici ce sont les **valeurs** qui sont
  load-bearing. D'où le millésime visible : la fraîcheur de la table est lisible, pas devinée.
  models.dev est communautaire et a changé d'organisation GitHub sans promesse de versioning du
  schéma ; c'est le prix de la propreté de ses clés, et la raison pour laquelle le repli LiteLLM
  n'est pas décoratif. Le coût étant dérivé à la lecture, un sync **retarife tous les Runs
  historiques, archivés inclus** : cela satisfait « un Run archivé s'ouvre et se chiffre », mais le
  chiffre d'un Run clos devient fonction d'un fichier modifiable — à assumer au titre de
  l'étiquetage honnête d'ADR-0022.

- **Limites connues, à ne pas confondre avec des bugs.** **Le mode fast est invisible** : le mode
  normal et le mode fast s'écrivent du même id de modèle dans les transcripts, donc un nœud en mode
  fast est sous-facturé ×2 et aucune normalisation ne peut le rattraper. **Le prix d'intro de
  `claude-sonnet-5`** expire le 2026-08-31 et aucune des trois sources ne porte de date d'effet :
  sans dimension de date (délibérément absente, voir hors-scope), la remontée surestimera de 50 %
  les 607 lignes antérieures, soit ~0,5 % d'un nombre déjà préfixé `~`. **Les prix de cache restent
  dérivés** (1.25× / 2× / 0.1× de l'input) : models.dev les confirme ligne par ligne et n'a pas de
  split 5m/1h.

## Alternatives rejetées

- **OpenRouter comme source.** 9 collisions à prix divergents après normalisation, un slug
  canonique à l'ordre des mots instable, un SKU fantôme à $30/$150, et des CGU qui interdisent la
  copie des informations du service. Suggéré par le propriétaire, mesuré, écarté — avec les preuves
  ci-dessus.
- **LiteLLM comme source primaire.** Zéro collision aussi, MIT aussi, et l'argument propre que
  ccusage lit exactement ce fichier. Écarté en v1 pour une seule raison : ses clés ne sont pas le
  vocabulaire de PDO (alias à ordre inversé) et des centaines de clés `claude-*` d'autres providers
  doivent être filtrées. **Reste le repli documenté** si models.dev disparaît : un adaptateur, pas
  une refonte.
- **Fetcher au build** (déjà rejeté par ADR-0022, toujours rejeté). La distinction est load-bearing
  et sera confondue : fetcher **au build** fige la table dans le binaire et **ramène** le couplage
  à la release qu'on supprime ; fetcher **out-of-band vers un cache disque** ne le fait pas.
- **Fetcher sur le chemin de lecture** (paresseusement, à la première lecture de coût). Interdit :
  ADR-0030 refuse déjà un aller-retour réseau dans un handler d'écriture ; une lecture de Run qui
  dépend d'Internet est exactement ce que le motif d'ADR-0022 protégeait, sous sa forme forte.
- **Un fichier unique réécrit par le sync**, ou **un fichier à deux sections** — cf. première puce.
- **Remplacement global au lieu d'une fusion par clé.** Chiffré : oublier `claude-opus-4-8`
  effacerait 79 941 lignes sur ~116 200, et gèlerait la table contre les releases futures.
- **Inférer le prix du nom du modèle.** Faux, pas seulement fragile : `claude-opus-4-1` et
  `claude-opus-4-0` sont à $15/$75, `claude-opus-4-5` à `4-8` à $5/$25 — même famille, 3× l'écart.
  Il n'y a **rien à calculer**, seulement quelque chose à *savoir*.
- **Embarquer / shell-out ccusage.** Toujours rejeté, mais le motif change : « réseau » n'est plus
  discriminant. Ce qui subsiste est la dépendance binaire + Node, et le fait que ccusage imposerait
  *sa* table plutôt que la nôtre.
- **Vider le memo au sync** au lieu de mettre l'empreinte dans sa clé. Redondant sous la nouvelle
  clé, et strictement moins bon : cela invaliderait aussi les Runs dont les prix n'ont pas bougé.
- **Requête conditionnelle (etag)** : ADR-0015 #471 interdit les champs morts ; le fetch est manuel
  ou une fois par 24 heures — conditionner n'achète rien en v1.

## Hors-scope (suivis à filer)

- **Nommer le modèle non tarifé** dans l'UI : **#425**. Sans elle, l'utilisateur reste incapable
  d'apprendre **quel** modèle manque quand aucun tier ne le connaît — et c'est ainsi que
  `claude-fable-5` est resté invisible.
- **Un adaptateur LiteLLM** — le repli nommé ci-dessus.
- **Le mode fast** et le **palier long-contexte > 200K** : des **paliers**, pas des prix de
  famille ; ni un multiplicateur ni une ligne de plus ne les exprime, et le premier n'a aucun
  signal côté transcript.
- **Une dimension de date par ligne.** La bonne horloge n'est pas l'heure courante : gater sur elle
  **retariferait l'histoire** — le coût d'un Run terminé changerait sans que ses transcripts
  changent — et serait invisible de la clé du memo. Purement additif plus tard.

## Amendement — La génération courante entre au plancher embarqué (#527, fork de #425)

Le grill de #425 a isolé une question qui **révise le principe de membership de D2** et ne devait pas
être exécutée en autonome : faut-il **amorcer** `const PRICES` avec la génération 5 (`claude-opus-5`,
`claude-sonnet-5`, `claude-fable-5`) pour qu'un montant de coût soit **non nul, hors ligne, d'emblée** —
sans clic « Sync coûts » ni `models.yaml` ? Le propriétaire a tranché le 2026-08-13 : « **On amende** »
(issue #527).

**Le symptôme corrigé.** Sur une instance jamais synchronisée et sans `models.yaml` — l'état par défaut
d'un install neuf, et le seul mode strictement hors ligne — un Run sur un modèle de génération 5 (celui
que le produit fait tourner **par défaut**) lisait son coût `~$0.0000 †`. ~30 % de la dépense lue à $0
pour cette seule raison ; le tier fetché la corrige, mais **seulement après le premier clic**, et le
retard de version du daemon de production rend ce clic tardif.

**Ce qui change.** La table embarquée tarife désormais **ce qu'aucun remote ne porte + les familles de
la génération courante**, en plancher, **toujours surchargées par un sync**. C'est le *principe de
membership* de D2 qui s'élargit — la gen-5 est le cas inverse (models.dev la porte), et on l'ajoute quand
même, en plancher.

**Pourquoi ce n'est pas « amorcer » au sens interdit.** « Amorcer » dans D2 / §9 = **matérialiser le
`const` sur un fichier disque** (seeder un `.json`), ce qui figerait un instantané qu'une release future
masquerait. Ajouter des lignes au `const` n'est pas ça : `builtin()` reste pur, la fusion par clé est
intacte (un sync gagne toujours par clé), **aucun fichier n'est seedé**, et une release qui ajoute une
ligne au `const` reste visible. La table est un plancher, pas un miroir des prix fetchables.

**`sonnet-5` = $3/$15, pas $2/$10.** Le `const` ne peut pas être daté (délibéré). Le prix d'intro
($2/$10) expire le **2026-08-31** ; graver $3/$15 (post-intro) n'est faux que pour les lignes sonnet-5
**pré-cutover** des instances **jamais synchronisées** (~0,5 %, dérive déjà ratifiée par cette ADR pour
le tier fetché) et se **corrige au premier sync**. Graver $2/$10 serait faux pour toute la vie
post-31-août de chaque release. Critère fermé par des tests couche 1 sur `price_table.rs` et
`run_cost.rs` (ADR-0004).

## Amendement — La table résolue exposée en lecture (#528)

Le hors-scope « un endpoint exposant le tier gagnant par modèle » est **réalisé**, et son argument de
suffisance était faux : juxtaposer `manual_keys` et `fetched_rows` ne **rend** pas le tier **gagnant**,
qui est un calcul de précédence par clé de famille, pas une lecture. La table **résolue** est désormais
exposée en lecture comme un tableau **`resolved`** (une entrée par famille : tier gagnant + `$/MTok`),
**porté par `GET /stats/cost`** et rendu dans l'onglet **Stats → Cost**, à côté de « Sync costs » : on
synchronise et on lit ce que PDO sait tarifer au même endroit. **Pas** de route `GET /prices` dédiée —
champ additif rétro-compatible sur un endpoint déjà consommé par cet onglet (zéro taxe proxy vite dev).
Il lit **la même** `PriceTable` que le fold de coût, donc la vue ne peut jamais énumérer un ensemble que
le tarificateur chiffrerait autrement (#373). Purement additif, lecture seule, dans le cadre de cet ADR —
**pas de nouvel ADR**.
