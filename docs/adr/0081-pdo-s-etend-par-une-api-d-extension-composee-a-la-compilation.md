# PDO s'étend par une API d'extension composée à la compilation, avec des hooks génériques

> Statut : accepted (grilling du 2026-09-26, story « architecture extensible »). Vocabulaire :
> CONTEXT.md § *Extensions*. S'appuie sur ADR-0079 (les mutations passent par des use cases).
> Amendé par #21 : comment une distribution obtient le frontend embarqué (§ *Amendements*).
> Amendé par #29 : les Emplacements d'UI deviennent un catalogue nommé, avec Commandes et
> clauses `when` (§ *Amendement #29*).
> Amendé par #31 : le catalogue gagne les emplacements d'Onglet, de Section, de Décoration et de
> Filtre de liste, et une Section participe au Save et au Cancel de son hôte (§ *Amendement #31*).

## Contexte

On veut pouvoir ajouter des capacités à PDO sans toucher le core, sauf son API d'extension, et
les livrer à leur propre rythme. Le core doit rester lisible comme « PDO est extensible » : il ne
nomme aucune extension en particulier.

## Décision

- **Des hooks génériques, sur le modèle des webhooks d'admission et d'authentification de
  Kubernetes.** Une Extension peut :
  - identifier l'**Acteur** d'une requête (`identify`) ;
  - **autoriser ou refuser une Opération** avant son exécution (`authorize`) ;
  - poser des **Attributs d'entité** à la création (`on_create`) ;
  - observer les événements (`on_event`) ;
  - contribuer des routes et des **Emplacements d'UI**.

  Chaque hook a un défaut neutre. Sans extension, l'appelant est l'Opérateur local, tout est
  autorisé, et PDO se comporte exactement comme avant.
- **Une Opération est générique** : ressource × verbe (créer, modifier, supprimer, action
  nommée), et elle est non exhaustive. **Toutes** les mutations en ont une. Ajouter une ressource
  ou une action est donc une évolution mineure de l'API, et le core ne trahit aucun besoin précis.
- **L'autorisation s'applique aux adaptateurs primaires** (HTTP, CLI, agents, Triggers), **jamais
  aux transitions internes du scheduler**. Une extension ne peut pas figer un Run en plein vol :
  le runtime ne déclare jamais forfait (ADR-0049). Si une extension panique dans `authorize`, la
  réponse est un refus.
- **`on_event` reçoit le JSON déjà diffusé sur `/ws`**, qui est un contrat gelé, plus
  `OperationApplied` après chaque mutation autorisée et réussie. L'enum interne n'est jamais
  exposé. `on_event` est asynchrone et isolé : une extension lente ou en panne ne ralentit jamais
  le core.
- **Les attributs sont stockés par le core sans être interprétés**, préfixés par l'id de
  l'extension et hors de l'event log (ce sont des métadonnées ajoutées, pas des faits du Run). On
  peut filtrer toute liste dessus.
- **Chaque extension a son propre espace** : ses routes sous `/ext/<id>/`, son dossier de données
  (sa base, ses migrations), et ses bundles d'UI chargés à l'exécution. React est partagé par
  import map et vérifié par majeure : en cas de décalage, le bundle n'est pas chargé et une
  bannière le signale.
- **Composition à la compilation.** Un binaire tiers dépend des crates du core par tag git et son
  `main` appelle `pdo::main_with(extensions)` : tout le binaire `pdo`, CLI comprise (amendement
  #23 ci-dessous). `serve(config, extensions)` démarre le daemon seul. **Seuls le crate d'API,
  `main_with` et `serve` sont couverts par le semver** : l'API démarre en **0.x**, et le reste du
  core ne promet rien. Axum est réexporté par l'API et fait donc partie du contrat.
- **Une extension de test interne** vit dans la suite de tests du core. Elle n'est ni publiée ni
  documentée, et elle fait jouer chaque hook. C'est le seul garde-fou qui fait échouer le core
  quand il casse son API.

## Alternatives écartées

- **Une variante d'Opération par cas d'usage.** Chaque nouveau besoin deviendrait un changement
  d'API, et la liste des variantes décrirait les extensions attendues.
- **Exposer l'enum d'événements interne.** Chaque refactor du runtime deviendrait une rupture
  d'API.
- **Des extensions chargées à l'exécution (WASM, wasmtime + WIT).** Reporté tant qu'aucun tiers
  n'a besoin de charger une extension dans le binaire standard. Le trait se porte en WIT sans
  refonte.
- **Une abstraction requête/réponse maison au lieu d'axum.** Plus de code à maintenir, moins
  expressive. Une montée de majeure d'axum est rare, et elle se paie en montée de majeure de l'API.
- **Un kit de conformité publié.** L'extensibilité est une capacité, pas une vitrine :
  l'extension de test interne suffit côté core.

## Limite acceptée

Quand une extension a besoin de ce que l'API n'expose pas (une vue sans emplacement, par exemple),
il faut modifier le core. Chaque ajout reste générique et utilisable par toute extension. Les
seams se découvrent en écrivant des extensions.

## Amendement #23 : le point d'entrée `main_with(extensions)`

Premier seam découvert ainsi, en écrivant la première distribution (`Loulen/pdo-extensions`).
Le binaire `pdo` n'est pas que le daemon : les agents appellent `pdo complete` et `pdo wait-user`
depuis leur session, et `pdo service install` enregistre l'exécutable courant comme daemon. Avec
`serve` seul, une distribution devait recopier le dispatch de la CLI, donc le contrat de codes de
sortie (`complete` 0/3/4/1, `wait-user` 3, `run wait` 2), la règle « runtime tokio pour le seul
daemon » et l'init du tracing, en dépendant de crates hors semver.

- **`pdo::main_with(extensions)` est tout le binaire** : parse de la ligne de commande, dispatch,
  code de sortie. Le binaire vanilla est `pdo::main_with(Vec::new)`, si bien que le dispatch
  n'existe qu'en un exemplaire.
- **Une fabrique, pas une liste** (`impl FnOnce() -> Vec<Box<dyn Extension>>`) : appelée au plus
  une fois, et seulement par `daemon` et `service`. Un `pdo complete` d'agent ne construit jamais
  les extensions ni leurs effets de bord.
- **Les extensions du binaire règlent aussi `pdo service`** : `install` et `uninstall` sont des
  Opérations soumises à leur Autorisation d'opération, comme celles du daemon. C'est une règle du
  core, plus un choix de chaque binaire. Un `--dry-run` ne change rien et ne demande donc rien.
- **`main_with_config(extensions, configure)`** applique `configure` à la `Config` du daemon
  avant `serve` : une distribution y déclare ce que `serve` prendrait, dont son canal de mise à
  jour (#24). Appelée au plus une fois, par `pdo daemon` seul.
- Ajout rétrocompatible : bump mineur du core (2.1.0), `pdo-extension-api` inchangé. Les
  sous-commandes une à une et `pdo::cli` restent hors contrat.

## Amendements (#21)

**Une distribution fournit le frontend déjà construit.** `pdo-daemon` embarque l'UI à la
compilation (rust-embed). Le checkout que fait cargo pour une dépendance par tag n'a pas de
`node_modules`, et un build script ne doit écrire que dans `OUT_DIR`. `crates/pdo/build.rs` place
donc toujours le frontend dans `$OUT_DIR/frontend-dist` et le prend, dans cet ordre :

1. `PDO_FRONTEND_DIST=<chemin absolu>` : un `dist/` déjà construit (avec `index.html`), copié
   tel quel. Ni Node ni pnpm. C'est la voie d'une distribution.
2. `PDO_SKIP_FRONTEND_BUILD` : rien n'est construit, le contenu déjà présent dans `OUT_DIR` est
   embarqué (vide sur un `target/` neuf). Pour les boucles qui ne compilent que le Rust.
3. Sinon `pnpm run build` dans `frontend/`, Vite écrivant dans `OUT_DIR`, **si
   `frontend/node_modules` existe**. Sinon le build échoue avec un message qui nomme les deux
   remèdes (`PDO_FRONTEND_DIST`, ou Node.js 22+ et pnpm puis `pnpm install --frozen-lockfile`).
   Le build ne compte plus sur l'auto-install de pnpm (`verify-deps-before-run`), qui dépend de
   la version et de la config de pnpm et remplissait le checkout du cache cargo.

Aujourd'hui la distribution construit ce `dist/` une fois, depuis un clone du core au même tag
(`pnpm install --frozen-lockfile && pnpm run build` dans `frontend/`), puis pointe
`PDO_FRONTEND_DIST` dessus. Procédure : README, « Building a binary that embeds PDO ». Le seul
reste dans les sources est le cache incrémental de `tsc -b` (`*.tsbuildinfo`), et seulement dans
la voie 3, à l'intérieur du `node_modules` installé à la main.

Écartés : faire lancer `pnpm install` par build.rs (réseau pendant `cargo build`, ~800 paquets
hors de portée de `cargo clean`, builds parallèles qui se marchent dessus) et faire télécharger un
asset par build.rs (casse les builds hors ligne ou vendorisés). Reste à faire : publier le `dist`
en asset de release (`pdo-frontend-dist-<tag>.tar.gz`), téléchargé par la distribution et non par
build.rs, et vérifier qu'un `dist` fourni correspond au tag.

## Amendement #29 : un catalogue d'emplacements nommés, des Commandes, des clauses `when`

Seam découvert en préparant l'extension Projet (`pm`, story `Loulen/pdo-extensions#3`) : quatre
emplacements figés (navigation, panneau de réglages, filtre de liste, onglet de détail) ne
suffisent pas, et chaque nouveau besoin aurait ajouté une variante à `Slot`. On suit le modèle
des *contribution points* de VS Code : peu de types de contribution, beaucoup d'emplacements.

- **Un catalogue d'emplacements nommés** (`UiLocation`, non exhaustif) : chaque emplacement a un
  identifiant stable (`topbar/start`, `topbar/end`, `settings`, `runs/filter`, `run/detail`),
  accepte certains types de contribution et transmet un contexte typé. Les identifiants, ce que
  chaque emplacement accepte et le contexte qu'il transmet font partie du semver de l'API. Le
  type est exporté en TypeScript (feature `ts` du crate d'API, activée par le daemon) et
  documenté dans `docs/reference/extension-ui.md` ; un test échoue quand un emplacement n'y a pas
  sa section.
- **Six types de contribution** (`UiElement`, non exhaustif) : Page, Action, Onglet, Section,
  Décoration, Filtre de liste. Une contribution (`UiContribution`) porte son emplacement, un id
  unique dans l'extension, un libellé, et une clause `when` optionnelle.
- **Un emplacement est une liste** : les contributions de toutes les extensions s'y ajoutent,
  dans l'ordre d'enregistrement des extensions puis dans l'ordre de déclaration. Aucune n'est
  exclusive.
- **Contexte** : chaque composant reçoit `extension` et `context`, qui porte au minimum le Run
  sélectionné et son Projet. Le Projet est déduit par le front du core, avec la règle de la liste
  des Runs : le Projet dont les membres contiennent le dépôt de la racine du Run, comparé tel quel
  (ADR-0033). Chaque emplacement ajoute ce qui est en jeu chez lui (le chemin d'une Page, l'entité
  d'un Onglet, le filtre d'un Filtre de liste).
- **Commandes** (`UiCommand` : id, titre, icône, nom de l'export) : le bundle exporte une fonction,
  une Action la référence. Le core rend l'entrée, puis appelle `fonction(context, host)` avec le
  contexte de l'emplacement de l'Action ; `host.notify` affiche un toast, `host.open` suit un lien
  profond. Une exception ou une promesse rejetée donne un toast d'erreur. Une Action dont la
  Commande n'est pas déclarée est écartée par le daemon (avec un avertissement dans son journal),
  une Action dont le bundle n'exporte pas la fonction n'est pas rendue.
- **Clause `when`** : une expression structurée (`has`, `eq`, `not`, `all`, `any`) sur des clés
  pointées du contexte (`run.status`, `project.name`), évaluée par le front du core. Fausse, la
  contribution n'est pas rendue. Un opérateur inconnu du front est faux.
- **`topbar/end`** : une icône par Page ou Action dans le groupe de droite de la barre du haut,
  avant Stats. L'icône est un SVG servi par l'extension, dessiné en masque dans la couleur du
  texte ; sans icône, la première lettre du libellé, encadrée. Une Page `topbar/end` s'ouvre dans
  `FullWindowShell` sans rail (le rail y devient optionnel) : l'en-tête du shell (icône, titre,
  fil d'Ariane et actions fournis par la Page, ✕, Échap), la navigation étant celle de la Page.
- **Lien profond** : `#/ext/<id>/<emplacement>/<chemin>` ouvre la première Page de l'extension à
  cet emplacement et lui transmet le chemin ; la barre d'adresse suit la Page ouverte (Précédent
  la ferme). Ces liens sont cliquables dans tous les rendus markdown du core (artefacts,
  commentaires de Review, `SKILL.md`, notes de version), écrits en lien ou en texte brut. Un lien
  vers une extension absente, non chargée ou sans Page à cet emplacement affiche un toast
  d'erreur, jamais une page blanche.
- **Rétrocompatibilité** : `Slot` et `UiSlot` restent acceptés ; le daemon convertit chaque slot
  0.1 vers son emplacement (`UiSlot::contribution`) et le front ne connaît plus que le
  catalogue. Les composants 0.1 reçoivent toujours leurs props (`entity`, `filter`,
  `setFilter`, `resource`). Le fil de `GET /ext` change : `ui.slots` devient `ui.contributions` et
  `ui.commands` ; le front et le daemon sont livrés ensemble.
- **Version** : `pdo-extension-api` passe en **0.2.0**. En 0.x, une mineure peut casser ; l'API
  déclare désormais la plus ancienne version qu'elle honore
  (`OLDEST_COMPATIBLE_API_VERSION = "0.1.0"`), et le daemon accepte une extension qui exige une
  version comprise entre celle-ci et la sienne. Une extension écrite pour 0.1 continue donc de
  compiler et de tourner.

Écartés : une clause `when` en chaîne à parser (à la VS Code), parce qu'une structure se
sérialise, se type en TypeScript et ne demande pas de grammaire ; un emplacement réservé à une
extension, contraire à la règle de la liste ; un contexte calculé par le daemon, qui ne connaît
pas la sélection de l'opérateur.

## Amendement #31 : onglets, sections, décorations et filtres ; une Section participe au Save

Suite de #29 pour l'extension Projet (`pm`) et ADR-0001 de `Loulen/pdo-extensions` (« mieux vaut
trop de points d'extension que pas assez ») : le catalogue s'ouvre à tous les écrans où une
extension a quelque chose à montrer. On étend le catalogue, sans nouveau type de contribution.

- **Onglets** : `node/detail` (le volet d'un node de Run, après I/O et Orchestration),
  `trigger/detail` (après « Details »), `skill/detail` (après « SKILL.md » et « Files »), en plus
  de `run/detail`. Le contexte porte l'entité (`kind`, `id`, `attributes`, `name`). Sans
  contribution, aucune barre d'onglets n'apparaît là où le core n'en a pas.
- **Sections** : `project/edit` (l'éditeur de Projet, sous les champs du core, à la création
  comme à l'édition), `run/new` (le formulaire de nouveau Run ou de Trigger), `node/config` et
  `pipeline/config` (les inspecteurs du canvas), `run/info` (l'onglet Info d'un Run), en plus de
  `settings`.
- **Une Section participe au Save et au Cancel de son hôte** (`SectionForm`, prop `form`, présent
  là où l'hôte a un Save : `project/edit`, `run/new`). La Section garde ses changements en
  brouillon et les applique dans son `onSave` : rien ne s'applique avant le Save de l'hôte. L'hôte
  enregistre d'abord ses propres champs, puis appelle les `onSave` dans l'ordre des Sections à
  l'écran, chacun attendu, avec l'entité enregistrée : son id est connu à ce moment, ce qu'exige un
  Projet créé par l'éditeur, qui n'a pas d'id avant le Save. Un échec garde le formulaire ouvert
  avec son message (ce que le core a enregistré le reste) ; pour `run/new`, où le Run ou le
  Trigger existe déjà, c'est un toast d'erreur. `onCancel` est appelé à l'annulation.
- **Décorations** (badge ou pastille) : `run/item` (colonne de droite de la ligne, ligne 2, celle
  des compteurs d'enfants, pour ne pas tronquer le nom de la pipeline), `trigger/item`,
  `pipeline/item` (fin de la ligne 2), `canvas/node` (une ligne sous le titre de la carte),
  `port/item` (à côté du nom du port), `review/file` (avant les compteurs de commentaires),
  `statusbar`. Le contexte porte l'entité et ses Attributs, que voit la clause `when`. **Sans
  contribution, rien n'est rendu** : la ligne garde sa mise en page ; à plusieurs, les marques
  s'alignent côte à côte et passent à la ligne plutôt que de pousser le texte.
- **Filtres de liste** : `triggers/filter`, `pipelines/filter`, `projects/filter`, en plus de
  `runs/filter`. La liste garde ce qui porte chaque Attribut du filtre, comme `?attr.` sur
  l'endpoint. `projects/filter` se rend au-dessus des listes groupées par Projet (Runs et
  Triggers) et garde les éléments dont le Projet porte les Attributs, par la règle du
  regroupement.
- **Frontière d'erreur** : chaque contribution reste derrière la sienne ; une Décoration qui
  plante devient un petit « ! » dont l'infobulle nomme l'extension, sans toucher sa ligne.
- **Rétrocompatibilité** : les slots 0.1 `ListFilter` des Triggers, des pipelines et des Projets
  et `DetailTab` d'un node, d'un Trigger et d'un skill sont convertis vers ces emplacements ;
  `DetailTab { Project }` devient une Section de `project/edit` (un Projet n'a pas de panneau de
  détail) et son composant reçoit le Projet en `entity`. Une extension 0.1 ou 0.2 compile
  toujours.
- **Version** : `pdo-extension-api` passe en **0.3.0** (ajout : des variantes de `UiLocation`,
  non exhaustif).

Au passage, la barre d'onglets du panneau d'infos d'un Run défile horizontalement au lieu de
déborder : les Onglets des extensions s'ajoutent après ceux du core.

Écartés : un type de contribution par emplacement (une « Section de formulaire »), parce que le
contrat du Save est le même partout où l'hôte en a un ; un Save des Sections avant celui du core,
impossible pour un Projet créé, qui n'a pas encore d'id.
