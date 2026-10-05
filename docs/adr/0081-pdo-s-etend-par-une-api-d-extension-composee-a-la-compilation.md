# PDO s'étend par une API d'extension composée à la compilation, avec des hooks génériques

> Statut : accepted (grilling du 2026-09-26, story « architecture extensible »). Vocabulaire :
> CONTEXT.md § *Extensions*. S'appuie sur ADR-0079 (les mutations passent par des use cases).
> Amendé par #21 : comment une distribution obtient le frontend embarqué (§ *Amendements*).
> Amendé par #29 : les Emplacements d'UI deviennent un catalogue nommé, avec Commandes et
> clauses `when` (§ *Amendement #29*).
> Amendé par #34 : la ressource d'extension et l'Hôte (§ *Amendement #34*).
> Amendé par #32 : une extension héberge ses propres emplacements, rendus par un module partagé
> (§ *Amendement #32*).
> Amendé par #33 : la CLI d'une extension (`pdo ext`) et ses skills (§ *Amendement #33*).
> Amendé par #31 : le catalogue gagne les emplacements d'Onglet, de Section, de Décoration et de
> Filtre de liste, et une Section participe au Save et au Cancel de son hôte (§ *Amendement #31*).
> Amendé par CORE-3 : une Opération porte ses Projets, le Manifest déclare les Ressources d'une
> extension, le core expose le catalogue des Ressources (§ *Amendement CORE-3*).

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

## Amendement #34 : la ressource d'extension et l'Hôte

Seam découvert en préparant l'extension `pm` (`Loulen/pdo-extensions#4`), qui a ses propres
entités (Backlogs, Tâches, Pages…) et dont chaque mutation doit être refusable, observable et
annotable par une autre extension, sans que l'une nomme l'autre (ADR-0001 de `pdo-extensions`).
Les hooks ci-dessus ne couvrent que les mutations **du core**.

- **Une ressource générique `ResourceKind::Extension { extension, kind }`**, avec les verbes
  existants. Sur le fil, une ressource s'écrit par son nom : `run`, `agent_profile`… pour celles
  du core, `<extension>/<kind>` pour celles d'une extension (`pm/tache`). Une extension de droits
  refuse `pm/tache` × `update` sans rien connaître de `pm`.
- **Un Hôte** (`Host`), remis dans `ExtensionContext::host` à `start`, un par extension. C'est
  par lui qu'une extension fait passer **ses** mutations par les mécanismes du core :
  - `authorize(acteur, opération)` consulte l'Autorisation d'opération de **toutes** les
    extensions, **celle qui émet comprise** ; le premier refus gagne avec sa raison, une panique
    est un refus. L'extension répond alors le `403` `operation_refused` du core (`Denied`,
    désormais dans l'API) et ne change rien.
  - `publish(événement)` diffuse l'événement sur `/ws` et vers `on_event` des **autres**
    extensions, dans une enveloppe du core : `{"type": "extension_event", "extension": <id>,
    "name": <nom>, "payload": <charge>}`. L'enveloppe est gelée comme le reste de `/ws` ; la
    charge appartient à l'extension.
  - `applied(OperationApplied)` : l'extension dit au core qu'une mutation autorisée a réussi, et
    l'`OperationApplied` suit, comme pour une mutation du core. Seule l'extension sait si sa
    mutation a réussi : le core ne le devine pas depuis la réponse HTTP, ce qui couvre aussi ce
    qui n'est pas une requête (une session WebSocket, un thread).
  - `on_create(acteur, entité)` appelle `on_create` des **autres** extensions et rend leurs
    Attributs préfixés par leur id. **L'extension les stocke** et les renvoie avec ses entités :
    le core ne garde pas les Attributs des entités qu'il ne connaît pas.
- **`OperationApplied` passe sur `/ws`** dès qu'une extension est chargée, pour les mutations du
  core comme pour celles des extensions : un seul chemin (le bus), donc le même ordre sur `/ws` et
  dans `on_event`. Sans extension, `/ws` porte exactement ce qu'il portait. Le front ignore ces
  messages (liste blanche de `useDaemonSocket`). Contrepartie : comme tout événement du bus, un
  `OperationApplied` est perdu (et journalisé) pour un abonné qui prend trop de retard sur le bus,
  alors qu'il allait avant directement dans la file de chaque extension.
- L'Hôte ne retient les extensions que faiblement : une fois le daemon arrêté, il refuse tout et
  ne publie rien. `publish` et `applied` n'attendent jamais une extension (même livraison
  isolée que pour les événements du core) ; `authorize` et `on_create` appellent les hooks des
  autres extensions sur le fil de l'appelant, comme le core pour ses propres mutations : ils sont
  aussi rapides que ces hooks, qui doivent l'être. `NeutralHost` (tout autorisé, rien publié,
  aucun Attribut) est celui d'un `ExtensionContext::new`, et le remplaçant d'un Hôte dans les
  tests d'une extension.
- **Montée de mineure de l'API : 0.2 -> 0.3** (une mineure est cassante en 0.x ; #34 suit #29,
  qui avait pris la 0.2) : `ResourceKind`
  n'est plus `Copy` (la variante porte des `String`), `ExtensionContext` porte un champ de plus
  (`host`) et implémente `Debug`, `PartialEq` et `Eq` à la main (deux contextes sont égaux s'ils
  nomment le même dossier et partagent le même Hôte).
  Une extension 0.1 ou 0.2 qui ne comptait pas sur `Copy` (dont `pdo-ext-hello`) compile telle
  quelle et reste acceptée : la casse est de compilation, pas de fil, donc
  `OLDEST_COMPATIBLE_API_VERSION` reste `0.1.0` (amendement #29).
- L'extension de test interne y gagne trois pairs (`a`, `b`, `c`, `interop.rs`) qui créent et
  suppriment leurs `item` par l'Hôte ; le daemon de démo les charge avec `PDO_TEST_INTEROP=1`.

Écartés : **faire enregistrer l'`OperationApplied` par le scope de la requête** (comme pour le
core) : il ne couvre ni la session WebSocket de `pm` ni un écrit hors requête, et l'aurait publié
en double avec un `applied` explicite. **Stocker les Attributs d'une entité d'extension dans le
core** : le core devrait alors filtrer des listes qu'il ne sert pas ; l'extension les stocke comme
le reste de ses entités. **Livrer ses propres événements à l'émettrice** : elle sait ce qu'elle a
publié, et une extension qui réagit à ses propres événements bouclerait.

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

