# Une edge porte plusieurs outputs et reste une seule edge runtime

> Statut : accepted (grilling #840). Vocabulaire : CONTEXT.md § « Edges — structure ».

## Contexte

Quand un node produit plusieurs documents destinés au même node aval (cas réel : les deux ports du design vers l'orchestrateur dans `grill-orchestrate`), le canvas dessinait autant de flèches parallèles que de ports, illisibles et pénibles à router. Trois façons de n'en dessiner qu'une : un regroupement purement visuel de N edges YAML ; un sucre YAML expansé en N edges à la frontière du modèle (comme les régions de boucle, ADR-0011) ; une edge multi-port de plein droit.

## Décisions

**1. Une edge peut porter plusieurs output ports du même node source, et c'est une seule edge pour le runtime.** Elle fire une fois, à la complétion du node source (qui ne peut compléter sans tous ses ports, donc aucun port ne manque jamais), et dépose un input émergent par port sur la cible. Écarté : l'expansion en N edges runtime. Elle rendait le regroupement invisible au daemon et au diff, mais dupliquait la clause `when`, le flag `repeated` et l'état de déclenchement sur des edges dont l'utilisateur ne voit qu'une, avec un rendu runtime (« quelle edge a firé ») qui ne correspondait plus à ce qu'il a dessiné. Écarté : le regroupement visuel de N edges YAML, qui laissait N entrées à éditer et à conditionner séparément pour une seule flèche.

**2. Rétrocompatibilité stricte.** La forme à un port est inchangée et reste celle émise quand l'edge porte un seul port ; la forme à plusieurs ports est additive. Deux edges existantes entre les mêmes nodes restent deux flèches. Le diff sémantique compare l'ensemble des ports portés, pas leur ordre.

**3. `when` désigne le port qu'elle lit.** Une clause porte le nom du port dont elle évalue la frontmatter ; le verdict vaut pour l'edge entière. Écarté : une `when` par port (une edge, plusieurs verdicts, la moitié de la flèche qui fire) et l'interdiction du multi-port sur une edge conditionnelle (perdait le cas nominal d'un `verdict` qui route un rapport et une image ensemble).

## Conséquences

- Le daemon apprend la forme multi-port : parseur, résolution des inputs, évaluation de `when` sur le port désigné, marquage runtime de l'edge. Le scheduler ne change pas de modèle : une edge, un déclenchement.
- Le label d'output se décline en un label par port ; le choix des ports vit dans le panneau de détail de l'edge.
