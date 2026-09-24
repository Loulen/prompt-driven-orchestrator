# Une absorption d'efforts ou de couples reste collée aux exécutions de sa ligne

> Statut : accepted (grilling du 2026-09-24, #888, itération 2). Vocabulaire : CONTEXT.md
> « Absorption ». **Amende ADR-0077** : une absorption de Modèles ne se limite plus à l'axe
> « By model », elle s'applique aussi aux couples modèle × effort des Nodes.

## Contexte

Après #892, on pouvait absorber des Pipelines, des Nodes et des Modèles. Le test humain a fait
apparaître deux manques. Sur l'axe « By model », on ne pouvait pas réunir les efforts d'un modèle,
typiquement *not set* et `high`. Dans le détail d'un Node, on ne pouvait pas réunir les couples
modèle × effort, par exemple plusieurs variantes de Fable sous `implementer`. De plus, les
absorptions de Modèles ne s'appliquaient pas aux couples des Nodes : un Node montrait encore
séparés deux modèles déjà réunis sur l'axe.

## Décision

1. **Deux nouvelles absorptions.** L'absorption d'efforts réunit, sur l'axe « By model », des
   efforts d'un seul modèle. L'absorption de couples réunit, dans le détail d'un Node, des couples
   quelconques : ce qu'on sélectionne donne une ligne. Elle est locale à la ligne Node.
2. **La propagation est à sens unique.** Les absorptions de l'axe « By model » (Modèles, efforts)
   s'appliquent aussi aux couples des Nodes. Une absorption de couples ne remonte jamais sur l'axe.
   On applique le global d'abord, puis le local, et les clés d'une absorption de couples passent
   par la résolution globale. Un couple touché par une absorption globale affiche une marque grisée
   qui renvoie vers l'axe, là où l'absorption se défait.
3. **La portée est figée à la pose.** Une absorption d'efforts ou de couples couvre les exécutions
   de sa ligne au moment où on la pose : la clé de la ligne et les membres qu'elle absorbait alors.
   - Si la ligne se fait absorber, l'absorption continue de ne couvrir que ces exécutions, et pas
     celles de son nouvel absorbant.
   - Si la ligne absorbe plus tard un autre modèle ou un autre Node, les exécutions de ce dernier ne
     sont pas couvertes.
   - Si on retire un membre, l'absorption qui couvrait ses exécutions repart avec lui.

   Exemple : sur claude-opus-5 (*not set* 10, high 20), on réunit *not set* dans high. Puis
   claude-opus-5-5 (*not set* 5, high 8) absorbe claude-opus-5. Résultat : high 38, *not set* 5.
   Les exécutions d'opus-5-5 n'avaient rien demandé.

## Alternatives écartées

- **L'absorption suit sa ligne**, comme les absorptions de Nodes quand leur Pipeline est absorbé
  (#892). Elle s'étendrait alors en silence aux exécutions de l'absorbant (5 *not set* d'opus-5-5
  réunis sans geste), ce qui contredit « explicite ou rien ». Le retrait deviendrait aussi
  asymétrique, parce qu'elle resterait sur l'absorbant. On garde ce comportement pour les Nodes de
  #892, qu'on ne change pas.
- **L'absorption devient inerte quand sa ligne disparaît** : une absorption toujours listée dans
  Settings qui ne fait plus rien.
- **Une absorption d'efforts valable pour tous les modèles** (« *not set* → high partout ») :
  *not set* ne veut pas dire la même chose d'un harnais ou d'un modèle à l'autre.
- **Une absorption de modèles locale au Node, efforts réunis par effort** : dans une liste plate de
  couples, la sélection ne dirait plus ce qu'on obtient, et il faudrait deux gestes pour le cas
  courant.