## Amendement #32 : des emplacements hébergés par une extension

Seam découvert avec l'extension Projet (`pm`) : une autre extension doit pouvoir ajouter un
onglet à une Page de `pm`, un badge à une Carte, une action dans la barre d'une Page, une section
dans sa barre latérale, sans que `pm` la connaisse (ADR-0001 de `pdo-extensions` : « mieux vaut
trop de points d'extension que pas assez »). Le catalogue du core n'y suffit pas : ces endroits
sont dans l'interface de `pm`, que le core ne rend pas.

- **Déclaration** : une extension déclare ses emplacements hébergés (`HostedLocation`) dans
  `UiContributions::host` : un identifiant `<id de l'extension>/<nom>` (`pm/onglet`), le **type de
  contribution** accepté (un seul, `UiElementKind`), une description et la **forme du contexte**
  (les clés qu'il ajoute au contexte commun, décrites). `GET /ext` les expose dans
  `ui.locations`. Le daemon écarte, avec un avertissement, une déclaration dont l'identifiant ne
  commence pas par l'id de l'extension, qui reprend un emplacement du core ou un emplacement déjà
  déclaré, ou qui accepte des Pages (une Page appartient au shell du core).
- **Contribution** : une autre extension y contribue comme au catalogue du core, en nommant
  l'emplacement (`UiLocation::Hosted("pm/onglet")`, sérialisé par son identifiant). La règle de la
  liste vaut : toutes les contributions s'ajoutent, dans l'ordre d'enregistrement puis de
  déclaration. Le daemon vérifie le type contre la déclaration de l'hôte. Une contribution à
  l'emplacement d'une extension absente (ou qui ne le déclare pas) est **ignorée sans erreur** :
  l'hôte n'est simplement pas installé.
- **Module partagé** `@pdo/extension-ui` : le front le déclare dans son import map à côté de
  React (`/shared/pdo-extension-ui.js`, qui relit ce que le front pose sur
  `globalThis.__PDO_SHARED__`). Un bundle l'importe en spécificateur nu, jamais embarqué. Il liste
  (`useHostedContributions(emplacement, contexte)`) et rend (`HostedContribution`,
  `HostedContributions`) les contributions à un emplacement hébergé, avec les mêmes frontières
  d'erreur et la même clause `when` que le core ; une Action y devient un bouton qui appelle sa
  Commande. Le contexte d'un élément est celui de l'hôte, fusionné par-dessus le contexte commun
  (Run sélectionné, Projet, `location`). Sans contribution, rien n'est rendu : la mise en page de
  l'hôte ne bouge pas.
- **Version** : le module est versionné avec l'API (sa version est `API_VERSION`) et vérifié par
  majeure comme React : un bundle déclare la majeure qu'il importe
  (`UiContributions::importing_ui_module`, `ui.ui_module_major`) ; une majeure différente de celle
  du front, et le bundle n'est pas chargé, avec une bannière. En 0.x, la vérification de
  `api_version` au démarrage refuse déjà une mineure incompatible. Un test du daemon échoue quand
  la version déclarée par le front (`uiModuleVersion.ts`) s'écarte de celle du crate.
- **Rétrocompatibilité** : `UiLocation` garde `Copy` et `id(self) -> &'static str` (l'identifiant
  d'un emplacement hébergé est un `&'static str`, une constante de l'API de l'hôte) ; tout le reste
  est ajouté. `pdo-extension-api` passe en **0.4.0** (après 0.3.0, amendement #34),
  `OLDEST_COMPATIBLE_API_VERSION` reste `0.1.0`. Le fil de `GET /ext` gagne `ui.locations` et `ui.ui_module_major`.

Écartés : un emplacement hébergé rendu par le core (il ne connaît pas l'interface de l'hôte) ;
une API d'enregistrement dynamique côté navigateur (les déclarations restent dans le manifeste
d'UI, lisibles par `GET /ext` et vérifiées par le daemon) ; un identifiant `String` qui aurait
retiré `Copy` à `UiLocation` et cassé le code 0.2 ; plusieurs types acceptés par emplacement
hébergé, sans besoin connu.

## Amendement #33 : la surface agent, `pdo ext <id>` et les skills d'extension

Seams découverts en spécifiant l'extension Projet (`Loulen/pdo-extensions#4`) : un agent doit
piloter une extension depuis sa session comme il pilote le core, et savoir le faire sans
consigne du prompt.

- **Deux hooks de plus, deux défauts neutres** : `cli()` déclare une ligne de commande (un
  résumé, `None` par défaut) et `run_cli(invocation)` l'exécute ; `skills()` liste des skills
  embarqués (un `SKILL.md` et ses fichiers, aucun par défaut). `pdo-extension-api` passe en
  **0.5.0** (après 0.4.0, amendement #32) : ajout rétrocompatible au source d'une extension (une
  extension 0.1 qui ne touche à rien compile et fonctionne), mais montée de mineure, donc rebuild
  obligatoire en 0.x ; `OLDEST_COMPATIBLE_API_VERSION` reste `0.1.0`.
- **`pdo ext <id> <args…>` relaie, sans construire les extensions.** La CLI envoie au daemon
  (`POST /ext-cli/<id>`, hors du préfixe `/ext/<id>` qui appartient à l'extension) les arguments
  tels quels, flags compris, l'entrée standard si c'est un pipe ou un fichier, son répertoire de
  travail, et l'identité de la session de node (`PDO_RUN_ID`, `PDO_NODE_ID`) comme les autres
  commandes d'agent. Le daemon appelle `run_cli` avec l'Acteur établi par `identify` : l'Agent
  `{run, node}` depuis une session de node, l'Opérateur local sinon. Les octets voyagent en
  base64. `pdo complete` et `pdo ext` ne construisent toujours pas les extensions : c'est le
  daemon qui les porte (amendement #23).
- **Codes de sortie** : celui de l'extension, tel quel ; `1` si le daemon est injoignable ou si
  l'extension échoue (panique : pas de verdict, comme `pdo complete`) ; **`127`**
  (`EXIT_NO_CLI`) si le daemon n'a pas d'extension `<id>` ou si elle n'a pas de CLI, l'usage
  du shell pour « commande introuvable ». `pdo ext` seul liste les extensions qui ont une CLI,
  lues dans `GET /ext` (champ `cli`).
- **`run_cli` tourne hors du chemin des requêtes** (`spawn_blocking`) : une commande peut durer
  (un backup) sans ralentir le daemon, et piloter un appel async par
  `Handle::current().block_on`. L'autorisation de ce qu'elle mute appartient à l'extension :
  le relais n'est pas une mutation du core.
- **Les skills d'une extension sont amorcés comme les skills natifs** (ADR-0062, #722) : à
  chaque démarrage, dans un dossier de la banque au nom de l'extension, sous l'id stable
  `ext-<extension>-<nom>`, verrouillés, marqués `extension: {id, version}` (l'UI affiche
  « Extension `<id>` » et la version). Ils sont réécrits quand la version de l'extension ou
  leur contenu change, retirés quand l'extension ne les fournit plus (liste vide comprise ;
  une panique de `skills()` ne retire rien), et livrés aux worktrees
  par les règles existantes dès qu'un tier les sélectionne. Un skill dont le `SKILL.md` ne passe
  pas le filtre, dont le nom diffère du nom déclaré, ou dont le libellé appartient déjà à un
  skill de l'opérateur est ignoré avec un avertissement, sans bloquer les autres. Les skills
  d'une extension retirée du binaire restent dans la banque (verrouillés) : ils reviennent à
  jour avec elle. `skills()` est appelé **avant** `start` (la banque est amorcée à
  l'ouverture de la base) : la liste ne dépend pas du dossier de données.

Écartés : un code dédié hors de la plage d'une extension (tout `u8` lui appartient ; `127` est
documenté et une extension ne doit pas le rendre) ; `POST /ext/<id>/cli` (masquerait une route
de l'extension) ; transporter stdin et stdout en texte (une Archive est binaire) ; lire
l'entrée standard dès qu'elle n'est pas un terminal (l'outil shell d'un agent laisse ouverte
une socket qui ne finit jamais : `pdo ext` attendait sans fin, relevé par le FP) ; un dossier
« PDO » commun avec les skills natifs (le dossier dit qui fournit le skill).

## Amendement #30 : les emplacements d'Action, et les menus du core sur le `DropdownMenu` partagé

Suite de l'amendement #29, préparée pour l'extension Projet (`pm`) et pour ADR-0001 de
`pdo-extensions` (« mieux vaut trop de points d'extension que pas assez ») : toutes les barres
d'outils et tous les menus du core accueillent des Actions.

- **Quatorze emplacements d'Action**, qui n'acceptent que des Actions
  (`UiLocation::is_action_location`) : `runs/header`, `run/item/menu`, `runs/bulk`, `run/toolbar`,
  `canvas/node/menu`, `canvas/edge/menu`, `node/toolbar`, `trigger/item/menu`,
  `pipeline/item/menu`, `pipelines/header`, `tab/menu`, `review/toolbar`, `review/file`,
  `skill/menu`. Chacun est documenté dans `docs/reference/extension-ui.md`, avec son contexte.
- **Contexte** : en plus du Run sélectionné et de son Projet, chaque emplacement transmet ce qui est
  en jeu chez lui. Un menu ou une barre d'élément transmet l'élément en `entity` (`kind`, `id`,
  `attributes`, et les champs du genre : `status` d'un Run, `enabled` d'un Trigger, `type` d'un
  node, `from`/`to` d'une edge, `path` d'un fichier). `runs/bulk` transmet la sélection
  (`selection: { kind: "run", ids }`). Les menus du canvas ajoutent le `pipeline` affiché (et le
  Run d'un canvas de Run), la Review la paire comparée (`review: { from, to }`). La barre du canvas
  d'un Run et la page Review donnent pour Run sélectionné le Run affiché. Une clause `when` lit ces
  clés comme les autres (`entity.status`).
- **Rendu** : dans une barre, un bouton icône par Action (l'icône de sa Commande, sinon la lettre
  encadrée), avec son libellé en infobulle, placé avant les boutons du core. Dans un menu, un
  groupe par extension, titré du nom de l'extension, après les entrées du core et **avant l'entrée
  destructive**. Dans la barre de sélection multiple, une entrée par Action après celles du core.
  Sans contribution, rien ne change : ni bouton, ni groupe, ni séparateur.
- **Échec** : chaque élément a sa frontière d'erreur ; une Commande qui lève ou rejette affiche un
  toast d'erreur, et la barre ou le menu continue de fonctionner.
- **Menu « ⋯ » des lignes** : les lignes de Run, de Trigger et de Pipeline gagnent un déclencheur
  `MoreHorizontal` après leurs icônes de survol (révélé avec elles, et gardé tant que le menu est
  ouvert), qui ouvre le `DropdownMenu` partagé : les verbes de la ligne, les groupes d'extension,
  puis l'entrée destructive. La ligne étant souvent un `<button>`, le déclencheur est un
  `span role="button"`, et ses clics et touches ne remontent pas à la ligne.
- **Migration** : les menus contextuels codés à la main (canvas, barre d'onglets, arbre des skills)
  passent sur le `DropdownMenu` partagé, sans changement de comportement : mêmes entrées, même
  ancrage (au pointeur pour un clic droit), même fermeture (Échap, clic ou clic droit ailleurs).
  Une entrée désactivée devient `aria-disabled` et reste focalisable au clavier (le modèle de menu
  de WAI-ARIA), au lieu d'un `disabled` natif ; elle ne fait toujours rien.
- **Version** : `pdo-extension-api` passe en **0.6.0** (après 0.5.0, amendement #33) : ajout
  d'emplacements, non cassant. Une extension 0.1 à 0.5 continue de compiler et de tourner.

Écartés : un type de contribution « entrée de menu » distinct de l'Action (une Action se rend déjà
selon l'endroit où elle se trouve) ; un menu « ⋯ » affiché seulement quand une extension y
contribue (la ligne gagnerait ou perdrait un déclencheur selon les extensions installées, et ses
verbes y sont utiles sans extension).

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
  `port/item` (à côté du nom du port), `review/file` (avant les compteurs de commentaires ;
  cet emplacement d'Action de l'amendement #30 accepte aussi des Décorations),
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
- **Version** : `pdo-extension-api` passe en **0.7.0** (après 0.6.0, amendement #30 ; ajout :
  des variantes de `UiLocation`, non exhaustif). Une extension 0.1 à 0.6 compile toujours.

Au passage, la barre d'onglets du panneau d'infos d'un Run défile horizontalement au lieu de
déborder : les Onglets des extensions s'ajoutent après ceux du core.

Écartés : un type de contribution par emplacement (une « Section de formulaire »), parce que le
contrat du Save est le même partout où l'hôte en a un ; un Save des Sections avant celui du core,
impossible pour un Projet créé, qui n'a pas encore d'id.

## Amendement CORE-3 : les Projets d'une Opération et le catalogue des Ressources

Seam découvert au grilling de l'extension `iam` (IAM-1 ; ADR-0005 de `pdo-extensions`, qui
amende son ADR-0001) : une extension de droits règle des Droits par Ressource et par Projet, sans
connaître ni les dépôts ni les autres extensions. Deux trous : une Opération ne dit pas dans quel
Projet elle a lieu, et rien ne liste les Ressources sur lesquelles régler des Droits.

- **Une Opération porte ses Projets** (`Operation::projects`, `Operation::in_projects`) : la liste
  des ids de Projet, vide pour une Opération **hors Projet**. Le core la remplit dans ses use
  cases, au moment de l'Autorisation d'opération :
  - `run` et `node` : le Projet qui possède le dépôt primaire du Run (`Runs::projects_of_run`) ;
    une création de Run : le dépôt qu'elle nomme (`target_repo`, sinon `target_repos[0]`), ou,
    créée depuis une session de node sans dépôt, le Run parent dont elle hérite le dépôt ;
  - `session` : le Run de la session (node, manager, shell de Run) ; l'assistant de bibliothèque
    est hors Projet ; `node` × `wait_user` / `resume` aussi par le Run ;
  - `review_comment` : le Run de la cible (`<run>` ou `<run>/<commentaire>`) ;
  - `project` ciblé : ce Projet ; `repository` × `fetch` / `fast_forward` : le Projet qui possède
    le dépôt ;
  - tout le reste (pipelines, Triggers, réglages, skills, profils, Stats, mise à jour) : hors
    Projet, sauf rattachement explicite à venir.

  Un dépôt appartient à au plus un Projet (ADR-0033), d'où une liste d'au plus un élément pour le
  core. Les membres d'un Projet se comparent tels qu'écrits ; pour qu'un autre chemin du même
  dossier (`/repo/`, `/repo/.`, un lien vers `/repo`) ne sorte pas un Run de son Projet, le dépôt
  d'un Run est essayé tel quel, puis normalisé, puis résolu sur le disque. Une extension la remplit pour ses Ressources (un Backlog de `pm` peut appartenir à
  plusieurs Projets). **Une extension ne recalcule jamais dépôt -> Projet** : elle lit
  `operation.projects`. Une lecture des Projets qui échoue est journalisée et laisse l'Opération
  hors Projet. Sur le fil, l'Opération gagne `"projects": [...]`, dans `/ws` comme dans
  `on_event`.
- **Le Manifest déclare les Ressources d'une extension** (`Manifest::resources`,
  `Manifest::with_resource(ResourceDeclaration)`) : la sorte (`tache`), un libellé, une ligne
  d'aide et les noms de ses verbes (`create`, `update`, `delete`, ou le nom d'une action,
  `Verb::name`).
- **Le catalogue des Ressources** (`Resource`) : les 19 Ressources du core (`ResourceKind::CORE`,
  `core_resources()`, libellé, aide et tous les verbes que ses use cases emploient) puis, dans
  l'ordre d'enregistrement, celles de chaque extension sous `<id>/<sorte>`. Exposé par
  `Host::catalogue()` (méthode par défaut : le catalogue du core seul) et par `GET /resources`
  (`[{resource, label, description, verbs, extension}]`, `extension` à `null` pour le core).
- **Version** : `pdo-extension-api` passe en **0.8.0** (ajouts : un champ de `Operation` et de
  `Manifest`, deux types, des méthodes). Les structures sont `#[non_exhaustive]` et la méthode de
  l'Hôte a une valeur par défaut : une extension 0.1 à 0.7 compile et tourne telle quelle,
  `OLDEST_COMPATIBLE_API_VERSION` reste `0.1.0`.
- L'extension de test interne (`test`, celle du daemon de démo des Feature Paths) joue ici le rôle
  de `fixture` : elle déclare `test/note`, ses pairs d'interopérabilité déclarent `<id>/item` et
  remplissent les Projets de leurs propres Opérations, et `GET /ext/test/operations` rend chaque
  Opération reçue avec ses Projets.

Écartés : **laisser l'extension déduire le Projet** du dépôt ou du Run : chaque extension
refaisait le calcul, avec le risque de diverger du core. **Un Projet unique** (`Option`) : un
Backlog de `pm` a plusieurs Projets, et le moindre privilège d'`iam` porte sur la liste. **Remplir
les Projets dans l'Hôte du core**, une fois pour toutes : `authorize` est synchrone et la lecture
des Projets ne l'est pas ; chaque use case le fait là où il connaît son Run.
