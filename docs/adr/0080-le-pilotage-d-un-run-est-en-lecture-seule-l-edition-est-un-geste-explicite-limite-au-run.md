# Le pilotage d'un Run est en lecture seule ; l'édition est un geste explicite, limitée au Run par défaut

> Statut : accepted (grilling des retours du 2026-10-05). **Amende ADR-0007** : retire « pas de mode
> Edit » et l'auto-sync montant ; ses invariants de cohérence runtime (a)-(e) restent en vigueur.
> Vocabulaire : CONTEXT.md « Édition pendant un Run ».

**Suivre un Run ne modifie jamais rien. Modifier se fait dans un mode d'édition explicite, et une
modification faite pour un Run ne vaut que pour ce Run, sauf geste explicite qui écrase le Pipeline
partagé après avertissement.** ADR-0007 reposait sur « PDO est mono-user local, aucun collègue à
surprendre ». Sur une instance partagée, cette prémisse est fausse : une équipe a constaté qu'un
prompt retouché dans son Run modifiait le Pipeline de tous ses collègues (« ça save pour tout le
monde »), et que les gestes d'édition mêlés au suivi (déplacer un nœud, tirer une edge, les onglets
Run/Edit de l'inspecteur) faisaient modifier un Pipeline par erreur. La consigne de contournement
était « ne modifiez pas de nœud », ce qui tue le hot-rerouting que voulait ADR-0007.

- **Pilotage** (défaut d'un onglet de Run) : canvas verrouillé, aucun geste d'authoring ; la
  configuration d'un nœud (prompt, skills) reste lisible en lecture seule.
- **Édition pour ce Run** : geste explicite qui rouvre l'édition à chaud d'ADR-0007 sur le
  snapshot run-scope. L'enregistrement par défaut n'écrit **que** le snapshot.
- **Écraser le Pipeline partagé** : geste distinct, précédé d'un avertissement qui dit qui sera
  touché (tous les futurs Runs, les Triggers qui le référencent) et si le Pipeline a changé depuis
  le lancement du Run. Il remplace la définition partagée par le snapshot entier.
- **Éditer le Pipeline source** : renvoie à l'onglet du Pipeline, dont l'édition directe ne change
  pas.

## Considered Options

- **Garder l'auto-sync et ajouter un « save pour ce Run seulement »** : rejeté, le défaut reste le
  geste dangereux, et c'est le défaut qui a surpris l'équipe.
- **Supprimer toute édition depuis un Run** : rejeté, le rerouting à chaud d'un Run (*Deliberate
  over autonomous*) reste un usage voulu ; il devient seulement délibéré.
