# La table de prix a une source distante, fetchée hors du chemin de lecture

## Contexte

ADR-0022 estime le coût d'un Run en multipliant les compteurs de tokens des transcripts locaux par une
**table de prix codée en dur** (`run_cost.rs:48`, 11 lignes, source « page de prix Anthropic, fetched
2026-07-06 »). Son premier point de décision motive ce choix par « pas de réseau », et son hors-scope
rejette explicitement le « rafraîchissement de prix live (LiteLLM / models.dev) — rejeté (daemon
network-free) ».

Le motif ne tient plus, pour deux raisons distinctes.

**1. La table dérive plus vite qu'elle ne se corrige.** Recensement du 2026-07-30 sur les
2 189 transcripts de la machine de référence, champ `message.model` :

| modèle | lignes | tarifé par le `const` ? |
|---|---:|---|
| `claude-opus-4-8` | 79 941 | oui |
| **`claude-opus-5`** | **28 427** | **non → $0** |
| **`claude-fable-5`** | **6 587** | **non → $0** |
| `claude-haiku-4-5-20251001` | 591 | oui (dé-datage) |
| **`claude-sonnet-5`** | **607** | **non → $0** |
| `claude-opus-4-6` | 2 | oui |
| `<synthetic>` | 86 | tarifé $0 exprès |

**35 621 lignes sur ~116 200, soit ~30 %, ne sont pas tarifées** — le coût affiché est une borne basse
sur près d'un tiers de la dépense, et les graphes de #377 s'en trouvent faux. Le modèle le plus **cher**
de la liste, `claude-fable-5` à $10/$50, est celui que personne n'avait identifié : il n'est nommé dans
aucune issue, parce que le produit ne le dit nulle part. Corriger la table exige aujourd'hui d'éditer du
Rust, de bumper, de releaser, et de lancer `make update` sur le daemon de production — un chemin qui,
de fait, n'est pas pris (le retard de version du daemon de production est structurel et récidivant, et
`make update` ne peut pas être lancé depuis un nœud puisque sa dernière ligne redémarre le daemon qui
porte le Run).

**2. « Network-free » n'a jamais été littéral.** Trois egress préexistent, tous ratifiés :
`service_unit.rs:49-50` déclare `After=network-online.target` / `Wants=network-online.target` sur
l'unité du daemon ; le daemon shelle les guards de Trigger sous son propre environnement d'auth, et
l'exemple canonique documenté est `gh issue list` (`CONTEXT.md:593,597`) — un appel à
`api.github.com` avec credentials, à chaque tick cron ; et `sandbox_image.rs:320-332` fait un
`docker pull` vers GHCR. Au-dessus de tout ça, chaque nœud est une session `claude` : le produit ne
fonctionne pas hors ligne, et ADR-0004:16 l'assume.

Ce qui était vrai et qui doit rester vrai est plus étroit que « pas de réseau » : **un `GET /runs/:id`
ne doit pas dépendre d'Internet**.

Le propriétaire a ratifié la réouverture sur l'issue #427 (2026-07-30) : « Je veux que la table soit
remplie depuis le remote. L'idée étant que si demain il y a un nouveau modèle, alors la table l'embarque.
Pour la remplir, soit appel au démarrage, soit bouton "sync coûts" depuis les stats. Il faut regarder
les sources disponibles, si possible utiliser OpenRouter. »

## Décision

Le daemon peut sortir sur Internet, **hors du chemin de lecture uniquement**, pour remplir un cache de
prix sur disque. La table de prix devient un empilement à trois tiers résolu **par clé de famille** :
`manuel → fetché → embarquée`. La lecture reste strictement locale : deux `fs::read` et un `const`.

```
fetch out-of-band (bouton, ou rafraîchissement au démarrage)
   → écrit ~/.pdo/prices/fetched.json
       → le calcul de coût lit le disque   (manuel → fetché → embarquée, FUSION PAR CLÉ)
```

### Ce qu'on décide

