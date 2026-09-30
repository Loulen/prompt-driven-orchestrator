# PDO s'étend par une API d'extension composée à la compilation, avec des hooks génériques

> Statut : accepted (grilling du 2026-09-26, story « architecture extensible »). Vocabulaire :
> CONTEXT.md § *Extensions*. S'appuie sur ADR-0079 (les mutations passent par des use cases).
> Amendé par #21 : comment une distribution obtient le frontend embarqué (§ *Amendements*).
> Amendé par #33 : la CLI d'une extension (`pdo ext`) et ses skills (§ *Amendement #33*).

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

## Amendement #33 : la surface agent, `pdo ext <id>` et les skills d'extension

Seams découverts en spécifiant l'extension Projet (`Loulen/pdo-extensions#4`) : un agent doit
piloter une extension depuis sa session comme il pilote le core, et savoir le faire sans
consigne du prompt.

- **Deux hooks de plus, deux défauts neutres** : `cli()` déclare une ligne de commande (un
  résumé, `None` par défaut) et `run_cli(invocation)` l'exécute ; `skills()` liste des skills
  embarqués (un `SKILL.md` et ses fichiers, aucun par défaut). `pdo-extension-api` passe en
  **0.2.0** : ajout rétrocompatible au source d'une extension (une extension 0.1 qui ne touche
  à rien compile et fonctionne), mais montée de mineure, donc rebuild obligatoire en 0.x.
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
