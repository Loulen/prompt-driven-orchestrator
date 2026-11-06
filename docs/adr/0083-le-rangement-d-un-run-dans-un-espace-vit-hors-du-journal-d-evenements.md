# Le rangement d'un Run dans un Espace vit hors du journal d'événements

> Statut : proposed (grilling du 2026-11-06). Vocabulaire : CONTEXT.md « Espace ».

**Les Espaces et l'appartenance d'un Run à un Espace sont stockés dans des tables SQLite dédiées,
à côté du journal d'événements, et jamais comme événement du Run.** Ranger un Run est une
préférence d'affichage d'une instance partagée, pas un fait de son exécution : le rejouer avec le
Run n'apporte rien, et le déplacer d'un Espace à l'autre ne doit pas allonger son historique. C'est
le même choix que pour les Projets, dont l'appartenance des dépôts vit déjà hors du journal.

## Considered Options

- **Un événement `RunSpaceChanged` dans le journal** : rejeté. Il serait rejoué avec le Run, mais un
  binaire qui ne connaît pas ce type relit tout type inconnu comme `RunStarted` : une instance qui
  repasse à une version antérieure corromprait la projection de chaque Run déplacé. Une table que
  l'ancien binaire ignore ne casse rien.

## Consequences

- Supprimer un Espace ne touche aucun Run : ses Runs redeviennent *Sans espace*.
- Deux tables : `spaces` (nom, clé de couleur) et `run_spaces` (`run_id` en clé primaire, donc au
  plus un Espace par Run). Rien n'est stocké pour un Run enfant : la liste le range sous l'Espace
  de son ancêtre le plus proche qui en a un. L'Espace d'un Trigger est une colonne nullable
  `space_id` de `triggers`, qu'un ancien binaire ignore aussi.
- Ranger un Run n'émet aucun événement, donc rien ne réveille les autres navigateurs : chaque
  mutation diffuse `space_changed` sur le WebSocket, comme `project_changed` pour les Projets.
- Oublier un Run efface aussi son rangement. Revenir au binaire précédent laisse les tables
  intactes ; seuls les Runs créés entre-temps naissent *Sans espace*.
