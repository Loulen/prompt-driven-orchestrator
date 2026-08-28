# 42. Le multi-repo est un axe par-Run ; les secondaires sont des snapshots

Sans cet ADR, un agent porterait la liste de dépôts dans le YAML de pipeline, monterait les dépôts
secondaires live plutôt qu'en snapshot, et tenterait un merge-back atomique multi-repo.

## Statut

Accepté — #465 slices 1 & 2. **Partiellement révisé par ADR-0047 (#565)** : les secondaires sont
désormais **modifiables par défaut**, le read-only devient un **opt-in par dépôt**. Les décisions 1,
3 et 5 restent vraies ; les décisions 2 (« read-only ») et 4 (garde inconditionnelle) sont amendées
ci-dessous, et les différés « écriture / git in-sandbox » sont levés par ADR-0047.

## Contexte

Un Run ne ciblait qu'un dépôt (`target_repo`, ADR-0033). #465 demande de travailler dans un contexte
multi-dépôts : lire le code d'autres dépôts pendant qu'on modifie le principal, avec une target
branch par dépôt.

## Décision

1. La liste de dépôts est portée par le **Run**, pas par la pipeline. Elle vit dans l'event log
   (`RunStarted.target_repos`), jamais dans le YAML. `target_repos[0]` = le **primaire** et conserve
   la sémantique de `target_repo` ; il n'est PAS matérialisé comme un `RepoPin`, seuls les `[1..]`
   le sont.
2. Les secondaires sont matérialisés en snapshots `git worktree add --detach <sha>` sous
   `<primaire>/.pdo/runs/<id>/repos/<alias>/`, le SHA étant résolu au démarrage par `git rev-parse
   --verify <base_branch>` **sans fetch**. **~~Read-only.~~ Amendé par ADR-0047 : modifiables par
   défaut** (flag `read_only`, défaut `false`). La mécanique de snapshot est inchangée.
3. Un Run multi-repo n'écrit et ne merge **que** le primaire. Le merge-back atomique multi-repo est
   **rejeté** (base_sha scalaire, commit-tree 2 parents mono-repo — #489/#490/#503).
4. **Amendé par ADR-0047 :** la garde **409 `secondary_repo_dirtied`** ne s'applique qu'aux
   secondaires **read-only**. Là où elle s'applique : fichiers *suivis* seulement (untracked toléré),
   à l'edge de complétion ; refus **non terminal** (revert puis re-complétion passent) ; **pas** dans
   `transition_guard` (pur/IO-free).
5. **La liste de secondaires est mutable en cours de Run** (`PATCH /runs/{id}/repos`). Le primaire
   reste immuable (pas d'alias ⇒ inatteignable par `remove` ; un `add` égal au primaire est refusé).
   L'édition émet un `RunReposEdited` portant la liste active complète **re-gelée** (SHA/alias déjà
   résolus). Le contrat est la **visibilité au spawn** : une édition affecte les nœuds lancés
   **après** elle ; les nœuds vivants gardent le contexte figé à leur spawn (préambule +
   `PDO_SECONDARY_REPOS` écrits une fois, jamais relus). Un **retrait** sort le snapshot de la
   projection mais le **laisse sur disque** (un nœud vivant qui lit encore ce chemin absolu reste
   valide). **Garde #221 en double** : le handler refuse l'édition d'un Run terminal (`409
   run_not_editable`) **et** le réducteur est un no-op sur un Run terminal — un event passif ne doit
   jamais « dé-terminaliser » un Run.

## Conséquences

- Les nœuds voient les secondaires par **chemin absolu** injecté (préambule + env
  `PDO_SECONDARY_REPOS`) parce que les sous-worktrees n'héritent pas des fichiers nichés sous le run
  worktree (`.pdo/` est gitignored). Chemin identique host/sandbox (invariant D3).
- **Amendé par ADR-0047 :** un secondaire **modifiable** gagne un mount `<secondary>/.git` en **rw**
  à chemin identique (le `:ro` casserait `git status`/l'index — EROFS). Un read-only n'en gagne
  aucun.
- `cleanup_run` démonte chaque secondaire par `git worktree remove --force` + `git worktree prune`
  **depuis le dépôt secondaire** (la registration `--detach` vit hors `repo_root`), sinon
  registration dangling. Il est **piloté par le disque** : il balaie `<run_dir>/repos/*` et démonte
  **chaque** snapshot présent (actif, retiré-mais-persistant, ou orphelin d'un crash). Itérer la
  seule projection raterait les deux derniers cas.
- L'alias est désambiguïsé sur collision de basename (`<base>`, `<base>-2`, …). Sur une **édition
  mid-run**, la désambiguïsation est **seedée depuis le disque** en plus des alias actifs, pour qu'un
  `remove repoB` suivi d'un `add <autre>/repoB` ne réutilise pas le dossier d'un snapshot
  retiré-mais-persistant.
- La garde de complétion itère la liste **active** : un secondaire retiré n'est plus dirty-checké —
  voulu.
- La validation par-secondaire est **un seul helper** (`resolve_one_secondary_pin`) partagé par le
  chokepoint de création **et** `patch_run_repos`, pour que les deux surfaces ne divergent pas.
- Les différés « écriture / git in-sandbox » sont **levés par ADR-0047**, sans rouvrir le merge-back
  rejeté en décision 3 : PDO ne livre jamais lui-même — c'est le nœud `Ship It` (ADR-0036) qui ouvre
  la PR, dépôt par dépôt. Reste différé : le sélecteur `repo:` par nœud.

## Alternatives écartées

- **Repos par-pipeline** : contredit ADR-0033 Alt #3 ; couplerait la structure DAG au ciblage de
  dépôt.
- **Monter les secondaires live (sans snapshot)** : perd la reproductibilité (le checkout local de
  l'opérateur bougerait sous le Run).
- **Merge-back multi-repo dans un même nœud** : incompatible avec le modèle base_sha/commit-tree
  mono-repo (#503/ADR-0036).
