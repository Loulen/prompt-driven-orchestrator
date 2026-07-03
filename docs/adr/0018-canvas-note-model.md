# Note de canvas — bloc racine `notes:`, jamais un type de nœud

Le canvas gagne des **notes** : des annotations de documentation inertes que le
designer épingle près d'un groupe de nœuds pour expliquer une intention (#307). Le
« + » de la toolbar devient un dropdown (Node | Note). L'instinct par défaut — « c'est
un élément du canvas, donc un type de nœud » — vient d'être renforcé par ADR-0017
(`script` = nouvel arm `NodeType`). Mais une note ne s'exécute jamais, ne spawne aucune
session, ne produit aucun artefact et n'a pas d'état terminal : la traiter comme un
`NodeType` forcerait chaque `match NodeType` + le scheduler / spawn / admission /
`outputs_validator` / détection-de-vie à gérer un arm « qui ne tourne pas », avec le
risque qu'une note soit accidentellement schedulée. Le coût est mesuré : `NodeType` est
référencé dans **16 fichiers Rust** (313 occurrences ; `graph_resolver.rs` seul en a 77),
dont ≥ 3 `match` exhaustifs qui casseraient à la compilation et une nuée d'arms `_ =>`
qui avaleraient silencieusement une note comme un nœud de DAG.

**Décision : modéliser les notes comme un bloc racine `notes:` du YAML** (sibling de
`loops:`/`edges:`), chaque entrée `struct Note { id, content, view: Option<ViewPosition> }`.
Le runtime **ignore entièrement** ce bloc — il n'entre ni dans l'ordonnancement, ni dans
le dataflow, ni dans le graphe de nœuds (les consommateurs itèrent `pipeline.nodes`,
jamais `pipeline.notes`).

- **`content` = sémantique, `view` = layout.** `view` (position, et plus tard taille)
  suit la règle établie du `view` d'un nœud et du `mode`/`waypoints`/`target_side` d'une
  edge : persisté dans le fichier, **exclu du diff sémantique**. `content` est du texte
  brut en v1 (markdown = enhancement ultérieur qui rouvre ADR-0013).
- **Rendu = custom element xyflow** (ADR-0003). Un « xyflow node » de type `note` est un
  détail de rendu React-Flow et n'est **pas** un PDO `NodeType` — la distinction est
  volontaire et load-bearing.
- **Champ Rust `notes` obligatoire** sur `PipelineDef` (+ émission TS livrée dans le même
  changement, cf. #296). Le frontend ne parse jamais le YAML : il consomme la forme
  **parsée par le daemon**. Une note écrite dans le fichier mais absente de `PipelineDef`
  survit au fichier mais **disparaît au reload** (drop silencieux de serde, parser
  lenient). `"notes"` doit aussi entrer dans `KNOWN_TOP_LEVEL_KEYS` (sinon warning
  parasite `unknown field 'notes' (ignored)`, attrapé par le gate
  `known_keys_cover_serialized_pipeline`).

**Pourquoi.** Suit la forme éprouvée du bloc `loops:` (ADR-0011 : entité nommée de
premier niveau, rendue sur le canvas, jamais un nœud) et l'instinct « pas de nouveau type
de nœud » d'ADR-0016. Sort la note du chemin d'exécution **par construction**, au lieu de
la neutraliser par des gardes disséminés qu'un refactor futur oublierait. Divergence
assumée d'avec ADR-0017 : `script` méritait un arm `NodeType` *parce qu'il s'exécute* ;
une note ne s'exécute pas. Le critère n'est pas « est-ce sur le canvas » mais « est-ce que
ça tourne ».

**Alternatives écartées.**
- **`NodeType::note`** — réutilise l'infra de nœud (render / sélection / drag / undo /
  persistance / bibliothèque), mais impose de gérer un arm non-exécutant sur ~16 fichiers
  (scheduler, spawn, admission, validateurs, détection de vie) et laisse fuir le risque
  d'un scheduling accidentel. Rejeté : le coût de « nœud qui n'est pas un nœud » dépasse
  la réutilisation.
- **Annotation éphémère xyflow, non persistée** (ou stockée dans un blob `meta`) — zéro
  schéma, mais la note ne voyage pas avec le pipeline partagé ni ne survit au reload.
  Rejeté : une note *est* de la documentation durable.

**Conséquences.**
- Nouveau plumbing frontend (render / sélection / drag / undo) **non partagé** avec les
  nœuds. Trois pièges « la note n'est pas un node » à neutraliser explicitement :
  persistance du drag (`updateNodeViews` ne mappe que `pipeline.nodes`), Delete du
  context-menu (`nodes.find` → no-op sur une note), et ne jamais confondre le type de
  rendu xyflow avec un `NodeType`.
- Les notes vivent sur `PipelineDef.notes` → couvertes par l'undo COW d'ADR-0014
  gratuitement, **à condition** que chaque reducer réaffecte le tableau (jamais de mutation
  en place, sinon corruption d'undo silencieuse).
- `mutation_validator` doit laisser les notes **librement mutables pendant un Run**
  (inertes, aucune session à orphaner) ; un edit de note émet `PipelineModified{kind:"yaml"}`
  → relecture scheduler no-op (ADR-0007).
- Décision de schéma persistant : une fois des pipelines `notes:` dans la nature, basculer
  vers `NodeType::note` exigerait le `pipeline_migrator` **plus** le travail sur les 16
  fichiers évité ici. Réversible par migration, donc « difficile à inverser ».

**Portée v1 (différé).**
- **Contenu markdown / mermaid** dans les notes : rouvre ADR-0013 (2ᵉ surface
  react-markdown + sink `dangerouslySetInnerHTML`, contenu humain-mais-rendu = nouvelle
  classe de confiance) → exigera son propre ADR/amendement. v1 = texte brut.
- **Note redimensionnable** (champ `size` layout-class à threader sur 5 couches et à
  strip du diff) : fast-follow. v1 = taille pilotée par le contenu.
- **Note en bibliothèque** (`POST /library/nodes`) : une note est pipeline-spécifique, pas
  un artefact réutilisable.
- Imbrication note↔loop-region, couleurs, ancrage/pin.

**Relations.** Suit ADR-0011 (bloc top-level nommé, non-nœud) et l'instinct d'ADR-0016
(pas de nouveau type de nœud). Diverge délibérément d'ADR-0017 (arm `NodeType` réservé à
ce qui s'exécute). Hérite d'ADR-0003 (rendu xyflow), ADR-0014 (undo COW), ADR-0007
(édition pendant un Run). Protège la frontière d'ADR-0013 en restant texte brut. Ne
supersede aucun ADR.
