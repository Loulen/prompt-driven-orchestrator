# La grille de câblage est une constante du produit, jamais un réglage

> Statut : superseded par ADR-0076 pour le pas (#877 : il devient un réglage S/M/L porté par le
> pipeline). Le geste `Shift` décrit ici tient toujours.
> Accepted à l'origine (grilling #840). Vocabulaire : CONTEXT.md § « Edges — structure ».

## Contexte

Le tracé des edges devient progressif et snappé à une grille (création par incréments, drag de segments). Il fallait fixer le pas de cette grille. Deux options : un réglage utilisateur (settings d'instance ou de projet) ou une constante embarquée.

## Décision

Le pas de la **grille de câblage** est une **constante du produit**, choisie une fois sur mockup, et le seul modificateur est le geste (`Shift` libère du snap et ré-origine la grille sur le point courant). Aucun réglage, ni global ni par pipeline.

Écarté : un pas configurable. Les `waypoints` sont persistés dans le fichier pipeline et voyagent avec lui (ADR §154) ; deux instances à pas différents rendraient le même YAML avec des tracés qui ne se redressent plus les uns sur les autres, et chaque drag re-snapperait sur une grille étrangère au tracé d'origine. Le partage de pipelines prime sur la personnalisation.

## Conséquences

- La grille décorative du fond et la grille de câblage sont deux choses ; elles peuvent coïncider sans que l'une dépende de l'autre.
- Changer le pas plus tard est une migration de rendu, pas de données : les waypoints existants restent valides, ils ne sont simplement pas alignés tant qu'on ne les touche pas (jamais de re-snap automatique).
