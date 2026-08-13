# 42. Le multi-repo est un axe par-Run ; les secondaires sont des snapshots read-only

## Statut

Accepté — #465 slice 1.

> Numéro posé sur la branche de base (1.12.0), où le max d'ADR sur disque est 0039 ;
> 0040/0041 sont réservés (disputés par #512/#509/#507). À **renuméroter au next-free
> au-dessus de `origin/main`** lors du rebase de finalisation si nécessaire — le code et le
> CHANGELOG référencent `ADR-0042`, à garder cohérents avec le nom de fichier.

## Contexte

Un Run ne ciblait qu'un dépôt (`target_repo`, ADR-0033). #465 demande de travailler dans un
contexte multi-dépôts : lire le code d'autres dépôts pendant qu'on modifie le principal, avec une
target branch par dépôt.

## Décision

1. La liste de dépôts est portée par le **Run**, pas par la pipeline. Elle vit dans l'event log
   (`RunStarted.target_repos`), jamais dans le YAML de pipeline. `target_repos[0]` = le **primaire**
   et conserve la sémantique de `target_repo` (ADR-0033 inchangée) ; il n'est PAS matérialisé comme
   un `RepoPin` (il reste dans `target_repo`), seuls les `[1..]` le sont.
2. Les dépôts **secondaires** sont **read-only** : snapshots `git worktree add --detach <sha>` sous
   `<primaire>/.pdo/runs/<id>/repos/<alias>/`, le SHA étant résolu au démarrage par
   `git rev-parse --verify <base_branch>` **sans fetch** (base = ref locale ; défaut = `HEAD`
   local).
3. Un Run multi-repo n'écrit et ne merge **que** le primaire. Le merge-back atomique multi-repo est
   **rejeté** (rouvrirait #489/#490/#503 : base_sha scalaire, commit-tree 2 parents mono-repo).
4. L'écriture dans un secondaire est refusée en **409 `secondary_repo_dirtied`** (fichiers *suivis*
   seulement ; untracked toléré), via `worktree_has_tracked_changes`, à l'edge de complétion. Le
   refus est **non terminal** (le nœud reste vivant : revert du fichier suivi puis re-complétion
   passent) ; il n'est **pas** dans `transition_guard` (pur/IO-free).

## Conséquences

- Les nœuds voient les secondaires par **chemin absolu** injecté — au préambule et via l'env
  `PDO_SECONDARY_REPOS` (`alias=abspath`) — parce que les sous-worktrees n'héritent pas des fichiers
  nichés sous le run worktree (`.pdo/` est gitignored). Le chemin est identique host/sandbox
  (invariant D3).
- Le sandbox ne gagne aucun mount en slice 1 (secondaires déjà sous `repo_root`, monté rw à chemin
  identique). Le git in-sandbox sur un secondaire est hors-slice (exigerait de monter le `.git` du
  secondaire ; `:ro` casse `git status`/l'index — EROFS).
- `cleanup_run` doit `git worktree remove --force` **+ `git worktree prune`** dans **chaque**
  secondaire, depuis le dépôt secondaire (la registration `--detach` vit hors `repo_root`), sinon
  registration dangling — classe #498. `cleanup_run` ne prunait rien auparavant.
- L'alias d'un secondaire est désambiguïsé sur collision de basename (deux secondaires de même nom
  ⇒ `<base>`, `<base>-2`, …) : les chemins de snapshot ne doivent jamais collisionner.
- Les Triggers portent la liste (colonne `target_repos TEXT`, JSON brut, ALTER gardé PRAGMA),
  forwardée au fire ; le chokepoint de création re-gèle chaque SHA.
- L'édition mid-run de la liste et le sélecteur `repo:` par nœud sont différés.

## Alternatives écartées

- **Repos par-pipeline** : contredit ADR-0033 Alt #3 (« quel dépôt = axe par Run ») ; couplerait la
  structure DAG au ciblage de dépôt.
- **Monter les secondaires live (sans snapshot)** : perd la reproductibilité (le checkout local de
  l'opérateur bougerait sous le Run) et n'isole rien.
- **Merge-back multi-repo dans un même nœud** : incompatible avec le modèle base_sha/commit-tree
  mono-repo (#503/ADR-0036).
