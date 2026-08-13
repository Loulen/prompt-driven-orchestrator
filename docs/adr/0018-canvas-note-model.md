# Note de canvas — bloc racine `notes:`, jamais un type de nœud

Le canvas gagne des **notes** : des annotations de documentation inertes que le designer
épingle près d'un groupe de nœuds pour expliquer une intention (#307). L'instinct par
défaut — « c'est un élément du canvas, donc un type de nœud » — vient d'être renforcé par
ADR-0017 (`script` = nouvel arm `NodeType`). Mais une note ne s'exécute jamais, ne spawne
aucune session, ne produit aucun artefact et n'a pas d'état terminal : la traiter comme un
`NodeType` forcerait le scheduler, le spawn, l'admission, les validateurs et la détection
de vie à gérer un arm « qui ne tourne pas », avec le risque qu'une note soit
accidentellement schedulée. Coût mesuré à la décision : `NodeType` était référencé dans
**16 fichiers Rust** (313 occurrences), dont ≥ 3 `match` exhaustifs qui casseraient à la
compilation et une nuée d'arms `_ =>` qui avaleraient silencieusement une note comme un
nœud de DAG.

**Décision : modéliser les notes comme un bloc racine `notes:` du YAML** (sibling de
`loops:`/`edges:`), chaque entrée portant id, contenu et position. Le runtime **ignore
entièrement** ce bloc — il n'entre ni dans l'ordonnancement, ni dans le dataflow, ni dans
le graphe de nœuds.

- **Layout, pas sémantique — le bloc `notes:` entier est exclu du diff.** Comme la
  position d'un nœud ou le tracé d'une edge, une note est persistée dans le fichier (elle
  voyage avec le pipeline partagé) mais **exclue du diff sémantique** : créer / déplacer /
  éditer / supprimer une note ne fait **jamais** bouger l'indicateur synced/diverged (la
  puce « non sauvegardé » s'allume, elle, comme pour tout déplacement). Classification
  « layout intégral » (position **et** contenu hors diff), retenue parce qu'une note est
  de la documentation, pas de la donnée d'orchestration — point de ratification humaine
  réversible : si « documentation = donnée sémantique », on ne strip que la position et
  éditer une note marquerait « diverged ». `content` est du **texte brut** en v1
  (markdown = enhancement ultérieur qui rouvre ADR-0013).
- **Rendu = custom element xyflow** (ADR-0003). Le type de rendu React-Flow n'est **pas**
  un PDO `NodeType` — la distinction est volontaire et load-bearing.

**Pourquoi.** Suit la forme éprouvée du bloc `loops:` (ADR-0011 : entité nommée de premier
niveau, rendue sur le canvas, jamais un nœud) et l'instinct « pas de nouveau type de
nœud » d'ADR-0016. Sort la note du chemin d'exécution **par construction**, au lieu de la
neutraliser par des gardes disséminés qu'un refactor futur oublierait. Divergence assumée
d'avec ADR-0017 : `script` méritait un arm `NodeType` *parce qu'il s'exécute* ; une note
ne s'exécute pas. Le critère n'est pas « est-ce sur le canvas » mais « **est-ce que ça
tourne** ».

**Alternatives écartées.** *`NodeType::note`* — réutilise l'infra de nœud
(render/sélection/drag/undo/persistance), mais impose un arm non-exécutant sur ~16
fichiers et laisse fuir le risque d'un scheduling accidentel : le coût du « nœud qui n'est
pas un nœud » dépasse la réutilisation. *Annotation éphémère non persistée* (ou blob
`meta`) — zéro schéma, mais la note ne voyage pas avec le pipeline partagé ni ne survit au
reload ; or une note *est* de la documentation durable.

**Conséquences.**

- Les notes restent **librement mutables pendant un Run** (inertes, aucune session à
  orphaner) — cohérent avec ADR-0007.
- Couvertes par l'undo COW d'ADR-0014 gratuitement, **à condition** que toute mutation
  réaffecte le tableau (jamais de mutation en place, sinon corruption d'undo silencieuse).
- Décision de schéma difficile à inverser : une fois des pipelines `notes:` dans la
  nature, basculer vers `NodeType::note` exigerait une migration **plus** le travail sur
  les 16 fichiers évité ici.

**Portée v1 (différé).** Contenu markdown/mermaid dans les notes : rouvre ADR-0013
(nouvelle surface de rendu + sink raw-HTML, contenu humain-mais-rendu = nouvelle classe de
confiance) → exigera son propre ADR/amendement. Note redimensionnable : fast-follow. Pas
de note en bibliothèque : une note est pipeline-spécifique, pas un artefact réutilisable.
Imbrication note↔région, couleurs, ancrage : différés.

**Relations.** Suit ADR-0011 (bloc top-level nommé, non-nœud) et ADR-0016 (pas de nouveau
type de nœud). Diverge délibérément d'ADR-0017 (arm `NodeType` réservé à ce qui
s'exécute). Hérite d'ADR-0003 (rendu xyflow), ADR-0014 (undo COW), ADR-0007 (édition
pendant un Run). Protège la frontière d'ADR-0013 en restant texte brut. Ne supersede
aucun ADR.
