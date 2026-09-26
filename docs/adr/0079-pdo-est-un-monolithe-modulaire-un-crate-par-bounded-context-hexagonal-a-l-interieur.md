# PDO est un monolithe modulaire : un crate par bounded context, hexagonal à l'intérieur

> Statut : accepted (grilling du 2026-09-26, story « architecture extensible »).
> **Remplace ADR-0039.** **Amende ADR-0009** : les trois couches deviennent les couches hexagonales
> *domain* (primitives pures), *application* (use cases, ports, orchestration) et *adapters*
> (le bord). Vocabulaire du rangement : `docs/agents/module-layout.md`.

## Contexte

Le daemon tenait en un seul crate plat : 93 fichiers frères, 172k lignes, dont un `lib.rs` de 51k
lignes qui portait les 144 routes, un état global d'environ 35 champs et des requêtes SQL brutes.
Le code n'avait que trois traits. Ni un humain ni un agent ne pouvait trouver « où vit le
comportement X » sans grep. ADR-0039 justifiait cette platitude par trois raisons. La troisième,
« aucun second consommateur des types du daemon », ne tient plus : l'API d'extension (ADR-0081)
en est un. Les deux autres (cibles `tracing`, `pub` de fuite) sont des coûts qu'on accepte.

## Décision

Il y a deux axes orthogonaux, et chacun a son outil :

- **L'axe métier** (DDD, bounded contexts) devient une **frontière de crate**. On en compte 12 :
  `run`, `pipeline`, `review`, `session`, `harness`, `sandbox`, `skill`, `trigger`, `stats`,
  `workspace`, `settings`, `platform`. S'y ajoutent un petit **noyau partagé** (Acteur,
  Opération, autorisation, bus d'événements, hôte d'extensions, attributs), le crate de l'API
  d'extension, et le binaire `pdo`, qui est la **composition root** (il branche les adaptateurs
  sur les ports, assemble les routers de chaque contexte, porte la CLI et la SPA).
- **L'axe technique** (hexagonal : domain, application, adapters) devient une **frontière de
  module** à l'intérieur de chaque crate de contexte. La règle de dépendance (le domaine
  n'importe ni I/O, ni axum, ni sqlx, ni les adaptateurs) est vérifiée par un **test
  d'architecture** de la suite.

Le compilateur fait donc respecter la frontière qui manquait le plus, entre sujets métier : un
contexte ne voit d'un autre que ce qu'il rend `pub`. Et comme cargo interdit les cycles entre
crates, la carte des contextes est acyclique par construction. Les ports sont des traits injectés
en `Arc<dyn Port>`. L'état global disparaît : chaque use case reçoit seulement ses ports.

Le frontend suit les mêmes 12 contextes (un dossier chacun, avec `api/`, `model/`, `store/`,
`components/`, plus un dossier `shared/`). Ses types d'API sont **générés depuis les DTO Rust**
(ts-rs) et ne sont plus écrits à la main.

## Alternatives écartées

- **Un crate par couche, contexte en dossier** (*layer-first*). Le compilateur ferait respecter la
  règle hexagonale, mais chaque contexte serait répété dans quatre crates : modifier un
  comportement de « trigger » voudrait dire en ouvrir quatre. Les frontières entre contextes, qui
  sont le vrai mal de ce code, ne seraient protégées nulle part. Et toute modification du domaine
  recompilerait la chaîne entière en aval.
- **Un crate par case contexte × couche** (~36 crates). Tout serait vérifié, mais au prix d'une
  cérémonie démesurée pour un dev solo.
- **Un crate unique rangé en dossiers.** Aucune frontière n'y serait vérifiée par le compilateur,
  et on retomberait dans le spaghetti dès qu'on relâche l'attention.
- **Les génériques plutôt que `dyn`** pour les ports. Ils se propagent dans toutes les signatures
  et font exploser les temps de compilation, pour un gain invisible dans un daemon dont le coût
  est dans les I/O.

## Conséquences acceptées

- **Les cibles `tracing` changent une fois.** `RUST_LOG=pdo_daemon::x` devient
  `RUST_LOG=pdo_<contexte>::…`, et le runbook donne la table de correspondance.
- **Les cycles actuels entre modules doivent être cassés** pendant la migration. Les dépendances
  mesurées sont surtout arborescentes (stats → run, workspace, harness, sandbox ; trigger → run,
  harness, skill). L'event log appartient au contexte `run`, et le bus au noyau.
- **Le contrat HTTP et WebSocket reste gelé** pendant toute la migration, qui se fait contexte
  par contexte.
