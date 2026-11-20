# Un Node partage le worktree du Run par défaut ; l'isolation est un opt-in

> Statut : accepted. Vocabulaire : CONTEXT.md § *Isolation de Node*.
> Amende ADR-0060 (l'isolation est un choix du Node).

## Contexte

ADR-0060 fait de l'isolation un choix écrit sur le Node, avec un défaut par type : un `agent` est
isolé dans un sous-worktree, un `script` partage le worktree du Run. En pratique, l'isolation par
Run suffit le plus souvent : un Pipeline séquentiel n'a pas besoin d'un fork par rôle. Chaque
sous-worktree coûte pourtant une copie du dépôt (disque, temps, répertoires de build comme
`target/`), et l'auteur doit penser à décocher l'isolation sur chaque nouveau Node `agent`.

Le défaut était en outre recopié à plusieurs endroits : la méthode de défaut du type de Node, le
tampon de la normalisation au parse (qui recopiait les valeurs), et la table de défauts du front.

## Décision

1. **Tout Node `agent` ou `script` partage le worktree de son Run par défaut.** L'isolation devient
   un choix explicite de l'auteur, à cocher pour un Node qui tourne en parallèle d'un autre ou dont
   l'attribution des commits doit être fiable. Les types structurels ne portent toujours aucune
   isolation.
2. **Une seule source du défaut côté daemon** : `NodeType::default_isolation`. La normalisation du
   parse l'appelle pour écrire la ligne d'un Node silencieux, la bibliothèque de Nodes l'appelle
   pour une entrée silencieuse, et un `NodeDef` construit en code s'y replie. La table de défauts du
   front en est le miroir, avec la même valeur.
3. **L'éditeur écrit la ligne.** Un Node ajouté depuis le canvas naît avec
   `isolated_worktree: false`, et la sérialisation écrit toujours le champ, même égal au défaut
   (règle d'ADR-0060 inchangée).
4. **Pas de migration.** Un Document de pipeline ou une entrée de bibliothèque silencieux prend le
   nouveau défaut à sa prochaine lecture. Un NodeRun déjà démarré garde l'isolation figée sur son
   `NodeStarted`.

## Ce qui reste vrai d'ADR-0060

- Le Document de pipeline écrit toujours le choix pour un `agent` ou un `script`.
- L'isolation reste le moyen du parallélisme sans arbre partagé et de l'attribution fiable : en
  partagé, le premier Node qui termine commite tous les changements alors présents, sous son nom.
- La livraison (commit en place, ou merge du sous-worktree) est inchangée.

## Conséquences

- Un Pipeline dont des Nodes `agent` silencieux tournent en parallèle et comptaient sur l'ancien
  défaut partage désormais un même arbre. Le CHANGELOG le signale (note « Upgrading ») : l'auteur
  ajoute `isolated_worktree: true` là où il en a besoin.
- Un `merge` retiré (ADR-0082), lu comme un `agent` inconnu, devient un `agent` partagé.
- L'import d'un workflow étranger continue d'écrire `isolated_worktree: true` sur chaque rôle : ce
  n'est pas un défaut, c'est une ligne écrite.

## Alternative écartée

- **Migrer les YAML silencieux** en y écrivant `isolated_worktree: true` pour garder leur sens :
  rejetée. Le changement de sens est accepté et annoncé, et un migrateur figerait l'ancien défaut
  dans des Documents dont l'auteur n'a jamais choisi l'isolation.
