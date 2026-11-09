# La Marque est déclarée par le binaire à la compilation

Le nom et le logo affichés par l'interface (la **Marque**) sont déclarés par le binaire lui-même, à la compilation, comme son Canal de mise à jour. Le core connaît deux Marques, **PDO** (le défaut du vanilla) et **Acogni Driver** ; la Distribution choisit la sienne. Aucun texte du front ne nomme le produit en dur : titre de la fenêtre, favicon, barre du haut, modale de bienvenue, tours, Version & update lisent tous la Marque. Les Thèmes ne dépendent pas de la Marque : les deux sont proposés partout, Acogni par défaut (ADR-0093).

Concrètement : `Config::with_brand(Brand::AcogniDriver)`, à côté de `with_release_channel` ; sans cet appel le binaire est PDO. Le daemon écrit la Marque dans la page qu'il sert (`<title>`, `<html data-brand>`) avant tout rendu, et sert ses logos sous `/shared/brand/` (`mark.svg`, `lockup.svg`, `favicon.svg`) pour les pages servies hors de l'interface. Le front ne nomme le produit qu'à un endroit, `shared/ui/brand/brand.ts` ; une garde du design system fait échouer les tests sur tout autre nom en dur. L'engrenage reste le logo d'Acogni, l'éditeur : il devient l'icône de Settings et signe « by Acogni ».

## Considered Options

- **Marque déclarée par une Extension** : une Extension décrit une capacité, pas le binaire ; deux extensions pourraient se contredire. Même raison que pour le Canal de mise à jour.
- **Marque choisie au runtime** (variable d'environnement, réglage) : un vanilla pourrait s'afficher « Acogni Driver » sans en avoir les extensions.
- **Logo fourni librement par la Distribution (SVG)** : aucune autre Distribution n'existe ; on l'ouvrira le jour où un tiers en aura besoin.
