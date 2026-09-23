# La taille de la grille de câblage est un réglage S/M/L, porté par le pipeline

> Statut : accepted (#877). Supersede ADR-0072 sur le pas de la grille ; le geste qu'ADR-0072
> fixait (`Shift` libère du snap et ré-origine la grille sur le point courant) tient. Vocabulaire :
> CONTEXT.md § « Routage — grille de câblage ».

## Contexte

ADR-0072 avait fixé le pas de la grille de câblage à une constante du produit (40px), pour qu'un
pipeline partagé se rende pareil partout : les `waypoints` voyagent dans le fichier, et les jambes
perpendiculaires (au moins une cellule) se redérivent du pas à chaque rendu.

À l'usage, 40px est trop gros. Une carte de node (35px de haut) est plus courte qu'une cellule, le
tracé saute de 40px en 40px, et deux jambes de 40px ne tiennent plus entre deux cartes proches : le
routeur fait des crochets. L'utilisateur revient sur la décision.

## Décision

1. **Trois tailles** : `S` = 20px, `M` = 30px, `L` = 40px. **Défaut : `M`.**
2. **Le pipeline porte sa taille.** Le panneau de détail du pipeline (Pipeline Inspector, section
   *Canvas*) propose *Global* / S / M / L. Une taille choisie s'écrit dans le fichier pipeline
   (`grid_size: S|M|L`, clé de premier niveau) : c'est ce qui répond à l'objection d'ADR-0072, les
   `waypoints` voyagent avec le pas sur lequel ils ont été tracés, et deux instances rendent le
   même YAML de la même façon.
3. **Un défaut global, par navigateur.** Settings › General › Interface › *Wiring grid size* règle la
   taille des pipelines qui n'en ont pas. Rangé à côté du *feedback* de la grille, avec la même
   nature : préférence locale, enregistrée au changement (`pdo.ui.wiringGridSize`), hors
   `instance_config`.
4. **`grid_size` est du layout, pas de la sémantique**, comme `notes` et le routage des edges :
   persisté dans le fichier, exclu du diff sémantique et du hash de contenu de la bibliothèque, des
   deux côtés (`layoutFields.ts`, `pipeline_semantics.rs`). Absent ⇒ le pipeline suit le défaut
   global ; il n'est écrit que sur choix explicite, donc un pipeline non touché se rouvre et se
   sauve à l'identique.
5. **Tout le câblage lit le pas en vigueur** : snap du tracé progressif, drag de segment, jambe
   perpendiculaire (`landingLeg`), ancre de départ (`anchorFromPoint`), snap du tracé auto,
   overlay de la grille pendant le geste. La grille décorative du fond prend le même pas, pour
   qu'un pas de 30px ne soit pas déphasé d'un fond à 20px.

Écarté :

- **Défaut global côté daemon** (`instance_config`). Il ne serait partagé qu'entre navigateurs
  d'une même instance, pas entre instances qui partagent un fichier ; il ne règle donc pas le cas
  qu'ADR-0072 craignait. Le vrai remède est déjà la décision 2.
- **Figer le pas dans le fichier au premier tracé.** Sans choix explicite, le pipeline aurait
  cessé de suivre le réglage global en silence, ce qui contredit le sens même d'un défaut global.

## Conséquences

- Un pipeline **sans** `grid_size` se rend avec le défaut de celui qui le lit : deux lecteurs aux
  défauts différents voient des jambes de longueurs différentes. C'est le prix d'un défaut global ;
  qui partage un pipeline et veut un rendu identique partout lui choisit une taille.
- Changer de taille reste une migration de rendu, pas de données (déjà acté par ADR-0072) : les
  `waypoints` existants ne sont **jamais re-snappés**. Les pipelines tracés sur 40px avant #877
  passent à 30px par défaut ; leurs points restent où ils sont et ne se réalignent que quand on les
  touche.
- Le plancher de jambe (16px) reste sous la plus petite taille (20px).
