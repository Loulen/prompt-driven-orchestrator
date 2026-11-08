# Le Coût de l'arbre se somme à la lecture ; le roll-up de Stats compte l'arbre à la date de sa racine

> Statut : accepted (grilling CORE-213). Complète ADR-0058 (total d'un Run) et ADR-0064 (filiation de Runs).

Un Run qui lance des Runs enfants n'affichait que le coût de ses propres sessions : le coût réel d'une orchestration était sous-estimé. Chaque Run enfant a son propre répertoire, donc ses propres transcripts ; le fold attribué d'ADR-0058 ne les voit pas, et Stats › Cost compte chaque enfant sous son propre Pipeline, à sa propre date de démarrage.

1. **Le Coût de l'arbre est une somme, à la lecture, de folds de Run indépendants.** Le total d'un Run (ADR-0058) reste le **Coût du Run parent** et n'absorbe jamais ses descendants. Coût de l'arbre = Coût du Run parent + Coût du Run parent de chaque descendant (récursif, archivés compris ; un descendant oublié n'a plus d'event log et sort de fait). Rien n'est persisté. La règle d'indisponibilité d'ADR-0052/0058 s'applique à l'arbre : un descendant « — » refuse la somme sans effacer les tranches connues, un descendant « † » rend l'arbre borne basse.
2. **Stats › Cost ne change pas par défaut.** Le réglage éphémère « Roll up child runs » range le coût de chaque descendant sous le Pipeline et le Projet de sa **racine**, ventilé « Parent run » / « Child runs » par profondeur, et **recompose la cohorte par racine** : l'arbre entier entre dans la période de démarrage de la racine, et « Runs terminés seulement » juge l'état de la racine. Les totaux de la période peuvent donc différer réglage activé ; l'interface le dit. Sans objet sur l'axe « By model ».

## Considered Options

- **Faire entrer les enfants dans le total du Run** : chaque coût d'enfant serait compté deux fois dans Stats (sous son Pipeline et sous celui du parent), et le fold unique d'ADR-0058 devrait lire les transcripts d'autres Runs.
- **Roll-up à cohorte invariante** (chaque Run reste dans la période de son propre démarrage, seule la ligne de rattachement change) : garantissait qu'activer le réglage ne change aucun total, mais coupe un arbre à cheval sur deux périodes. Écarté : l'opérateur veut lire le coût d'une orchestration comme un tout, sous le Pipeline de sa racine.
- **Rattacher le coût d'un enfant au nœud qui l'a lancé** : brouille la médiane par exécution de ce nœud ; la ventilation par profondeur sépare plus nettement parent et enfants.
