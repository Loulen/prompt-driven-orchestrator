# Chaque route a sa commande CLI, un test le vérifie

> Statut : accepted (grilling du 2026-11-05). Numéro attribué à la fusion vers `main`.

## Contexte

Un geste de l'UI passe toujours par une route du daemon, mais seule une partie de ces routes a
une commande `pdo` : un agent ou un script qui veut créer un Projet, déclencher un Trigger ou
terminer une Session hébergée écrit du `curl` à la main, avec le jeton, les en-têtes de session
et les corps de requête de l'UI. Rien ne signale une route ajoutée sans commande, et la logique
d'un geste calculée dans le frontend reste hors de portée de la ligne de commande.

## Décision

1. **La logique reste côté serveur.** Une commande est un client mince d'une route : elle envoie
   ce que l'UI envoie et affiche ce que le daemon répond. Ce qu'un geste calcule encore dans le
   frontend passe derrière une route avant d'avoir sa commande ; une règle (droits, validation,
   normalisation) n'est jamais réécrite dans la CLI.
2. **Chaque route a sa commande, ou une exclusion écrite et motivée.** Les exclusions vivent dans
   `crates/tidy/src/cli_parity/exclusions.rs`, en deux listes :
   - *exclues* : une raison par groupe de routes, quand la ligne de commande n'a rien à y faire
     (un WebSocket pour le navigateur, un terminal, une présence, une ressource avec laquelle
     l'UI dessine, un outil que le shell a déjà : `ls`, `git`) ; ajouter une entrée est une
     décision, relue en review ;
   - *provisoires* : des gestes qui méritent une commande pas encore écrite, rangés par famille
     et datés ; chaque commande écrite retire ses entrées.
3. **Un test du crate `tidy` le vérifie** (`crates/tidy/tests/cli_parity.rs`), dans le *fast
   gate*. Il lit le routeur et les sources de la CLI comme du texte, avec les lecteurs de la
   couverture des routes : une route sans commande ni exclusion le fait échouer, comme une
   exclusion qui nomme une route disparue, une route qu'une commande appelle déjà, ou une route
   exclue deux fois. Les sources de la CLI sont tout fichier sous un dossier `adapters/cli/` ou
   nommé `adapters/cli.rs`, et `crates/pdo/src/cli/`.
4. **Une commande d'opérateur suit le même contrat** : texte lisible par défaut, `--json` pour la
   réponse du daemon, codes de sortie `0` succès, `1` daemon injoignable ou refus (avec la phrase
   du daemon), `2` ligne de commande illisible, `4` sujet inconnu du daemon (comme `pdo run
   status`). Elle passe par le client partagé `pdo_platform::adapters::cli::daemon`, qui appelle
   en opérateur ou en Session de Node, avec `PDO_TOKEN`.

## Conséquences

- `pdo project`, `pdo trigger`, `pdo session`, `pdo audit` et `pdo whoami` couvrent les routes
  des Projets, des Triggers, des Sessions hébergées, du journal d'audit et de l'Acteur courant.
- Les routes des Runs, des Nodes, des Pipelines, des Settings, des Stats, de la banque de skills
  et de la Review sont des exclusions provisoires, à retirer famille par famille.
- Un appel s'écrit pour que le lecteur le voie : le chemin, littéral, en argument du verbe
  (`daemon.post(&format!("/triggers/{id}/fire"), None)`). Un chemin construit ailleurs ne compte
  pas.

## Alternatives écartées

- **Une table route -> commande relue en review, sans test** : elle dérive dès la première route
  ajoutée sans qu'on y pense.
- **Un passe-plat `pdo api <méthode> <chemin>`** : il couvre toutes les routes d'un coup, mais
  laisse à l'appelant les corps de requête et la logique de l'UI ; il ne dit pas quels gestes
  existent.
- **Un enregistreur des appels à l'exécution** : nextest lance un processus par test, aucun
  enregistreur ne voit les appels des autres (la même raison que la couverture des routes,
  ADR-0080).
