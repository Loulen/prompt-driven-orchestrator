# La Marque est déclarée par le binaire à la compilation

Le nom et le logo affichés par l'interface (la **Marque**) sont déclarés par le binaire lui-même, à la compilation, comme son Canal de mise à jour. Le core connaît deux Marques, **PDO** (le défaut du vanilla) et **Acogni Driver** ; la Distribution choisit la sienne. Aucun texte du front ne nomme le produit en dur : titre de la fenêtre, favicon, barre du haut, modale de bienvenue, tours, Version & update lisent tous la Marque. Les Thèmes ne dépendent pas de la Marque : les deux sont proposés partout, Acogni par défaut (ADR-0093).

## Considered Options

- **Marque déclarée par une Extension** : une Extension décrit une capacité, pas le binaire ; deux extensions pourraient se contredire. Même raison que pour le Canal de mise à jour.
- **Marque choisie au runtime** (variable d'environnement, réglage) : un vanilla pourrait s'afficher « Acogni Driver » sans en avoir les extensions.
- **Logo fourni librement par la Distribution (SVG)** : aucune autre Distribution n'existe ; on l'ouvrira le jour où un tiers en aura besoin.