- **Trois tiers, deux fichiers, un seul écrivain par fichier.**
  `~/.pdo/prices/models.yaml` est le tier **manuel** : l'humain l'écrit, PDO n'y touche **jamais** et
  ne le seed jamais. `~/.pdo/prices/fetched.json` est le tier **fetché** : le daemon le réécrit
  **intégralement** (tmp + `rename` dans le même répertoire, idiome de `sandbox_staging.rs:790`), et
  personne d'autre. `const PRICES` reste le tier **embarqué**.
  Deux formes ont été écartées. Un fichier unique réécrit par le sync **rejoue le défaut que l'issue
  combat** — il effacerait une correction à la main. Un fichier à deux sections condamnerait le daemon
  à réécrire partiellement le fichier que l'humain édite, alors que ce codebase a **zéro** précédent de
  réécriture partielle sûre et deux précédents qui disent pourquoi : le `duplicate` de pipeline
  réécrit le YAML « verbatim sauf la ligne `name:` de colonne 0, jamais re-sérialisé, pour préserver
  clés top-level inconnues, commentaires et ordre des champs » (`CONTEXT.md:1611`), et
  `sandbox_staging.rs:411` érige le **writer unique** en règle, rattachée par ADR-0031:282 à #447
  « un fait, un propriétaire ». Deux fichiers font **disparaître** le problème au lieu de le résoudre.
  C'est aussi la forme homomorphe à ADR-0015, où chaque tier a son propre stockage (SQLite / env du
  process / `const`) : ajouter un tier, c'est ajouter un stockage, pas cohabiter dans celui d'un autre.

- **La table embarquée est un plancher, jamais une amorce.** `claude-opus-4-0` ($15/$75),
  `claude-sonnet-4-0` ($3/$15) et `claude-3-5-haiku` ($0.80/$4) sont dans le `const` et **absentes des
  trois sources distantes examinées** — models.dev et LiteLLM les ont purgées de leur namespace
  `anthropic`, OpenRouter a délisté 3.5-haiku tout court. Le `const` est donc le **seul tarificateur**
  de ces familles, pas un jeu de données jetable qu'un sync remplacerait.

- **Fusion par clé, jamais remplacement.** Une clé présente dans un tier gagne ; une clé absente garde
  ce que le tier suivant en dit. Sous remplacement global, oublier `claude-opus-4-8` effacerait
  **79 941** lignes sur ~116 200 : un bug de prix faux converti en blackout total. Et le fichier
  **gèlerait** la table — une release ajoutant une ligne deviendrait invisible. Analogie maison :
  ADR-0031 §2, « un profil est un **diff**, jamais un instantané ».

- **models.dev est la source, et OpenRouter est rejeté.** `GET https://models.dev/api.json`, en ne
  lisant que `root["anthropic"]["models"]`. Le critère décisif n'est ni la fraîcheur (les trois sources
  ont publié `claude-opus-5` dans l'heure de sa sortie) ni la licence : c'est que **les clés de
  models.dev sont déjà le vocabulaire de PDO** — l'id de l'API Anthropic, tirets compris — donc la
  normalisation se réduit à un dé-datage, et le risque de *mauvais mapping* (bien pire qu'une ligne
  manquante : il produit un nombre plausible et faux) tombe à zéro. Les prix y sont déjà en **$/MTok**
  et en `number`, ce qui supprime la classe d'erreur du facteur 10⁶.
  OpenRouter, suggéré par le propriétaire, a été mesuré et écarté : normaliser ses ids vers une clé de
  famille produit **9 collisions à prix divergents** (`claude-opus-4-8` sort à la fois $2.5/$12.5,
  $5/$25 et $10/$50 selon la variante `:batch` / `-fast` / alias), son `canonical_slug` est
  inutilisable (l'ordre des mots s'inverse selon la génération — `claude-4.8-opus-20260528` mais
  `claude-opus-5-20260723`), et il expose un SKU fantôme `anthropic/claude-opus-4.7-fast` à
  **$30/$150** pour un mode que la doc Anthropic déclare indisponible sur ce modèle — un normalisateur
  *last-wins* gonflerait opus-4-7 **×6**. models.dev et LiteLLM produisent **zéro** collision. En prime,
  les CGU §7 d'OpenRouter interdisent de « scrape or copy any information on the Site or the Services »,
  ce que fait littéralement un daemon qui persiste puis réaffiche des prix ; models.dev et LiteLLM sont
  **MIT**.
  **Anthropic n'expose pas ses prix** : `GET /v1/models` rend `id`, `capabilities`, `created_at`,
  `display_name`, `max_input_tokens`, `max_tokens`, `type` — aucun champ tarifaire, et 401 sans clé. La
  page de pricing reste le **juge d'appel**, pas une source machine.
  **Une seule source, un seul parseur en v1.** L'URL est surchargeable, mais le parseur est de forme
  models.dev : pointer l'URL sur LiteLLM produit une moisson vide, donc un refus explicite, pas un
  silence.

