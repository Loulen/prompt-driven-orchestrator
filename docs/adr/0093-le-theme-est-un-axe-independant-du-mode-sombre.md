# Le Thème est un axe indépendant du Mode sombre

> Statut : accepted (grilling du 2026-10-30, CORE-139). Amende ADR-0091. Vocabulaire :
> CONTEXT.md § *Thème*, *Mode sombre*.

## Contexte

La refonte Acogni Driver a remplacé les couleurs de l'interface précédente. Les utilisateurs
veulent pouvoir revenir à ces couleurs sans perdre la densité ni l'identité de la refonte. Le
réglage existant, « thème » System / Light / Dark, ne porte que la luminosité, et la feuille de
tokens servie aux Extensions (ADR-0091) l'expose par un attribut que des Extensions lisent.

## Décision

1. Deux préférences indépendantes, par navigateur : le **Thème** (Acogni par défaut, Classic),
   qui choisit les couleurs, et le **Mode sombre**, un interrupteur activé par défaut. L'option
   qui suivait le système d'exploitation disparaît ; une préférence existante est migrée sans
   perte.
2. Un Thème ne redéfinit que la couche palette des tokens, en sombre et en clair. Les rôles
   (surfaces, texte, filets, primaire) la lisent : aucun composant ne connaît le Thème. Police,
   Densité, rayons et logo ne changent pas.
3. Le Thème Classic replace les anciennes couleurs sur les tokens actuels (trois fonds, six
   statuts, un accent) : fidèle à l'œil, pas au pixel.
4. Le document porte deux attributs, l'un pour le Thème, l'autre pour le mode. Leurs noms
   suivent ce vocabulaire : l'attribut qui portait la luminosité change de sens, sans alias
   (règle d'ADR-0091). Le core sert aussi, à adresse stable, le script qui pose ces deux
   attributs avant le premier affichage, pour les pages qu'une Extension publie hors de
   l'interface.
5. Seul le core définit des Thèmes ; une Extension les suit par les tokens.

## Alternatives écartées

- **Quatre thèmes énumérés** (Acogni sombre, Acogni clair, Classic sombre, Classic clair) : le
  nombre de combinaisons double à chaque Thème ajouté, et une Extension qui ne veut savoir que
  « sombre ou clair » doit connaître tous les Thèmes.
- **Réintroduire l'ancien jeu de tokens** (six fonds, douze couleurs de statut) pour le Thème
  Classic : deux design systems à maintenir, et des Extensions qui devraient connaître les deux.
- **Thème fourni par une Extension** : aucun besoin connu, et chaque Thème élargit la surface que
  toute UI d'Extension doit supporter. Rouvrable plus tard sans casser cette décision.
