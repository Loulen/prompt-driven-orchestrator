# La télémétrie est poussée en OTLP, `/metrics` en option, sans l'Acteur par défaut

> Statut : accepted (grilling du 2026-10-18, CORE-77). Vocabulaire : CONTEXT.md § *Export de
> télémétrie*.

## Contexte

Une organisation veut agréger les métriques de toutes ses instances de PDO, rattachées à ses
Projets, ses dépôts et git. Ces instances tournent le plus souvent sur des postes individuels,
derrière un NAT ou un VPN.

## Décision

1. **Export OTLP poussé** par l'instance vers un collecteur choisi : métriques et traces, pas de
   logs. C'est la voie principale.
2. **`/metrics` (format Prometheus) en option**, pour qui peut joindre l'instance. Chaque voie a
   son interrupteur ; les deux sont éteintes par défaut.
3. Les mesures se rattachent à l'instance, au Projet, au dépôt (son URL distante, sinon le nom du
   dossier, jamais un chemin local), à la branche et aux commits, au pipeline, au harnais et au
   modèle. Les identifiants de Run et de nœud, de cardinalité non bornée, vont sur les traces,
   pas sur les métriques.
4. **L'Acteur n'est pas exporté par défaut.** Un réglage distinct l'ajoute, avec un avertissement :
   mesurer l'activité d'une personne relève, chez un employeur, d'obligations d'information et de
   protection des données que PDO ne peut pas remplir à sa place.

## Alternatives écartées

- **`/metrics` seul** : un serveur Prometheus doit joindre chaque instance, ce que les postes
  individuels ne permettent généralement pas.
- **Exporter l'Acteur par défaut** : transformerait l'activation de la télémétrie en dispositif de
  suivi individuel sans que la personne qui l'active le voie.
