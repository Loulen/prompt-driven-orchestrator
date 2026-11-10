# Le core sert son design system aux UI d'extension à l'exécution

> Statut : accepted (grilling du 2026-10-27, CORE-104). Vocabulaire : CONTEXT.md § *Design
> system partagé*, *Densité*.
>
> Amendé par ADR-0093 : les couleurs se choisissent par Thème, indépendant du Mode sombre.

## Contexte

L'interface adopte l'identité Acogni Driver et la densité d'un IDE, d'après les choix retenus
d'un prototype cliquable (une option par composant). Les UI d'Extension partagent le DOM du core
mais aucun contrat de style : une Extension recopie les tokens du core à chaque montée de version
et réimplémente ses primitives. Un changement de style du core ne lui parvient pas, et la copie
d'une Extension peut même masquer les valeurs du core dans la même couche CSS.

## Décision

1. Le core sert un **Design system partagé** : ses tokens (couleurs de chaque Thème, en sombre et en clair, échelle
   typographique, hauteurs, rayons, polices) et un jeu restreint de primitives, par le **module
   partagé** que l'import map fournit déjà à côté de React. Ils entrent dans la surface semver
   du module : retirer ou casser une primitive, c'est une nouvelle majeure.
2. Une **feuille de tokens à adresse stable** sert les pages qu'une Extension publie hors de
   l'interface (connexion…), qui ne chargent pas le module.
3. **Pas d'alias de transition** : les anciens noms de tokens disparaissent avec la refonte. Une
   Extension migre quand elle monte sur la version qui la livre.
4. Les polices sont embarquées dans le binaire avec leur licence (SIL OFL 1.1) ; l'interface
   n'appelle aucun CDN.

## Alternatives écartées

- **Un paquet du design system intégré au build** du core et de chaque Extension : chaque bundle
  embarque sa copie, une évolution du style exige de recompiler toutes les Extensions, et le core
  dépendrait d'un dépôt de marque.
- **Un contrat CSS seul** (tokens et classes) : laisse chaque Extension réimplémenter ses
  primitives, la duplication qu'on veut supprimer.
- **Garder les anciens noms en alias** le temps de la migration : une dette sans échéance ;
  refusée.
