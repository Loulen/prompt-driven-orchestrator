# Les constructs mécaniques exigent un upstream typé à l'édition

*Amended by ADR-0011 : écrite à l'époque des nœuds `Switch`/`ForEach`, la règle s'applique depuis aux clauses `when:` des edges et au driver `over:` des boucles `collection`.*

**Une clause `when:` ou un driver `over:` ne sont éditables que si l'upstream déclare un `frontmatter:` schema couvrant le champ référencé** — l'éditeur propose des dropdowns au lieu d'une saisie libre. Sans cette contrainte, une `when:` qui réfère à un champ inexistant ne fire **jamais**, la pipeline route en `else`, et rien ne signale la cause : le déterminisme runtime ne pardonne pas. Une `when:` peut référer à une variable pipeline `$<name>` à la place ; la règle porte alors sur le block `variables:`. À la déconnexion d'une edge, l'éditeur reset les prédicats sur frontmatter (ceux sur variables restent intacts).

**Conséquence en cascade** : dès qu'un nœud est routé par une `when:` sur frontmatter ou drive une boucle `collection`, il *doit* déclarer le schéma de l'output concerné — y compris les agents LLM (Reviewer, Planner), dont le schéma reste optionnel partout ailleurs.

**Pourquoi ça ne contredit pas *Sharp tool* (ADR-0001).** L'esprit d'ADR-0001 vise le **design** — formes de graphe exotiques autorisées, pas de validation prescriptive sur la *forme*. Ici on est sur l'**intégrité d'un contrat mécanique** entre deux nœuds : autre registre. Écarté aussi : *introspecter les artefacts des runs précédents* pour autocompléter — ça mélange contrat déclaratif et observation runtime, et un agent LLM peut produire un champ imprévu une fois sur deux.

Pas de migration auto des pipelines sans schéma : un diagnostic info-only au chargement dit « cette clause référence un champ que son upstream ne déclare pas — la branche ne fire jamais », l'utilisateur tranche.