- **Le dé-datage est asymétrique, et c'est voulu.** Sur le tier **manuel**, une clé datée est
  **refusée** en imprimant la forme correcte : stripper collapserait silencieusement deux lignes que
  l'auteur voulait distinctes, et le refus enseigne. Sur le chemin **fetché**, l'identifiant est
  **dé-daté** : la source expose des ids datés (`claude-opus-4-5-20251101`,
  `claude-haiku-4-5-20251001`), les refuser jetterait `claude-haiku-4-5` — 591 lignes, et c'est
  justement la forme que les transcripts écrivent. On contrôle la transformation, et l'invariant est
  vérifié : **0 collision** après dé-datage sur `models.dev/anthropic`. Une collision à prix divergents
  fait **tomber la clé entière** du tier fetché, nommée — c'est un défaut de source, pas un cas à
  arbitrer par heuristique (posture de #395 : « jamais de faux verdict `synced` »,
  `library_store.rs:397-399`). Le suffixe `-fast` n'est **jamais** stripé : le stripper créerait la
  collision, le garder produit une clé qui ne matche aucun transcript — coût nul, aucune fausse
  déflation.

- **Un sync qui ne change rien n'écrit rien.** Si les lignes normalisées sont identiques à celles déjà
  dans `fetched.json`, le fichier n'est pas réécrit : la réponse est un `noop: true` + `reason`
  (ADR-0025) et l'**empreinte de la table ne bouge pas**, donc `COST_MEMO` reste chaud pour tous les
  Runs. Le prix payé est que `fetched_at` n'avance pas sur un noop, donc le rafraîchissement au
  démarrage re-demandera la source après 24 h pour ne rien écrire — un `GET` par démarrage au pire,
  contre une invalidation complète du memo à chaque sync. L'arbitrage penche du côté du memo :
  l'égalité des lignes est la preuve que rien n'avait à changer.

- **Le garde numérique s'applique aux DEUX tiers disque.** Le tier fetché est écrit par le daemon, qui
  a validé à l'écriture — mais le nom du fichier est la seule chose qui en interdit l'édition à la
  main. Un prix négatif ou non fini y est donc refusé exactement comme dans le tier manuel (ligne
  inerte, clé retombant sur le tier suivant), parce qu'un `NaN` empoisonne `usd` **et** sérialise en
  JSON `null` vers un frontend qui le type `number`. Les règles de **clé** (dé-datage, sentinelle),
  elles, restent asymétriques comme décrit ci-dessus.

- **Une moisson vide est un échec, pas un résultat.** Une dérive de schéma chez models.dev écrirait
  sinon un `fetched.json` vide qui **détruirait la dernière table connue**. Le garde « zéro ligne
  Anthropic → on n'écrit rien » est principiel ; tout autre plancher serait un nombre magique. C'est le
  seul chemin par lequel cette feature pourrait *détruire* quelque chose.

- **Deux déclencheurs, deux postures d'échec.** Le contrat d'egress d'ADR-0030 se transpose en trois
  clauses : (1) le local précède toujours le réseau — `image inspect` avant `docker pull`
  (`sandbox_image.rs:491`, « FAST PATH — précède TOUT réseau … offline-safe ») devient *table sur
  disque avant fetch* ; (2) un échec réseau retombe sur un chemin qui produit la même chose —
  `fallback build` (ADR-0030:72-74) devient *les tiers déjà présents* ; (3) **sauf** quand l'effet a
  été explicitement demandé, et là c'est une erreur **dure qui nomme** la source (ADR-0030:107-112,
  « un `docker pull` en échec est une erreur DURE qui NOMME le ref, jamais un build silencieux »).
  D'où : le **bouton** échoue en **502 nommant l'URL** ; le **rafraîchissement au démarrage** échoue en
  un `warn!`, jamais fatal — régime de `boot_recovery.rs:161-168`, dont le commentaire justifie
  exactement ce choix par les courses d'ordonnancement du boot (`service_unit.rs` émet
  `After=network-online.target` sans `After=docker.service`).

