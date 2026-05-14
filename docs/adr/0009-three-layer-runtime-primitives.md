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
