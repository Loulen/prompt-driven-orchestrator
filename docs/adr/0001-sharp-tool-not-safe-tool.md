# Sharp tool, not safe tool

PDO orchestre des pipelines d'agents Claude Code via un éditeur visuel. La tentation est forte de **prescrire la qualité** des pipelines (interdire un fan-out CM sans Reviewer downstream, forcer un Merger explicite, lint bloquant à l'enregistrement, warnings paternalistes à l'exécution).

**Décision : on ne le fait pas.** L'outil ne contraint pas et n'avertit pas l'utilisateur sur la qualité du design de ses pipelines. PDO fournit des primitives nettes (Node CM/DO, Cycle, Blackboard, Merge Resolver auto-spawné) et l'usage est libre. Si une pipeline est foireuse — fan-out non revu, accumulation infinie, deadlock conceptuel — c'est la responsabilité de son designer.

**Pourquoi.** Choisi contre l'alternative *"PDO valide le graphe avant exécution"* parce que (1) la frontière entre design "foireux" et design "intentionnellement exotique" est floue et changera avec les usages, (2) un outil prescriptif éduque ses utilisateurs à attendre des warnings et devient impossible à libéraliser plus tard sans surprendre, (3) la philosophie cible est *"primitives + composition libre"*, pas *"workflow vendor-prescribed"*. Conséquence pratique : pas de "lint pipeline" bloquant ; au max, un mode info-only désactivable ; l'éditeur permet les graphes exotiques (cycles, fan-out CM sans Merger explicite, ports déconnectés).
