# Version, CHANGELOG et numéro d'ADR se fixent à la fusion vers `main`

> Statut : accepted (grilling du 2026-10-31, CORE-156). Numéro attribué à la fusion vers `main`,
> selon la règle qu'il pose.

## Contexte

Chaque ticket bumpait la version du workspace, celle de l'API d'extension et du module partagé du
front, écrivait son entrée de CHANGELOG et prenait le numéro d'ADR suivant. Des tickets parallèles
d'une même branche d'intégration se disputaient donc les mêmes lignes : sur une story, le core est
passé de 2.53 à 2.67, cinq fusions sont entrées en conflit, et une fusion de `main` a renuméroté
l'API (0.17 -> 0.19) et un ADR (0091 -> 0092), suivie de 24 min de re-vérification.

## Décision

1. Un ticket ne touche à aucune version (workspace, API d'extension, module partagé du front,
   `Cargo.lock`). Il écrit son entrée sous `## Unreleased` en tête du CHANGELOG.
2. À la fusion `integration/*` -> `main`, une seule commande déduit le niveau semver des commits
   conventionnels depuis le dernier tag (`feat` mineur, `fix` patch, `!` majeur), bumpe toutes les
   versions, renomme `Unreleased` en titre de version et numérote les ADR en brouillon. L'agent
   qui fusionne confirme le niveau proposé. Un bump poussé sur `main` reste une release.
3. Un ADR naît en brouillon, `docs/adr/draft-<slug>.md`, et se cite `ADR draft-<slug>`. La même
   commande lui donne le numéro suivant et réécrit ses citations.

## Alternatives écartées

- **Bump par ticket, conflits résolus à la main** : c'est la situation de départ et son coût.
- **Numéro d'ADR pris au grilling, renuméroté à la main en cas de collision** : deux stories
  parallèles entrent encore en collision, et la renumérotation retouche les citations à la main.
- **Pas de section `Unreleased`, CHANGELOG écrit à la fusion** : celui qui fusionne devrait
  reconstituer l'entrée depuis les commits, sans le contexte des tickets.
