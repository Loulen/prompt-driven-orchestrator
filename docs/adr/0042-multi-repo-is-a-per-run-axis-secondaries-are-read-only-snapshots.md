# 42. Le multi-repo est un axe par-Run ; les secondaires sont des snapshots read-only

## Statut

Accepté — #465 slices 1 & 2.

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
5. **La liste de secondaires d'un Run est mutable en cours de Run** (slice 2). `PATCH
   /runs/{id}/repos` (`{ add, remove }`) ajoute/retire des secondaires read-only sur un Run **vivant**
   ; le primaire reste immuable (pas d'alias ⇒ inatteignable par `remove` ; un `add` égal au primaire
   est refusé `secondary_is_primary`). L'édition émet un event **`RunReposEdited`** portant la liste
   active complète **re-gelée** (`Vec<RepoPin>`, SHA/alias déjà résolus), et le réducteur écrase
   `RunState.target_repos` (miroir du bras `RunStarted`). Le contrat est la **visibilité au spawn**
   (*spawn-time visibility*) : une édition affecte les nœuds lancés **après** elle ; les nœuds déjà
   vivants gardent le contexte figé à leur spawn (préambule + `PDO_SECONDARY_REPOS` sont écrits une
   fois au lancement, jamais relus). Un **ajout** matérialise le snapshot à l'édition (visible
   in-conteneur par le mount `repo_root:rw` existant — **0 mount neuf**) ; un **retrait** le retire de
   la projection mais **laisse le snapshot sur disque** (un nœud vivant qui lit encore ce chemin
   absolu reste valide), le démontage physique étant différé au `cleanup_run`. **Garde #221 en
   double** : le handler refuse l'édition d'un Run terminal (`409 run_not_editable`) **et** le
   réducteur `RunReposEdited` est un no-op sur un Run terminal — un event passif ne doit jamais
   « dé-terminaliser » un Run. Refus typés (`RepoEditRefusal`, patron `CompletionRefusal`,
   projection unique). L'édition **n'est pas** dans `transition_guard` (nœud-lifecycle seul).

## Conséquences

- Les nœuds voient les secondaires par **chemin absolu** injecté — au préambule et via l'env
  `PDO_SECONDARY_REPOS` (`alias=abspath`) — parce que les sous-worktrees n'héritent pas des fichiers
  nichés sous le run worktree (`.pdo/` est gitignored). Le chemin est identique host/sandbox
  (invariant D3).
- Le sandbox ne gagne aucun mount en slice 1 (secondaires déjà sous `repo_root`, monté rw à chemin
  identique). Le git in-sandbox sur un secondaire est hors-slice (exigerait de monter le `.git` du
  secondaire ; `:ro` casse `git status`/l'index — EROFS).
- `cleanup_run` démonte chaque secondaire par **`git worktree remove --force` + `git worktree
  prune`** depuis le dépôt secondaire (la registration `--detach` vit hors `repo_root`), sinon
  registration dangling — classe #498. Depuis la slice 2 il est **piloté par le disque** : il balaie
  `<run_dir>/repos/*` et démonte **chaque** snapshot présent (actif, retiré-mais-persistant, ou
  orphelin — créé mais dont l'event n'a pas été appendé sur un crash), en résolvant le dépôt
  propriétaire par le pointeur `.git` du snapshot. Itérer la seule projection raterait les
  retirés-mais-persistants et les orphelins. `cleanup_run` ne prunait rien avant la slice 1.
- L'alias d'un secondaire est désambiguïsé sur collision de basename (deux secondaires de même nom
  ⇒ `<base>`, `<base>-2`, …) : les chemins de snapshot ne doivent jamais collisionner. Sur une
  **édition mid-run**, la désambiguïsation est **seedée depuis le disque** (`<run_dir>/repos/*`) en
  plus des alias actifs, pour qu'un `remove repoB` suivi d'un `add <autre>/repoB` n'essaie pas de
  réutiliser le dossier `repoB` d'un snapshot retiré-mais-persistant.
- Le garde de complétion `secondary_repos_dirtied` itère la liste **active** (`RunState.target_repos`)
  : un secondaire retiré n'est plus dirty-checké — comportement voulu (on l'abandonne).
- Les Triggers portent la liste (colonne `target_repos TEXT`, JSON brut, ALTER gardé PRAGMA),
  forwardée au fire ; le chokepoint de création re-gèle chaque SHA.
- La validation par-secondaire (chemin absolu + git, self-référence, doublon, `rev-parse --verify`,
  alias) est **un seul helper** (`resolve_one_secondary_pin`) partagé par le chokepoint de création
  **et** `patch_run_repos`, pour que les deux surfaces ne divergent pas (classe #509).
- Restent **différés** (slices ultérieures) : l'écriture / MR dans un secondaire (retomberait dans le
  merge-back multi-repo rejeté), le `git` in-sandbox sur un secondaire, et le sélecteur `repo:` par
  nœud.

## Alternatives écartées

- **Repos par-pipeline** : contredit ADR-0033 Alt #3 (« quel dépôt = axe par Run ») ; couplerait la
  structure DAG au ciblage de dépôt.
- **Monter les secondaires live (sans snapshot)** : perd la reproductibilité (le checkout local de
  l'opérateur bougerait sous le Run) et n'isole rien.
- **Merge-back multi-repo dans un même nœud** : incompatible avec le modèle base_sha/commit-tree
  mono-repo (#503/ADR-0036).
