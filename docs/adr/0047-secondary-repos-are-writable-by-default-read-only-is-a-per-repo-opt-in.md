# 47. Les dépôts secondaires sont modifiables par défaut ; read-only devient un opt-in par dépôt

Sans cet ADR, un agent garderait les dépôts secondaires read-only par défaut, en croyant (comme
ADR-0042) que les rendre modifiables rouvrirait le merge-back multi-repo rejeté.

## Statut

Accepté — #565. Révise partiellement ADR-0042 (décisions 2 & 4, différés « écriture / git
in-sandbox »). Étend ADR-0030. Repose sur ADR-0036 (la livraison est le fait du nœud `Ship It`,
jamais du daemon).

## Contexte

Le read-only par défaut inverse l'intention du multi-repo : on déclare plusieurs dépôts justement
pour **en modifier plusieurs, liés, au cours d'un même Run** (changer une API back + adapter le SDK
client ailleurs). Le read-only ne couvre que « lire du contexte ailleurs ».

ADR-0042 tenait que rendre un secondaire modifiable rouvrirait le merge-back multi-repo rejeté.
**Ce raisonnement était faux sur un point décisif** : *PDO ne livre jamais lui-même*. Son merge-back
interne n'est que de la comptabilité intra-Run sur des branches `pdo/*` jetables, hard-supprimées au
`cleanup_run`. La livraison réelle est le fait d'un **nœud agent** — le nœud `Ship It` (ADR-0036) —
dépôt par dépôt, exactement comme pour le primaire.

## Décision

1. **Modifiable par défaut, read-only en opt-in.** `RepoPin` et `TargetRepoInput` portent un flag
   `read_only: bool` (défaut `false`), exposé en case à cocher par dépôt secondaire. Cochée, elle
   rétablit le comportement d'ADR-0042.

2. **Polarité du défaut de désérialisation = `false`.** Un pin historique (sans la clé) se relit
   `read_only = false`. C'est sûr : le seul consommateur comportemental est la garde de complétion,
   qui ne tourne jamais sur un Run terminal ; le seul cas de bascule est un Run **vivant qui traverse
   une mise à jour du daemon**, où « plus permissif » est bénin (l'agent doit *activement* écrire
   puis livrer).

3. **La garde `secondary_repo_dirtied` devient conditionnelle.** Elle saute les pins
   `read_only == false`. C'est l'unique mécanisme de refus ; le gater là suffit à autoriser
   l'écriture. Le préambule d'un secondaire modifiable **cesse** de l'annoncer « read-only » et
   invite au contraire à y écrire/committer/livrer.

4. **git in-sandbox sur un secondaire modifiable = mount `.git` rw à chemin identique.** Le snapshot
   est un worktree détaché dont le store d'objets est `<secondary>/.git` — **hors `repo_root`, donc
   non monté**. Pour un secondaire modifiable (et lui seul), on ajoute un bind
   `-v <secondary>/.git:<secondary>/.git:rw` (chemin host == conteneur, invariant D3 ; `:ro`
   casserait l'index/`git status` — EROFS). Le conteneur tourne déjà en `--user <uid>:<gid>` de
   l'hôte, donc les écritures sont bien possédées.

5. **PDO ne gagne aucune responsabilité de livraison.** Pas de branche pipeline par dépôt, pas de
   `base_sha` par dépôt, pas de boucle de merge-back multi-repo. Un invariant du daemon ne peut pas
   vivre dans un prompt (ADR-0036) — donc PDO **offre la capacité** (écrire + `git` fonctionnel), le
   prompt **décide l'usage**.

## Conséquences

- Aucune migration SQLite : le payload d'event est du JSON schemaless, `target_repos` est un blob
  JSON — `read_only` y voyage tel quel.
- Le mount `.git` est **figé à la création du conteneur** : `docker start` ne réévalue pas les
  mounts. Un secondaire **ajouté modifiable en cours de Run** ne verra son `.git` monté **qu'après
  recréation du conteneur** — cohérent avec la *visibilité au spawn* d'ADR-0042. En mode host, aucun
  problème. Limitation documentée, pas un bug.
- Ordre de teardown inchangé et load-bearing : `docker rm -f` **avant** `git worktree remove` (le
  conteneur bind-monte le dépôt). Un mount `.git` rw vivant ajoute un aléa de busy-mount si cet ordre
  régresse.
- **Blast radius assumé :** un `.git` monté rw expose le store d'objets réel du secondaire à l'agent
  in-sandbox (refs, GC…). C'est le prix de « l'agent livre » : il lui faut de toute façon un accès en
  écriture pour pousser. L'agent est déjà de confiance sur le primaire réel.
- Les changements non committés d'un secondaire modifiable au teardown sont **perdus** (le snapshot
  est prune) : responsabilité de l'agent, comme pour le primaire.

## Alternatives écartées

- **Ne faire que la surface (flag + case, sans lever la garde)** : feature morte.
- **Cloner le secondaire sous `repo_root` (au lieu de monter son `.git`)** : 0 mount neuf et
  isolation du store réel, mais le clone `--local` perd les remotes réels → l'agent ne peut plus
  `gh pr create` contre le vrai dépôt sans recâbler l'origin. Le mount `.git` rw garde les remotes
  gratuitement.
- **`read_only` défaut `true`** : plus fidèle au passé mais contredit « modifiable par défaut » au
  niveau du fil et perd l'astuce byte-identique.
- **Merge-back multi-repo atomique côté PDO** : toujours rejeté (ADR-0042 décision 3) — mais
  désormais **hors-sujet**, puisque la livraison n'est pas l'affaire de PDO.
