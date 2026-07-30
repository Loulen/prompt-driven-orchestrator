# Trois couches de primitives runtime : resolver, mutations, commandes

Le daemon expose aujourd'hui des commandes (`resume_run`, `restart_node`, `mark_node_done`, `kill_node`) qui mélangent requête sur le graphe, mutation d'état, et décision de scheduling dans un même appel. Le scheduler, le manager, et l'UI partagent ces commandes, mais aucun ne peut agir avec plus de granularité que ce qu'elles offrent — une commande `restart_node` qui résout les inputs, kill le node, invalide le downstream, et relance, tout en un bloc. Le manager (agent Claude Code conversationnel) ne peut pas inspecter le graphe, choisir un sous-ensemble de nodes à invalider, puis agir — il est forcé de passer par des commandes opaques.

**Décision : l'API runtime est structurée en trois couches explicites.**

**Couche 1 — Graph resolver.** Fonctions pures, sans side-effect, qui prennent le graphe + l'état du run et retournent des informations : downstream subgraph d'un node, nodes prêts à spawn, body subgraph d'un Loop/ForEach, nodes restants avant complétion. Le resolver existe déjà partiellement (`ready_nodes`, `compute_body_subgraph`) ; cette décision formalise la couche comme surface réutilisable.

**Couche 2 — Primitives de mutation.** Opérations atomiques sur un node ou un ensemble de nodes, sans logique de scheduling :
- `start_node(node_id, iter, overrides?)` — résout les inputs depuis le blackboard par défaut (suit les edges, lit les artifacts latest-iter de chaque upstream) ; les overrides permettent de remplacer un port spécifique par un chemin explicite. Spawn la session tmux + sous-worktree.
- `stop_node(node_id, iter)` — kill tmux + children, émet `node_failed`, pas de re-évaluation scheduler.
- `inject_outputs(node_id, iter, artifacts)` — écrit des fichiers d'artifacts pour un iter donné, pas de re-évaluation scheduler.
- `invalidate_nodes(Vec<node_id>)` — reset les nodes listés à `pending`, supprime leurs artifacts. Pas de traversée de graphe — la liste est explicite, le caller la construit (typiquement via le resolver).

**Couche 3 — Commandes de commodité.** Composent resolver + primitives pour les cas courants :
- `retry_node(node_id)` = resolver(downstream) → `invalidate_nodes(downstream)` → `start_node(node_id, next_iter)`.
- `invalidate_downstream(node_id)` = resolver(downstream) → `invalidate_nodes(downstream)`.
- Le scheduler lui-même est une commande de commodité : `scheduler_step(run_state)` = resolver(ready_nodes) → pour chaque ready node, `start_node(...)`.

Le scheduler, le manager, et l'UI ont accès aux trois couches. Le scheduler utilise principalement la couche 3. Le manager peut descendre aux couches 1+2 pour des interventions chirurgicales. L'UI expose la couche 3 via des boutons (Retry, Stop, Pause/Resume).

**Pourquoi.** Choisi contre l'alternative *"commandes monolithiques exposées en REST, le manager les appelle telles quelles"* parce que le postmortem du run `20260513-094606-dcdf206` (#108) montre que chaque commande existante déclenche des effets en cascade incontrôlables — `resume_run` spawne des rogue iters, `kill_node` re-entre dans le scheduler. La séparation en couches coupe ce couplage : les primitives ne re-entrent jamais dans le scheduler, et les commandes de commodité sont des séquences linéaires de primitives, pas des appels récursifs. Choisi contre l'alternative *"le manager appelle directement les primitives bas-niveau sans couche resolver"* parce que le manager est un agent LLM qui peut mal raisonner sur la topologie du graphe — le resolver lui donne une réponse fiable sur "qu'est-ce qui est downstream" sans qu'il ait à traverser le YAML lui-même.

## Addendum (#357) — L'interpréteur de `SchedulerAction`

