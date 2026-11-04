# L'Apparence est une préférence par Acteur, gardée par le daemon

> Statut : accepted (grilling du 2026-11-03, story « Customize appearance »). Amende ADR-0093
> (point 1, « par navigateur »). Numéro attribué à la fusion vers `main` (ADR-0095).
> Vocabulaire : CONTEXT.md § *Apparence*, *Élément masquable*, *Thème*, *Mode sombre*,
> *Nodes compactes*.

## Contexte

Les préférences d'interface étaient toutes par navigateur (ADR-0093 pour le Thème et le Mode
sombre, la même règle pour Nodes compactes). L'utilisateur retrouvait donc un autre réglage sur
chaque poste, et devait reconfigurer chacun. Il veut retrouver son Apparence partout où il se
connecte.

## Décision

1. Le daemon garde l'**Apparence** de chaque Acteur, dans le contexte des réglages : Thème, Mode
   sombre, Nodes compactes et la table des écarts des Éléments masquables (`élément → visible`).
   `GET` et `PUT /actors/me/appearance` lisent et remplacent celle de l'Acteur de la requête ;
   aucune route n'atteint celle d'un autre. Sans Extension d'identité, tous les appels viennent de
   l'Opérateur local : un seul document pour l'instance.
2. Le document est versionné et sa taille est bornée. Le daemon n'en valide que la **forme**
   (types, version, taille), jamais la liste des éléments ni les noms de Thème : ajouter un
   élément ne demande aucun changement du daemon.
3. Seuls les **écarts aux défauts** sont enregistrés : un défaut changé plus tard atteint ceux qui
   n'y ont pas touché.
4. Le navigateur garde une **copie locale** sous les clés de stockage d'avant. Le script de thème
   (ADR-0093, point 4) pose donc Thème et Mode sombre avant le premier affichage, dans
   l'interface comme sur une page qu'une Extension publie hors d'elle, sans changement. L'interface
   corrige la copie quand le daemon répond. Les autres onglets du navigateur suivent la copie tout
   de suite ; les autres postes relisent le daemon au chargement et au resync.
5. Au premier chargement d'un Acteur sans Apparence enregistrée, les valeurs du navigateur
   deviennent les siennes.
6. L'**UI scale** reste par navigateur : chaque écran garde la taille qui lui convient.

## Alternatives écartées

- **Tout par navigateur** : l'utilisateur reconfigure chaque poste ; c'est ce que la story
  demande de quitter.
- **Tout par Acteur, sans copie locale** : un flash au premier affichage de chaque page, et les
  pages publiées hors de l'interface ne savent plus quel Thème ni quel mode poser avant d'appeler
  le daemon.
- **Pousser chaque changement aux autres postes par le flux d'événements** : un événement à
  filtrer par Acteur pour un gain minime ; le rechargement et le resync suffisent.
