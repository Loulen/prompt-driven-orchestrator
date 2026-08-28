# ADR-0058 — Le total d'un Run est la somme de ses contributions attribuées

> Statut : accepted (issue #647, 2026-08-28).
> Amende ADR-0022 et ADR-0029. Complète ADR-0052.

## Contexte

Le détail d'un Run et Stats calculaient son coût par deux folds distincts. Le premier parcourait
tous les transcripts du Run ; le second calculait une contribution par exécution puis un résiduel
`total - attribué`. Ces chemins divergeaient notamment lors d'un redémarrage Copilot et pouvaient
facturer deux fois un message Claude rejoué dans une autre session.

Les mesures des journaux réels établissent deux invariants. Le `totalNanoAiu` cumulatif d'une
session Copilot inclut déjà ses subagents. Les transcripts d'un subagent Claude ne sont accessibles
que sous le répertoire de la session parente. Aucun subagent ne constitue donc une exécution de
Node indépendante.

## Décision

Un seul fold attribué alimente désormais le total du Run, le coût du Node et Stats.

- Chaque fichier Claude appartient à la session dont il porte l'identité ; son sous-répertoire
  `subagents/` appartient au même Node parent.
- Les messages Claude sont dédupliqués à l'échelle du Run par `(message.id, requestId)`, dans
  l'ordre des exécutions. Un message ne peut contribuer qu'une fois.
- `Infrastructure` est un bucket lu directement, plus un résiduel. Un transcript ambigu reste
  `Non attribué`.
- Chaque session Copilot contribue son maximum `totalNanoAiu`, converti par la constante publiée
  d'ADR-0052. Ses tokens ne sont jamais retarifés.
- Un harnais sans source de coût rend le total indisponible (`—`) sans effacer les tranches
  calculables. Un coût de Node inconnu reste `null`, jamais zéro.

Le coût reste dérivé à la lecture et n'est jamais persisté. Le champ projeté sur un Node agrège
toutes ses exécutions, comme ses temps d'en-tête et la ligne Node de Stats.

## Alternative écartée

Conserver les deux folds et ajouter un test de réconciliation. Écarté : deux comportements
existants étaient déjà couverts par des tests contradictoires. Deux implémentations indépendantes
ne peuvent pas garantir structurellement l'attribution unique demandée.