Les évaluateurs `scheduler::*` (`evaluate_outgoing_edges_full`, `evaluate_loop_body_completion`, `evaluate_collection_barrier`, `seed_pending_loops`) sont la Couche 1 concrète : purs, ils produisent un `Vec<SchedulerAction>` sans toucher au monde. L'exécution de cette liste — `Spawn` via la primitive `spawn_node` (Couche 2), `Halt`/`Complete`/`SwitchRouted`/`Loop*`/`Collection*` via les émetteurs d'événements — est une Couche 3 : un interpréteur linéaire (`scheduler_interpreter::interpret`) partagé par `advance_run`, `handle_node_completion` et `re_evaluate_after_command`. Il ne réentre jamais le scheduler (n'appelle ni `advance_run`, ni `re_evaluate_after_command`, ni lui-même ; la re-projection inter-passes reste dans les pilotes). La seule divergence historique entre chemins — ré-appliquer ou non la dé-dup `spawn_superfluous` avant un `Spawn` — est portée par un argument typé `SpawnDedup { GuardSuperfluous, InternalOnly }`, et non plus par une copie de code silencieusement désynchronisée.

## Addendum (2026-07-30, #236) — la surface HTTP de Couche 3, et l'état réel de la Couche 2

**La puce `stop_node` ci-dessus décrit `kill_node`, pas la primitive.** *« kill tmux + children, émet
`node_failed`, pas de re-évaluation scheduler »* : la moitié « émet `node_failed` » est **fausse**.
`node_primitives::stop_node` émet **`NodeStopped`**, payload `{"reason":"stopped_by_user"}`. Les deux
sémantiques coexistent volontairement — un arrêt *demandé* (bouton Stop du canvas,
`POST /runs/{id}/nodes/{node_id}/stop`, et la tête de `node_retry`) laisse le nœud `stopped` ; le
`kill_node` du manager émet `NodeFailed` (`{"reason":"killed via kill_node command","source":"kill_node"}`)
parce qu'il tranche un travail en cours. Le bras `kill_node` **n'appelle donc pas** la primitive : il
ré-inline le kill tmux (plus le kill du process tree conteneur, #407) précisément pour changer le genre
d'événement. Ne pas « unifier » les deux : ce sont deux verdicts distincts sur le nœud, projetés
différemment (`NodeFailed` est gardé par `validate_fail` et par `iter >= node.iter` ; `NodeStopped` ne
l'est pas) et rendus différemment dans l'UI.

Trois autres écarts de cette section sont des **livraisons incomplètes**, pas des règles, et sont
consignés ici pour qu'on cesse de les lire comme un état des lieux :

- `inject_outputs(node_id, iter, artifacts)` a **zéro appelant de production** — seuls ses trois tests
  unitaires la touchent. Le seul candidat, le bras `inject_artifact` de `POST /runs/{id}/commands`,
  n'a pas le même modèle d'adressage : la primitive écrit dans
  `blackboard::port_dir(node, iter, port)/output.md`, la commande écrit un chemin relatif **libre**
  sous `.pdo/artifacts` (gardé anti-traversée). Les câbler l'un sur l'autre serait une **feature**, pas
  un refactor.
- Le paramètre `overrides` de `start_node` est **inatteignable** : les deux appelants de production
  passent `None`. Il n'est exercé que par un test unitaire.
- `invalidate_downstream(node_id)`, annoncée en Couche 3, **n'a jamais été livrée**. Son seul vestige
  est le resolver exposé en lecture seule par `GET /runs/{id}/nodes/{node_id}/retry/preview`.

**Deux primitives de spawn coexistent, avec des garanties inégales.** `node_spawn::spawn_node` (async,
appende ses propres événements, rend un `SpawnOutcome`) **porte elle-même** le garde de transition
(#212), le cap d'admission, la précondition sandbox (#445), l'isolation de panic et le contexte de
région `collection` — délibérément pas ses appelants, pour que l'invariant tienne pour tout appelant
présent et futur. `node_primitives::start_node` (sync, rend ses événements au caller) n'en porte
**aucune**. Appelants : `advance_run`, `scheduler_interpreter::interpret` et le bras `restart_node`
passent par `spawn_node` ; `force_spawn_node` (bouton Start de l'UI *et* commande `start_node`) et
`node_retry` (bouton Retry) passent par `start_node`. `force_spawn_node` **re-code les trois
préconditions chez lui** — d'où ses `409` documentés ; `node_retry` **n'en code aucune**. La primitive
de référence est `spawn_node` : toute voie de spawn nouvelle passe par elle, et `start_node` est le
legacy à résorber, pas un deuxième point d'entrée légitime.

**`run_command` / `dispatch` est la surface HTTP de Couche 3 que pilote le Pipeline Manager** — la liste
de « commandes de commodité » ci-dessus ne la nommait pas. Il y a en fait **deux** surfaces de Couche 3,
à ne pas confondre : les routes par nœud `POST /runs/{id}/nodes/{node_id}/{start,stop,retry}` (boutons du
canvas ; `node_retry` **est** la composition décrite plus haut, la seule livrée telle quelle :
`graph_resolver::downstream_subgraph` → `invalidate_nodes(aval)` → `invalidate_nodes(soi)` →
`start_node`), et `POST /runs/{id}/commands` (manager). Sur les douze bras de cette dernière, **neuf ne
composent aucune primitive, et c'est correct, pas un manque** : `pause_run` / `resume_run` /
`extend_cycle` / `bump_region` / `end_region` / `rename_run` appendent du contrôle de flot ou de la
métadonnée puis délèguent la re-planification à `re_evaluate_after_command` ; `mark_node_done` passe par
le garde de complétion partagé ; `cleanup_run` démonte worktrees et branches ; `retry_all` **forke un Run
neuf** et forwarde son `201`. La Couche 2 est un vocabulaire de mutation **de nœud** — la moitié du
pilotage d'un *Run* n'est pas une mutation de nœud, et forcer ces bras à traverser une primitive
inventerait la primitive pour sauver le schéma.

**Décision (#236) : le cœur extrait, `dispatch`, retourne un `axum::Response`, pas un `CommandOutcome`
sémantique.** Choisi contre l'alternative *« un enum de verdict par bras, mappé en HTTP par un unique
mapper »* parce que ce mapper est **prouvablement lossy** sur les 22 triplets (statut, content-type,
corps) que la surface émet aujourd'hui : sept `404 text/plain` « run not found » contre un `404` JSON
(celui de `force_spawn_node`), un `410` JSON (ADR-0024), un `409 text/plain` contre huit `409` JSON, des
corps `400`/`409` non uniformes, cinq formes de succès qu'aucun enum de verdict n'exprime, et le `201`
de `create_run_core` forwardé **verbatim** par `retry_all` — que le frontend lit pour naviguer vers le
Run retryé. Un enum obligerait à choisir une forme canonique par statut, donc à casser des réponses
filaires que le manager et le frontend lisent déjà — alors qu'ADR-0025 fait de la véracité de ces
réponses une règle. Retourner la `Response` est **lossless par construction** : le refactor devient un
déplacement de code que le contrat filaire ne peut pas voir, et ADR-0025 est préservée sans avoir à le
prouver bras par bras.

**Corollaire d'ordonnancement.** Le parse pur du `kind` s'installe **après** la gate 410 d'ADR-0024,
jamais avant, pour qu'une commande malformée contre un Run oublié reste un `410` et non un `400` — la
précédence appartient à ADR-0024, #236 ne fait que ne pas la casser.
