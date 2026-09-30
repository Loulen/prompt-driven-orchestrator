# PDO s'étend par une API d'extension composée à la compilation, avec des hooks génériques

> Statut : accepted (grilling du 2026-09-26, story « architecture extensible »). Vocabulaire :
> CONTEXT.md § *Extensions*. S'appuie sur ADR-0079 (les mutations passent par des use cases).
> Amendé par #21 : comment une distribution obtient le frontend embarqué (§ *Amendements*).
> Amendé par #34 : la ressource d'extension et l'Hôte (§ *Amendement #34*).

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
- **Montée de mineure de l'API : 0.1 -> 0.2** (une mineure est cassante en 0.x) : `ResourceKind`
  n'est plus `Copy` (la variante porte des `String`), `ExtensionContext` porte un champ de plus
  (`host`) et implémente `Debug`, `PartialEq` et `Eq` à la main (deux contextes sont égaux s'ils
  nomment le même dossier et partagent le même Hôte).
  Une extension 0.1 qui ne comptait pas sur `Copy` (dont `pdo-ext-hello`) compile telle quelle.
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
