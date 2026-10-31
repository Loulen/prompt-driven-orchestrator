# Les contrôles structurels tournent dans un crate `tidy`, avant toute suite longue

> Statut : accepted (grilling du 2026-10-31, CORE-156). Numéro attribué à la fusion vers `main`.

## Contexte

Les tests qui lisent le dépôt comme du texte (taille des fichiers, couverture des routes, règle
hexagonale, liens de la doc, version du module partagé du front) répondent en moins de 3 s, mais
vivaient dans le binaire de test d'intégration, qui lie tout le démon. Un worktree de Run compile
à froid (7 à 9 min par build, trois Runs en parallèle), puis la suite s'arrêtait au premier
échec : ces tests ont cassé après 15 à 25 min, sept fois sur une story, et chaque échec a coûté
une relance complète.

## Décision

1. Ces tests vivent dans un crate `tidy` du workspace, non publié, sans autre dépendance que la
   bibliothèque standard. Avec `cargo fmt --check`, il forme le *fast gate*, lancé avant toute
   suite longue ; il répond en moins d'une minute, même sur un `target/` froid. Un test qui a
   besoin du code d'un crate du produit reste hors de `tidy`.
2. Trois gates : *fast gate* pendant le dev, *scoped gate* sur le périmètre du diff, *full gate*
   (tout ce que la CI déclare, sans fail-fast) une seule fois avant la livraison d'un ticket.
   L'E2E du front sort du *full gate* : il tourne une fois par story, avant la fusion vers
   `main`, pour que le gate d'un ticket ne paie pas une suite navigateur que ses voisins
   relancent aussi.
3. Les Runs réutilisent la compilation des dépendances par sccache, activé par le Makefile
   quand il est installé.

## Alternatives écartées

- **Un filtre nextest sur le binaire d'intégration** : la minute n'est tenue qu'avec un `target/`
  chaud, ce qu'un worktree de Run n'a jamais.
- **Un `CARGO_TARGET_DIR` partagé entre worktrees** : les Runs parallèles se sérialisent sur le
  verrou de cargo, et le profil de test a été réglé pour l'inverse (`incremental = false`).
- **Copier un `target/` de référence à la création du worktree** : demande un seam du core dans
  la création des worktrees, pour un gain que sccache donne déjà sur les dépendances.
