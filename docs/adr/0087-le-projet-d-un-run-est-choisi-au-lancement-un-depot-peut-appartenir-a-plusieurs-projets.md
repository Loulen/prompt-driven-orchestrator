# Le Projet d'un Run est choisi au lancement ; un dépôt peut appartenir à plusieurs Projets

> Statut : accepted (grilling du 2026-10-18, CORE-77). Vocabulaire : CONTEXT.md § *Projet*.
> Amende ADR-0081 (les Projets d'une Opération sur un Run).
> Amendé par ADR draft-un-run-dans-le-projet-de-son-depot : une création qui ne nomme pas de
> Projet est lancée dans le Projet qui tient son dépôt ; hors Projet, elle lit le disque.

## Contexte

Un chemin appartenait à au plus un Projet, et le Projet d'un Run se déduisait, à la lecture, du
Projet qui possède son dépôt primaire. Ce modèle tient tant qu'un dépôt ne sert qu'à une équipe.
Il casse dès qu'un dépôt partagé (une bibliothèque commune, un dépôt de documentation) se
travaille dans plusieurs Projets : il faut choisir lequel le « possède », et les Runs des autres
Projets tombent hors de leur Projet, donc hors de leurs Droits et de leurs Pipelines validés.

## Décision

1. **Un dépôt peut être membre de plusieurs Projets.**
2. **Le Projet d'un Run est choisi au lancement et enregistré sur le Run**, jamais recalculé. Le
   lancement part d'un Projet, qui présélectionne son dépôt primaire par défaut et propose ses
   autres dépôts comme secondaires. Un Run lancé hors Projet n'en a pas.
3. Les Opérations sur un Run, ses nœuds, ses sessions et ses commentaires de review portent ce
   Projet enregistré ; un Run enfant hérite du Projet de son parent.
4. **Les Runs existants** reçoivent, une fois, le Projet que l'ancienne règle leur attribuait ;
   l'event log reste la source de vérité et le Résumé de Run porte le Projet.

## Alternatives écartées

- **Garder la déduction et choisir un Projet « principal » par dépôt** : le cas qui motive la
  décision (un dépôt partagé) reste mal rangé pour tous les Projets sauf un.
- **Déduire l'ensemble des Projets du dépôt** (un Run dans tous les Projets de son dépôt) : un Run
  deviendrait visible et modifiable par des équipes qui ne l'ont pas lancé, et une Opération
  porterait plusieurs Projets là où les Droits en attendent un.

## Conséquences

- Changer les membres d'un Projet ne déplace plus aucun Run existant.
- Un Run ne change pas de Projet après son lancement.
- La migration des Runs existants écrit, une fois, la clé `project_id` dans leur `run_started`
  (`null` pour un Run sans Projet) ; un `run_started` qui porte la clé n'est jamais revu. Les
  Triggers existants reçoivent de même le Projet de leur dépôt cible. Un Run créé depuis une
  session de nœud reste dans le Projet de son parent ; un Projet supprimé depuis qu'un Trigger ou
  un Run le nommait laisse leurs nouveaux Runs hors Projet.
