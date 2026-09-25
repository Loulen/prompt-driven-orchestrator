# Une absorption est une configuration d'instance appliquée à la lecture, jamais réécrite dans l'event log

> Statut : accepted (grilling du 2026-09-23, #888). Vocabulaire : CONTEXT.md « Absorption ».
> **Amende ADR-0029** (la survie « par pipeline » au renommage repose désormais sur l'absorption
> automatique) et **ADR-0065** (pas de normalisation automatique des ids de modèle, mais une
> absorption explicite peut en réunir deux).
> **Amendé par ADR-0078** : absorptions d'efforts et de couples, portée figée à la pose, et
> propagation des absorptions de Modèles aux couples des Nodes.

## Contexte

Stats identifie un Pipeline par le `pipeline_id` gelé dans `RunStarted` : c'est le nom de fichier
du Pipeline dans la bibliothèque, avec repli sur son nom. Depuis #774, renommer un Pipeline renomme
aussi son fichier, donc la clé change. ADR-0029 promettait que « par pipeline » survivait au
renommage, et ce n'est plus vrai : un Pipeline renommé apparaît sur deux lignes. Le même problème
touche les Nodes renommés d'une version à l'autre (`review` → `code-review`) et les ids de modèle
qu'ADR-0065 affiche tels quels. L'opérateur veut réunir ces lignes pour lire une seule série.

## Décision

1. **L'absorption est une configuration d'instance.** Elle est stockée dans `pdo.db`, à côté de la
   configuration d'instance (ADR-0015). Elle n'appartient ni au Pipeline, ni à l'export (ADR-0059).
   Elle n'est pas un réglage de Stats : elle est persistante et ne revient pas à un défaut à
   l'ouverture.
2. **Elle s'applique à la lecture, au moment de l'agrégation.** Une clé d'absorbé est remplacée par
   celle de son absorbant avant que les onglets agrègent leurs données. L'event log n'est jamais
   réécrit. L'interrupteur **Uncombined** n'est donc qu'une lecture sans cette substitution, et
   retirer un membre prend effet immédiatement, historique compris. Tout cache qui garde un
   résultat déjà agrégé met l'empreinte des absorptions dans sa clé.
3. **Un seul niveau, un seul absorbant par ligne.** Absorber un absorbant lui transfère ses
   membres. Une chaîne de renommages A → B → C donne donc C, qui absorbe A et B, sans résolution
   transitive à la lecture.
4. **Le renommage crée l'absorption.** Un renommage qui change l'id enregistre l'absorption
   ancien → nouveau, visible et retirable comme une absorption manuelle.
5. **Explicite ou rien.** On ne normalise toujours rien automatiquement : ni les noms voisins, ni
   les familles de modèles. Une ligne absorbante porte sa marque dans l'interface, et on peut
   toujours la défaire.

## Alternatives écartées

- **Réécrire le `pipeline_id` des événements** : casse l'append-only et le caractère de source de
  vérité unique de l'event log. C'est irréversible et impossible à comparer avant/après.
- **Un id de Pipeline stable, indépendant du nom de fichier** : ça règle le renommage, mais ni la
  suppression suivie d'une recréation, ni l'historique déjà écrit sans id, ni les Nodes, ni les
  modèles.
- **Un groupe nommé sans absorbant** : ça oblige à inventer un libellé. Le besoin réel, une
  migration ancienne → nouvelle version, a déjà une ligne naturelle à qui donner le nom.
- **Un regroupement éphémère dans la bande de filtres** : il faudrait le refaire à chaque ouverture
  (règle des réglages éphémères), alors que c'est un fait durable de l'instance.
