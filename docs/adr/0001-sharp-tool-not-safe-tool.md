# Sharp tool, not safe tool

**PDO ne contraint ni n'avertit l'utilisateur sur la qualité du design de ses pipelines** — pas de lint bloquant à l'enregistrement, pas de Reviewer downstream imposé après un fan-out CM, pas de Merger forcé, pas de warning paternaliste à l'exécution. L'éditeur permet les graphes exotiques (cycles, fan-out CM sans Merger, ports déconnectés) ; si une pipeline est foireuse, c'est la responsabilité de son designer.

**Pourquoi**, contre *« PDO valide le graphe avant exécution »* : (1) la frontière entre design foireux et design intentionnellement exotique est floue et bougera avec les usages ; (2) un outil prescriptif éduque ses utilisateurs à attendre des warnings et devient impossible à libéraliser plus tard sans les surprendre ; (3) la philosophie cible est *primitives + composition libre*, pas *workflow vendor-prescribed*.

## Portée du « désactivable » (clarification #268)

Deux familles à ne pas confondre.

- Les **nudges advisory de design** (p. ex. la suggestion de fan-out collection) sont consultatifs et portent une croix de dismiss persistante.
- Les **diagnostics de correction** (`unknown field (ignored)`, `unknown node type, defaulting to doc-only`, edge dangling) ne sont **pas** des warnings de qualité : ils signalent une perte ou une altération **silencieuse** de config. Ils restent toujours visibles, sans croix.

Le mute *permanent* du lint de correction reste une décision humaine à part ; défaut : aucun mute. Réversible et additif — relâcher plus tard ne surprend personne, l'inverse oui (raison 2 ci-dessus).
