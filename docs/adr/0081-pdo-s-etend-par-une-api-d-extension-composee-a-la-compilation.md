# PDO s'étend par une API d'extension composée à la compilation, avec des hooks génériques

> Statut : accepted (grilling du 2026-09-26, story « architecture extensible »). Vocabulaire :
> CONTEXT.md § *Extensions*. S'appuie sur ADR-0079 (les mutations passent par des use cases).
> Amendé par #21 : comment une distribution obtient le frontend embarqué (§ *Amendements*).

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
- **Composition à la compilation.** Un binaire tiers dépend des crates du core par tag git et
  appelle `serve(config, extensions)`. **Seuls le crate d'API et `serve` sont couverts par le
  semver** : l'API démarre en **0.x**, et le reste du core ne promet rien. Axum est réexporté par
  l'API et fait donc partie du contrat.
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
