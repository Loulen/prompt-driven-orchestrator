# Trois couches de primitives runtime : resolver, mutations, commandes

> **Amendé par ADR-0049 (spec résilience, 2026-08-24).** Une commande de reprise **humaine**
> (Couche 3) peut **ré-ouvrir un run terminal** via une re-projection sûre — un geste explicite
> (`reopen_run`, ou une commande ciblée qui l'embarque), jamais une initiative du runtime. Le
> principe tient : un **bouton de nœud** ne réanime pas un run de lui-même ; c'est l'action de
> reprise **de l'humain** qui ré-ouvre, et le runtime ne flippe jamais le `RunStatus` seul.

Le daemon exposait des commandes (`resume_run`, `restart_node`, `mark_node_done`, `kill_node`) qui mélangent requête sur le graphe, mutation d'état et décision de scheduling dans un même appel. Le scheduler, le manager et l'UI partagent ces commandes, mais aucun ne peut agir avec plus de granularité que ce qu'elles offrent. Le manager (agent Claude Code conversationnel) ne peut pas inspecter le graphe, choisir un sous-ensemble de nœuds à invalider, puis agir — il est forcé de passer par des commandes opaques.

**Décision : l'API runtime est structurée en trois couches explicites.**

- **Couche 1 — Graph resolver.** Fonctions pures, sans side-effect : graphe + état du run → informations (downstream d'un nœud, nœuds prêts à spawner, corps d'une boucle, nœuds restants avant complétion).
- **Couche 2 — Primitives de mutation.** Opérations atomiques sur un ou des nœuds (spawner, stopper, injecter des outputs, invalider une liste explicite de nœuds — jamais de traversée de graphe, le caller construit la liste via le resolver), sans aucune logique de scheduling.
- **Couche 3 — Commandes de commodité.** Composent resolver + primitives pour les cas courants (le retry d'un nœud = downstream via resolver → invalidation → re-spawn). Le scheduler lui-même en est une.

Le scheduler, le manager et l'UI ont accès aux trois couches : le scheduler utilise principalement la 3 ; le manager peut descendre en 1+2 pour des interventions chirurgicales ; l'UI expose la 3 via des boutons.

**Pourquoi.** Le postmortem du run `20260513-094606-dcdf206` (#108) a montré que chaque commande monolithique déclenchait des effets en cascade incontrôlables — `resume_run` spawnait des rogue iters, `kill_node` re-entrait dans le scheduler. La séparation en couches coupe ce couplage : **les primitives ne re-entrent jamais dans le scheduler**, et les commandes de commodité sont des séquences linéaires de primitives, pas des appels récursifs. Et le manager est un agent LLM qui peut mal raisonner sur la topologie du graphe — le resolver lui donne une réponse fiable sur « qu'est-ce qui est downstream » sans qu'il ait à traverser le YAML lui-même.

## L'interpréteur d'actions du scheduler (#357)

Les évaluateurs purs du scheduler sont la Couche 1 concrète : ils produisent une liste d'actions sans toucher au monde. L'exécution de cette liste est une Couche 3 : un interpréteur **linéaire unique**, partagé par les trois pilotes (avance de run, complétion de nœud, ré-évaluation post-commande), qui ne réentre jamais le scheduler (ni les pilotes, ni lui-même). La seule divergence historique entre pilotes — ré-appliquer ou non la dé-duplication de spawn — est portée par un argument typé de l'interpréteur, et non plus par une copie de code silencieusement désynchronisée.

## Stop et kill sont deux verdicts distincts, à ne jamais unifier

Un arrêt *demandé* (bouton Stop du canvas, route par-nœud) laisse le nœud `stopped` (événement `NodeStopped`) ; le `kill_node` du manager tranche un travail en cours et émet `NodeFailed`. Les deux sémantiques coexistent volontairement : gardées différemment à la projection et rendues différemment dans l'UI. Les « unifier » serait une régression de sens, pas un refactor.

## La primitive de spawn de référence porte les gardes elle-même (#236)

Deux primitives de spawn coexistent, avec des garanties inégales. La primitive de référence porte **elle-même** le garde de transition (#212), le cap d'admission, la précondition sandbox (#445), l'isolation de panic et le contexte de région `collection` — délibérément pas ses appelants, pour que l'invariant tienne pour tout appelant présent et futur. L'autre (synchrone, historique) n'en porte **aucune** : c'est le legacy à résorber, pas un deuxième point d'entrée légitime. **Toute voie de spawn nouvelle passe par la primitive de référence.**

**Résorption (#487).** `node_retry` (bouton Retry/Play) appelait la primitive legacy et **n'en re-codait aucune garantie** — d'où l'incident : sur un Run terminal, un clic spawnait une session orpheline, fuyait un sous-worktree et répondait un `200` menteur. Il route désormais par la primitive de référence, plus une sonde de tête run-liveness (le refus doit précéder le stop et l'invalidation, sinon l'auto-invalidation gèle le nœud). La résurrection sœur `GET …/pane` d'une session morte passe elle aussi par la porte d'admission et laisse une trace `NodeStarted` (#487 §3). Reste `force_spawn_node` (bouton Start), qui **re-code** les trois gardes chez lui au lieu d'appeler la primitive de référence : correct mais non résorbé — le dernier appelant HTTP de la primitive legacy.

## La surface HTTP de Couche 3 (#236)

Il y a **deux** surfaces de Couche 3, à ne pas confondre : les routes par nœud `POST /runs/{id}/nodes/{node_id}/{start,stop,retry}` (boutons du canvas) et `POST /runs/{id}/commands` (manager). Sur cette dernière, la majorité des bras **ne composent aucune primitive, et c'est correct, pas un manque** : la Couche 2 est un vocabulaire de mutation **de nœud**, et la moitié du pilotage d'un *Run* (pause, resume, extension de boucle, renommage, cleanup, retry-all qui forke un Run neuf) n'est pas une mutation de nœud — forcer ces bras à traverser une primitive inventerait la primitive pour sauver le schéma.

**Le cœur extrait de cette surface retourne la réponse HTTP telle quelle, pas un verdict sémantique.** Choisi contre l'alternative *« un enum de verdict par bras, mappé en HTTP par un unique mapper »* parce que ce mapper est **prouvablement lossy** sur les 22 triplets (statut, content-type, corps) que la surface émettait au moment de la décision : des corps `404`/`409` non uniformes entre bras, cinq formes de succès qu'aucun enum de verdict n'exprime, et le `201` de création de Run forwardé **verbatim** par `retry_all` — que le frontend lit pour naviguer vers le Run retryé. Un enum obligerait à choisir une forme canonique par statut, donc à casser des réponses filaires que le manager et le frontend lisent déjà — alors qu'ADR-0025 fait de la véracité de ces réponses une règle. Retourner la réponse est **lossless par construction** : le refactor devient un déplacement de code que le contrat filaire ne peut pas voir.

**Corollaire d'ordonnancement.** Le parse du `kind` d'une commande s'installe **après** la gate 410 d'ADR-0024, jamais avant : une commande malformée contre un Run oublié reste un `410`, pas un `400` — la précédence appartient à ADR-0024.