- **Le démarrage rafraîchit, il n'amorce pas.** Même armé, le fetch de boot ne se déclenche **que** si
  `fetched.json` existe déjà et a plus de 24 heures. **Aucun egress avant que l'utilisateur ait cliqué
  « Sync coûts » une première fois** — le clic **est** le consentement. Motif : ADR-0001:11, « défaut
  Z, réversible et additif — relâcher vers Y/X plus tard ne surprend personne, l'inverse oui », et
  ADR-0012 (l'autonomie se gagne), dont le seul précédent de polarité,
  `AUTOCOMPLETE_TURN_END_DEFAULT = false` (`stale_detector.rs:74`), est un opt-in. La tâche est
  **détachée** et posée **après** `tokio::spawn(axum::serve(...))` (`lib.rs:1815`), enveloppée dans
  `run_isolated` (`lib.rs:3702`) : elle ne retarde jamais le premier `accept()`. Contrairement à
  `boot_recovery`, elle n'est **pas** `await`ée avant `build_router`.

- **La lecture reste locale, par requête, sans `OnceLock`.** La table se charge une fois par requête au
  **bord** — là où `(home_root, sandbox_root)` est déjà résolu (`lib.rs:7943`, `stats.rs:406`) — jamais
  dans la boucle par Run. Un `OnceLock` figerait l'empreinte au boot et rendrait le redémarrage
  obligatoire pour changer un prix, ce qui annule l'objet de la décision. Un champ d'`AppState` avec
  TTL n'a pour précédent que `docker_probe_cache` (`lib.rs:311`), motivé par un aller-retour Docker :
  deux `fs::read` de quelques Ko ne l'achètent pas.

- **L'empreinte de la table entre dans la clé du memo.** `CostMemoKey` passe de `(run_id, mtime)` à
  `(run_id, mtime, empreinte)`. Sans ce troisième composant, un sync ne bougerait **aucun** mtime de
  transcript, donc `GET /stats/cost` (mémoïsé, `stats.rs:450`) servirait les anciens dollars **jusqu'au
  redémarrage** pendant que `GET /runs/:id` (non mémoïsé, `lib.rs:11520`) dirait vrai. Deux surfaces qui
  se contredisent est pire que l'une des deux fausse — et les Runs concernés, terminés et aux
  transcripts figés, sont exactement ceux qu'on veut réparer. Le sync **ne vide pas** le memo : sous la
  nouvelle clé une entrée périmée devient inatteignable, et vider invaliderait aussi les Runs dont les
  prix n'ont pas bougé.

- **Absent : silencieux. Présent mais rejeté : dit une fois, et lisible dans l'UI.** Un fichier absent
  est l'état normal de toute instance — pas même une ligne de log. Un fichier présent mais illisible ou
  non parsable, ou une ligne refusée, produit **un** `warn!` et un `reason` consultatif sur
  `GET /settings`. ADR-0015 amendement #471 : « une valeur que l'utilisateur a posée ne doit jamais
  cesser de compter en silence » ; ADR-0001:11 classe la perte de config silencieuse dans les
  diagnostics **toujours visibles**. Le précédent exact est #432 : `PDO_DEFAULT_SANDBOX` « passe par
  aucun validateur par construction, donc le seul endroit honnête pour le remonter est ici, en `reason`
  consultatif à côté de la valeur » (`lib.rs:6894-6898`). Et `journalctl` seul est le motif de panne
  récurrent de ce produit (#497, #485). Le chargeur mémorise la dernière empreinte avertie pour ne pas
  émettre une ligne par requête de `/stats/cost`.
  **Un `fetched.json` dont le `schema` n'est pas `prices-v1` est entièrement inerte.** Jamais de lignes
  lues sous un schéma non reconnu — précédent `hash_algo: "semantic-v1"` (`library_store.rs:259-269`),
  dont le commentaire dit que « changer l'algorithme sans ce marqueur aurait fait passer **tous** les
  pipelines promus en ⚠, exactement le symptôme qu'on corrige ».

- **Une ligne rejetée retombe sur le tier suivant, elle ne détruit pas l'estimation.** Un typo sur
  `claude-opus-4-8` ne doit pas effondrer 79 941 lignes. La clé garde ce que le tier suivant en dit, et
  l'inertie **se dit**. Si aucun tier ne connaît la clé, le modèle reste non tarifé — `$0` + `partial`,
  comportement actuel, rien de neuf.

- **Aucun des deux fichiers n'est seedé.** Seeder les 11 lignes embarquées créerait un **instantané**
  (ADR-0031 §2) : le jour où une release ajoute une ligne au `const`, un fichier seedé la masquerait.
  « Ne rien poser est un état de première classe, et le défaut » (ADR-0031 §9). La découvrabilité passe
  donc **entièrement** par le bloc `GET /settings`, qui nomme les deux chemins **même quand les
  fichiers sont absents**, et affiche le millésime du dernier fetch.

- **Le chemin est injecté, jamais `$HOME` lu.** `price_table::paths(home_root)` est de l'arithmétique
  de chemin, à l'image de `sandbox_image::default_dockerfile_path(sandbox_root)`. `library_store.rs:49`
  lit `$HOME` globalement, et le coût de ce régime est chiffré : un `HOME_TEST_LOCK`
  (`library_store.rs:967`) pris par 34 tests de `lib.rs` avec 36 `allow(clippy::await_holding_lock)`.
  #408 a payé une slice pour en sortir `run_cost`. Corollaire : le seam #408 déplace la racine des
  **transcripts**, pas celle des **prix** — les prix sont un concept d'**instance**, l'hôte les porte
  même pour un Run sandboxé.

- **Le fetch est testable sans réseau.** L'URL est un seam par variable d'environnement
  (`PDO_PRICE_SOURCE_URL`), lu une fois au boot et porté dans `DaemonConfig` — l'idiome de
  `PDO_TMUX_CMD_OVERRIDE` (#181) et `PDO_DOCKER_CMD_OVERRIDE` (#407), dont le commentaire est la charte
  (`lib.rs:1531`) : « so no test needs a real daemon or a global `std::env::set_var` race ». Le
  rafraîchissement de boot est un second champ de `DaemonConfig`, désarmable par
  **`PDO_PRICE_SYNC=off|0|""`** — le seul env d'**opt-out** du crate, et assumé comme tel : une feature
  qui doit marcher d'emblée ne peut pas être armée par variable d'environnement, et l'opt-in réel est
  le premier clic. Les trois harnais qui lancent le **vrai binaire** le posent
  (`tests/log_level_default.rs`, `frontend/playwright.config.ts`, `tests/smoke.sh`).
  C'est le **seul** point qui sépare
  production et tests : `from_env()` (`lib.rs:1552`) n'est appelé que par `serve` (`:1586`), tandis que
  les 240 `TestDaemon` construisent `DaemonConfig` par littéral exhaustif — ajouter un champ **casse la
  compilation** des cinq constructeurs de `tests/common/mod.rs`, ce qui force une décision explicite
  au lieu d'une convention oubliable. Gater sur `nested_daemon` ne protégerait rien :
  `tests/common/mod.rs:58` retire `PDO_NODE_ID` exprès.

- **Aucune dépendance nouvelle.** `reqwest` est déjà une dépendance du daemon avec `rustls-tls`,
  `json`, `http2` et `system-proxy` (`crates/pdo-daemon/Cargo.toml:45`), et `webpki-roots` est dans le
  lock : les trust anchors sont compilés. Contrainte à préserver — la rationale de rustls
  (`Cargo.toml:42-44`) est que `openssl-sys` ne cross-compile pas vers `aarch64-unknown-linux-gnu` ;
  une dépendance qui le ramènerait casserait la release sur cette cible **seulement**, invisible de la
  CI. Le client doit être **async** : `reqwest::blocking` panique depuis le contexte du runtime, y
  compris dans un `spawn_blocking` (les threads du pool bloquant portent ce contexte), ce que
  `main.rs:18-20` documente déjà pour les chemins CLI.

## Conséquences

- **Positif.** Un nouveau modèle se tarife **sans intervention** dès qu'un sync a tourné, et la fenêtre
  d'ops passe d'un cycle de release à un clic. Le chemin de lecture ne change pas de nature : il reste
  deux `fs::read` et un `const`, donc `GET /runs/:id` et `GET /stats/cost` répondent à l'identique,
  hors ligne comme en ligne. Une remise entreprise, ou un modèle qu'aucune source ne publie
  (`claude-mythos-5`), reste réparable par le tier manuel — et cette correction **survit** au sync, ce
  qui est visible dans le compte-rendu.

- **Négatif / assumé.** Le daemon a désormais un egress **de plus**, et une source tierce devient une
  dépendance de **correctitude des chiffres affichés**. La doctrine applicable est celle d'ADR-0013:26
  — « la version fait partie de la frontière de sécurité, pas un détail de dépendance » — sauf qu'ici ce
  sont les **valeurs** qui sont load-bearing, non la version. D'où le `fetched_at` visible : le
  millésime de la table est lisible, pas deviné. models.dev est communautaire (PR + CI de schéma) et a
  changé d'organisation GitHub (`sst` → `anomalyco`) sans promesse de versioning du schéma ; c'est le
  prix de la propreté de ses clés, et la raison pour laquelle le repli LiteLLM n'est pas décoratif.
  Le coût étant dérivé à la lecture, un sync **retarife tous les Runs historiques, archivés inclus** :
  cela satisfait la contrainte du CHANGELOG (« un Run archivé s'ouvre et se chiffre »), mais le chiffre
  d'un Run clos devient fonction d'un fichier modifiable — à assumer au titre de l'« étiquetage
  honnête (load-bearing) » d'ADR-0022:66-70.
  `COST_MEMO_CAP` (`run_cost.rs:293`) porte maintenant plusieurs entrées par Run à travers un
  changement de table ; l'overflow vide toute la map, ce qui reste correctness-preserving par
  construction.

- **Limites connues, à ne pas confondre avec des bugs.**
  **Le mode fast est invisible** : `claude-opus-5` et `claude-opus-5-fast` s'écrivent du **même id**
  dans `message.model`, donc un nœud en mode fast est sous-facturé ×2 et **aucune** normalisation ne
  peut le rattraper. models.dev expose la grille dans `experimental.modes.fast` (10/50), inutilisable
  faute de signal côté transcript.
  **Le prix d'intro de `claude-sonnet-5`** (2/10) expire le 2026-08-31 → 3/15, et **aucune** des trois
  sources ne porte de date d'effet. Sans dimension de date — délibérément absente, voir hors-scope — la
  remontée surestimera de 50 % les 607 lignes sonnet-5 antérieures, soit **0,5 %** d'un nombre déjà
  préfixé `~`.
  **Les prix de cache restent dérivés** (1.25× / 2× / 0.1× de l'input). models.dev les **confirme**
  ligne par ligne et n'a **pas** de split 5m/1h : le bucket 1 heure n'est dérivable d'aucun de ses
  champs, et la doc Anthropic confirme le facteur 2.

## Alternatives rejetées

- **OpenRouter comme source.** 9 collisions à prix divergents après normalisation, un `canonical_slug`
  à l'ordre des mots instable, un SKU fantôme à $30/$150, et des CGU qui interdisent la copie des
  informations du service. Suggéré par le propriétaire, mesuré, écarté — avec les preuves ci-dessus.
- **LiteLLM comme source primaire.** Zéro collision aussi, MIT aussi, et l'argument propre que
  `ccusage` lit exactement ce fichier (donc PDO se réconcilierait avec l'outil de recoupement de
  l'équipe). Écarté en v1 pour une seule raison : ses clés ne sont pas le vocabulaire de PDO (alias à
  ordre inversé — `claude-4-opus-20250514` **et** `claude-opus-4-20250514`), et 273 clés `claude-*`
  d'autres providers doivent être filtrées. **Reste le repli documenté** si models.dev disparaît : un
  adaptateur, pas une refonte.
- **Fetcher au build** (ce que rejetait déjà ADR-0022, et qui reste rejeté). La distinction est
  load-bearing et sera confondue : fetcher **au build** fige la table dans le binaire et **ramène** le
  couplage à la release qu'on supprime ; fetcher **out-of-band vers un cache disque** ne le fait pas.
- **Fetcher sur le chemin de lecture** (paresseusement, à la première lecture de coût). Interdit :
  ADR-0030:119-121 pose déjà la règle en refusant un aller-retour réseau dans un handler `PUT`. Un
  `GET /runs/:id` qui dépend d'Internet est exactement ce que le motif d'ADR-0022 protégeait, sous sa
  forme forte.
- **Un fichier unique réécrit par le sync**, ou **un fichier à deux sections**. Voir la première puce
  de décision : la première efface les corrections manuelles, la seconde exige une réécriture partielle
  sûre d'un fichier humain, dont ce codebase n'a aucun précédent.
- **Remplacement global au lieu d'une fusion par clé.** Chiffré : oublier `claude-opus-4-8` effacerait
  79 941 lignes sur ~116 200, et gèlerait la table contre les releases futures.
- **Inférer le prix du nom du modèle.** Faux, pas seulement fragile : `claude-opus-4-1` et
  `claude-opus-4-0` sont à $15/$75, `claude-opus-4-5` à `4-8` à $5/$25 — même famille, 3× l'écart. Le
  commentaire de `run_cost.rs:46-47` l'interdit déjà noir sur blanc (« never a `starts_with("opus-4")`
  shortcut »). Il n'y a **rien à calculer**, seulement quelque chose à *savoir*.
- **Embarquer / shell-out `ccusage`.** Toujours rejeté, mais le motif change : « réseau » n'est plus
  discriminant. Ce qui subsiste est la dépendance **binaire + Node**, et le fait que ccusage imposerait
  *sa* table plutôt que la nôtre.
- **Vider `COST_MEMO` au sync** au lieu de mettre l'empreinte dans sa clé. Redondant sous la nouvelle
  clé, et strictement moins bon : cela invaliderait aussi les Runs dont les prix n'ont pas bougé.
- **`etag` / `If-None-Match`.** ADR-0015 amendement #471 interdit les champs morts. Le payload fait
  3,3 Mo mais le fetch est manuel ou une fois par 24 heures : conditionner n'achète rien en v1.

## Hors-scope (suivis à filer)

- **Nommer le modèle non tarifé** dans l'UI (`unpriced_models`, rendre `—` au lieu de `~$0.00`) :
  **#425**, AC #4. Sans elle, l'utilisateur reste incapable d'apprendre **quel** modèle manque quand
  aucun tier ne le connaît — et c'est ainsi que `claude-fable-5` est resté invisible.
- **Un `GET /prices` exposant le tier gagnant par modèle.** ~~C'est de la découvrabilité de modèles, donc
  le territoire de #425. `manual_keys` et `fetched_rows` sur `GET /settings` suffisent à rendre visible
  qu'un tier masque un autre.~~ **Réalisé en #528**, et l'argument de suffisance était faux : juxtaposer
  `manual_keys` et `fetched_rows` ne **rend** pas le tier **gagnant** — c'est un calcul de précédence par
  clé de famille, pas une lecture. #425 a livré `unpriced_models` sans absorber cette vue. #528 l'expose
  comme un tableau **`resolved`** (une entrée par **famille** : tier gagnant + **`$/MTok`**) **ajouté au
  bloc `price_table` de `GET /settings`**, **pas** une route `GET /prices` (champ additif rétro-compatible,
  cf. CONTEXT.md *Versioning*). Purement additif, lecture seule, dans le cadre de cet ADR — **pas de
  nouvel ADR**.
- **Un adaptateur LiteLLM** — le repli nommé ci-dessus.
- **Le mode fast** et le **palier long-contexte > 200K** : ce sont des **paliers**, pas des prix de
  famille ; ni un multiplicateur ni une ligne de plus ne les exprime, et le premier n'a de toute façon
  aucun signal côté transcript.
- **Une dimension de date par ligne.** La bonne horloge n'est pas `now()` : `price_for` et `line_cost`
  ne prennent pas le temps, `parse_line` ne lit jamais le `timestamp`. Gater sur `now()` **retariferait
  l'histoire** — le coût d'un Run terminé changerait sans que ses transcripts changent — et serait
  invisible de la clé du memo. Purement additif plus tard.
- **Transcrire dans `docs/agents/run-scenario.md` la recette de pile isolée et de teardown** que le
  Feature Path de #427 a dû porter lui-même : ce playbook ne dit nulle part comment démarrer ni démonter
  une pile, ce qui est la cause racine documentée de #422, et il omet le socket tmux dérivé du port.
