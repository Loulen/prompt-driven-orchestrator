# 45. Les dépôts secondaires sont modifiables par défaut ; read-only devient un opt-in par dépôt

## Statut

Accepté — #565. Révise partiellement ADR-0042 (décisions 2 & 4, différés « écriture / git
in-sandbox »). Étend ADR-0030 (modèle de mounts sandbox). Repose sur ADR-0036 (la livraison est le
fait du nœud `Ship It`, jamais du daemon).

## Contexte

ADR-0042 a rendu les dépôts **secondaires** read-only *par défaut* : snapshot `--detach`, écriture
refusée en `409 secondary_repo_dirtied`, seul le primaire est écrit. Le read-only par défaut inverse
l'intention du multi-repo : on déclare plusieurs dépôts dans une tâche justement pour **en modifier
plusieurs, liés, au cours d'un même Run** (ex. changer une API back + adapter le SDK client dans un
autre dépôt). Le read-only ne couvre que le cas « lire du contexte ailleurs ».

ADR-0042 tenait le raisonnement suivant : rendre un secondaire modifiable rouvrirait le merge-back
multi-repo rejeté (base_sha scalaire, commit-tree 2 parents mono-repo — #489/#490/#503). **Ce
raisonnement était faux sur un point décisif** : *PDO ne livre jamais lui-même*. Le daemon ne fait
aucun `git push` / `git fetch` / `gh`, n'expose aucune commande qui pousse ou merge vers un remote
ou `main`, et son merge-back interne (`commit_and_merge_sub_worktree`) n'est que de la comptabilité
intra-Run sur des branches `pdo/*` jetables, **hard-supprimées** au `cleanup_run` (`git branch -D`).
La livraison réelle est le fait d'un **nœud agent** — le nœud `Ship It` de la bibliothèque
(`code-mutating`, prompt éditable et non suivi par git, ADR-0036) — qui `gh pr create` / `git merge`
depuis son sous-worktree. Rendre un secondaire modifiable **ne touche donc pas** au merge-back
mono-repo : le nœud shippe chaque dépôt indépendamment, exactement comme il le fait pour le primaire.

## Décision

1. **Modifiable par défaut, read-only en opt-in.** `RepoPin` et l'input `TargetRepoInput` portent un
   flag `read_only: bool` (défaut `false`). L'UI l'expose en **case à cocher par dépôt secondaire**,
   décochée par défaut. Cochée, elle rétablit le comportement d'ADR-0042.

2. **Polarité du défaut de désérialisation = `false`.** `#[serde(default)]` + `skip_serializing_if`
   sur `read_only`, si bien qu'un pin historique (sans la clé) se relit `read_only = false`
   (modifiable) et qu'un pin modifiable neuf reste byte-identique sur le fil. C'est sûr : le seul
   consommateur comportemental est la garde de complétion, qui ne tourne jamais sur un Run terminal
   ; le seul cas de bascule est un Run **vivant qui traverse une mise à jour du daemon**, où
   « plus permissif » est bénin (l'agent doit *activement* écrire puis livrer). On assume ce défaut
   parce qu'il colle à l'intention (« modifiable par défaut ») à toutes les couches.

3. **La garde `secondary_repo_dirtied` devient conditionnelle.** `secondary_repos_dirtied_refusal`
   saute (`continue`) les pins `read_only == false`. C'est l'unique mécanisme de refus ; le gater là
   suffit à autoriser l'écriture. Le préambule d'un secondaire modifiable **cesse** de l'annoncer
   « read-only / ne pas modifier » et invite au contraire à y écrire/committer/livrer.

4. **git in-sandbox sur un secondaire modifiable = mount `.git` rw à chemin identique.** Le snapshot
   est un worktree détaché dont le `.git` pointe vers `<secondary>/.git/worktrees/<alias>` et dont le
   store d'objets est `<secondary>/.git` — **hors `repo_root`, donc non monté**. Pour un secondaire
   modifiable (et lui seul), on ajoute un bind `-v <secondary>/.git:<secondary>/.git:rw` (chemin
   host == conteneur, invariant D3 ; `:ro` casserait l'index/`git status` — EROFS). Le conteneur
   tourne déjà en `--user <uid>:<gid>` de l'hôte : les écritures dans le `.git` monté sont donc bien
   possédées. Un secondaire read-only ne gagne aucun mount.

5. **PDO ne gagne aucune responsabilité de livraison.** Pas de branche pipeline par dépôt, pas de
   `base_sha` par dépôt, pas de boucle de merge-back multi-repo. La livraison d'un secondaire est le
   fait de l'agent (prompt `Ship It`, éditable par l'utilisateur). Un invariant du daemon ne peut pas
   vivre dans un prompt (ADR-0036) — donc PDO **offre la capacité** (écrire + `git` fonctionnel), le
   prompt **décide l'usage**.

## Conséquences

- Aucune migration SQLite : le payload d'event est du JSON schemaless, la colonne trigger
  `target_repos` est un blob JSON — `read_only` y voyage tel quel.
- Le mount `.git` est **figé à la création du conteneur** (comme `extra_mounts`/`env`) : `docker
  start` ne réévalue pas les mounts. Un secondaire **ajouté modifiable en cours de Run** (`PATCH
  /runs/{id}/repos`) ne verra son `.git` monté **qu'après recréation du conteneur** — cohérent avec
  la *visibilité au spawn* d'ADR-0042 (une édition n'affecte que ce qui vient après). En mode host,
  aucun problème (le `.git` est déjà accessible). Limitation documentée, pas un bug.
- Ordre de teardown inchangé et load-bearing : `docker rm -f` **avant** `git worktree remove` (le
  conteneur bind-monte le dépôt). Un mount `.git` rw vivant ajoute un aléa de busy-mount si cet ordre
  régresse — à garder tel quel.
- **Blast radius assumé :** un `.git` monté rw expose le store d'objets réel du secondaire à l'agent
  in-sandbox (il peut créer/déplacer des refs, GC, etc.). C'est le prix de « l'agent livre » : il lui
  faut de toute façon un accès en écriture au dépôt pour pousser. L'agent est déjà de confiance sur
  le primaire réel.
- Les changements non committés d'un secondaire modifiable au teardown sont **perdus** (le snapshot
  est prune) : c'est la responsabilité de l'agent de committer + livrer, exactement comme pour le
  primaire.

## Alternatives écartées

- **Ne faire que la surface (flag + case, sans lever la garde) :** feature morte — la case
  n'aurait aucun effet fonctionnel.
- **Cloner le secondaire sous `repo_root` (au lieu de monter son `.git`) :** 0 mount neuf et
  isolation du store réel, mais le clone `--local` perd les remotes réels → l'agent ne peut plus
  `gh pr create` contre le vrai dépôt sans recâbler l'origin. Contredit « l'agent livre ». Le mount
  `.git` rw garde les remotes gratuitement.
- **`read_only` défaut `true` (préserver l'historique à la lettre) :** plus fidèle au passé mais
  contredit « modifiable par défaut » au niveau du fil et perd l'astuce byte-identique ; le seul
  gain (Runs vivants traversant une MAJ) est un cas dégénéré bénin.
- **Merge-back multi-repo atomique côté PDO :** toujours rejeté (ADR-0042 décision 3) — mais
  désormais **hors-sujet**, puisque la livraison n'est pas l'affaire de PDO.
