# Trois couches de primitives runtime : resolver, mutations, commandes

> **Amendé par ADR-0049.** Une commande de reprise **humaine** (Couche 3) peut **ré-ouvrir un run terminal** via une re-projection sûre — un geste explicite, jamais une initiative du runtime. Le principe tient : un **bouton de nœud** ne réanime pas un run de lui-même, et le runtime ne flippe jamais le `RunStatus` seul.

**L'API runtime se lit en trois couches, et les couches basses ne re-rentrent jamais dans le scheduler.** Sans cette séparation on retombe sur des commandes monolithiques qui mélangent requête sur le graphe, mutation d'état et décision de scheduling — le postmortem du run `20260513-094606-dcdf206` (#108) a montré `resume_run` spawnant des rogue iters et `kill_node` ré-entrant dans le scheduler.

- **Couche 1 — Graph resolver.** Fonctions **pures** : graphe + état → informations (downstream, nœuds prêts, corps d'une boucle, restants avant complétion). Elle existe aussi parce que le manager est un agent LLM qui peut mal raisonner sur la topologie : il lui faut une réponse fiable sur « qu'est-ce qui est downstream » sans traverser le YAML lui-même.
- **Couche 2 — Primitives de mutation.** Atomiques, sur un ou des nœuds, **sans aucune logique de scheduling** ni traversée de graphe : invalider prend une **liste explicite**, que le caller construit via le resolver.
- **Couche 3 — Commandes de commodité.** Séquences **linéaires** de primitives, jamais d'appels récursifs. Le scheduler en est une.

Les trois consommateurs ont accès aux trois couches : le scheduler utilise surtout la 3, le manager peut descendre en 1+2 pour des interventions chirurgicales, l'UI expose la 3 via des boutons.

**L'interpréteur d'actions du scheduler (#357).** Les évaluateurs purs sont la Couche 1 concrète ; l'exécution de leur liste d'actions est une Couche 3 : un interpréteur **linéaire unique** partagé par les trois pilotes (avance de run, complétion de nœud, ré-évaluation post-commande). La seule divergence historique entre pilotes — ré-appliquer ou non la dé-duplication de spawn — est portée par un **argument typé**, plus par une copie de code silencieusement désynchronisée.

## Stop et kill sont deux verdicts distincts, à ne jamais unifier

Un arrêt *demandé* (bouton Stop, route par-nœud) laisse le nœud `stopped` (`NodeStopped`) ; le `kill_node` du manager tranche un travail en cours et émet `NodeFailed`. Gardés différemment à la projection, rendus différemment dans l'UI. Les « unifier » serait une régression de sens, pas un refactor.

## La primitive de spawn de référence porte les gardes elle-même (#236)

Deux primitives de spawn coexistent, avec des garanties **inégales**. La primitive de référence porte **elle-même** le garde de transition, le cap d'admission, la précondition sandbox, l'isolation de panic et le contexte de région `collection` — délibérément pas ses appelants, pour que l'invariant tienne pour tout appelant présent et futur. L'autre (synchrone, historique) n'en porte **aucune** : c'est du legacy à résorber, pas un second point d'entrée légitime. **Toute voie de spawn nouvelle passe par la primitive de référence.**

Preuve par l'incident (#487) : `node_retry` appelait la primitive legacy sans en re-coder les garanties — sur un Run terminal, un clic spawnait une session orpheline, fuyait un sous-worktree et répondait un `200` menteur. Il route désormais par la référence, plus une sonde de tête run-liveness (**le refus doit précéder le stop et l'invalidation**, sinon l'auto-invalidation gèle le nœud). Reste `force_spawn_node`, qui **re-code** les trois gardes chez lui : correct mais non résorbé, dernier appelant HTTP de la primitive legacy.

## La surface HTTP de Couche 3 (#236)

Deux surfaces à ne pas confondre : les routes par nœud (boutons du canvas) et `POST /runs/{id}/commands` (manager). Sur cette dernière, la majorité des bras **ne composent aucune primitive, et c'est correct, pas un manque** : la Couche 2 est un vocabulaire de mutation **de nœud**, et la moitié du pilotage d'un *Run* (pause, resume, extension de boucle, renommage, cleanup, retry-all qui forke un Run neuf) n'est pas une mutation de nœud — forcer ces bras à traverser une primitive inventerait la primitive pour sauver le schéma.

**Le cœur extrait de cette surface retourne la réponse HTTP telle quelle, pas un verdict sémantique.** Choisi contre *un enum de verdict par bras mappé en HTTP*, prouvablement **lossy** sur les triplets (statut, content-type, corps) émis : corps `404`/`409` non uniformes entre bras, cinq formes de succès qu'aucun enum n'exprime, et le `201` de création forwardé **verbatim** par `retry_all` — que le frontend lit pour naviguer vers le Run retryé. Un enum casserait des réponses filaires que le manager et le frontend lisent déjà, alors qu'ADR-0025 fait de leur véracité une règle. Retourner la réponse est **lossless par construction**.

**Corollaire d'ordonnancement.** Le parse du `kind` d'une commande s'installe **après** la gate 410 d'ADR-0024, jamais avant : une commande malformée contre un Run oublié reste un `410`, pas un `400`.
