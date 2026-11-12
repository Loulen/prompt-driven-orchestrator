# Un Run est lancé dans le Projet de son dépôt ; hors Projet, il lit le disque

> Statut : accepted. Vocabulaire : CONTEXT.md § *Projet*.
> Amende ADR-0087 (le Projet d'un Run est choisi au lancement).

## Contexte

ADR-0087 enregistre le Projet d'un Run au lancement, sans jamais le déduire de son dépôt : une
création qui ne nomme pas de Projet lance le Run hors Projet, même sur le dépôt d'un Projet. Dès
que les Droits se jugent par Projet, ce Run échappe aux Droits du Projet qui travaille ce dépôt,
et créer un Run hors Projet sur n'importe quel chemin revient à lire le disque de l'instance.

## Décision

1. **Une création qui ne nomme pas de Projet** (geste d'un Acteur : `POST /runs`, `pdo run
   create`, une Extension par son Host) est lancée dans **le** Projet qui tient son dépôt primaire,
   et l'Opération `run` × `create` est jugée dans ce Projet. Le Projet est enregistré sur le Run
   comme s'il avait été nommé.
2. **Un dépôt de plusieurs Projets** est refusé (`400`) quand la création n'en nomme aucun : la
   réponse nomme les Projets que l'Acteur peut lire (les autres comptés) et les liste dans
   `projects` ; la CLI ajoute le `--project` à passer, le formulaire New Run dit la même phrase.
3. **Un Run hors Projet travaille dans un dépôt lu sur le disque** : sa création demande en plus
   `repository` × `read` hors Projet, que l'Extension d'autorisation peut réserver. Refusée, rien
   n'est créé (ni Run, ni branche).
4. **Un Run créé depuis une session de nœud** reste dans le Projet de son parent (ADR-0064,
   ADR-0087) et n'y cible qu'un dépôt membre ; un parent hors Projet ne cible qu'un dépôt hors de
   tout Projet.
5. Un Trigger qui tire et un Run relancé gardent le Projet qu'ils portent : la déduction ne vaut
   que pour le geste de création.

## Alternatives écartées

- **Garder « hors Projet » par défaut** : un Run sur le dépôt d'un Projet échappe aux Droits de
  ce Projet, et un Acteur sans lecture du disque lance quand même un Run sur un chemin quelconque.
- **Choisir le premier Projet du dépôt** quand plusieurs le tiennent : le Projet d'un Run
  dépendrait de l'ordre des adhésions, invisible pour qui lance.

## Conséquences

- **Cassant** : une création sans Projet sur un dépôt de plusieurs Projets, acceptée hors Projet
  avant, est refusée.
- Le formulaire New Run d'un Acteur sans lecture du disque ne prend plus de chemin saisi : il
  cherche dans les dépôts de ses Projets, et un dépôt choisi sous un Projet fixe ce Projet.
- Un Run déjà lancé ne change pas de Projet (ADR-0087).
