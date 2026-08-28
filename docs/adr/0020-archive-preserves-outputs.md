# L'archivage préserve les outputs (Blackboard archivé, hors run_dir)

Sans cet ADR, un agent laisserait `cleanup_run` détruire les outputs du Run avec son `run_dir`, ou
tenterait de reconstruire le canvas d'un Run archivé depuis la projection de l'event log — qui a
perdu les champs dont la résolution d'I/O a besoin.

**Décision : à l'archivage, `cleanup_run` copie les sorties du Run vers un store durable *global*
`~/.pdo/runs/<run-id>/` — `artifacts/` (le Blackboard) + `pipeline.yaml` + `pipeline.prompts/` —
*avant* de détruire le worktree repo-local ; les handlers de lecture (`/artifact`, `/nodes/<n>/io`,
`/pipeline`) repointent vers cette copie quand le Run est `archived` ; le canvas se réhydrate en
**lecture seule** via le chemin `/pipeline` existant.**

C'est l'extension aux artefacts complets du motif déjà en place pour le **snapshot de pane**
(persisté hors du sous-worktree pour survivre à son reap, #205). Cela **révise** deux points de
CONTEXT.md (*Cleanup vs archive* : « supprime le dossier des artefacts » ; *Blackboard* : « part au
cleanup ») : seuls la branche `pdo/run-<run-id>`, les worktrees et le `run_dir` repo-local sont
récupérés.

## Ce qu'on décide

- **Store global sous `$HOME`, pas sous le repo.** `~/.pdo/runs/<run-id>/` est **disjoint** du
  `run_dir` que `remove_dir_all` supprime — c'est *exactement* ce qui la fait survivre. Les `run-id`
  sont globalement uniques, donc pas de collision inter-repos même quand le daemon pilote plusieurs
  repos cibles.

- **Copie *avant* la destruction, best-effort.** Le worktree principal est détruit par `git worktree
  remove --force` **avant** le `remove_dir_all` ; la copie doit donc précéder la suppression des
  worktrees, pas seulement le `remove_dir_all`. Un échec de copie `warn!` mais **n'avorte pas**
  l'archivage — le teardown ne doit jamais échouer sur un accessoire.

- **On copie *seulement* les sorties, jamais le checkout.** Pas le worktree complet (~7 Mo, `.git` =
  pointeur qui casserait à la copie), ni les sous-worktrees `nodes/*`, ni les `pane-*.snapshot`.

- **La lecture se branche sur `status == Archived`.** `node_io_resolver::resolve` reste
  **inchangé** : il reçoit le `PipelineDef` parsé depuis le `pipeline.yaml` préservé.

- **`forget_run` est la soupape.** Le « permanent delete » supprime aussi `~/.pdo/runs/<id>`
  (best-effort). Sans ça, les outputs préservés fuient à jamais après un forget.

## Pourquoi préserver le `pipeline.yaml` plutôt que reconstruire depuis la projection

L'event log projette `node_defs` + `edges`, mais `PortBrief`/`EdgeInfo` **droppent** `port_type` et
`repeated` — précisément les champs dont `node_io_resolver::resolve` a besoin (`port_type` choisit la
stratégie de résolution ; `repeated` décide le glob `iter-*`). Reconstruire imposerait une
**migration de schéma d'event log** + un backfill, et donnerait un canvas *dégradé* (perte des
loop-regions, notes, waypoints, prompts).

**Aucune interaction avec ADR-0012** : celle-ci décide *qui* peut initier un effet durable
irréversible ; #315 garde l'archivage humain/pipeline-initié et ne change *que ce qui est retenu*.

## Alternatives écartées

- **Reconstruire le canvas côté front depuis `GET /runs/<id>`** : migration de schéma + perte de
  loops/notes/variables, pour un résultat moins fidèle et *plus* de code front.
- **Rester destructif, remplacer le 404 avalé par un message honnête.** Écartée par le mainteneur
  (#315) : on veut *accéder* aux outputs, pas juste être honnête sur leur absence.

## Limites acceptées

- **Le store global n'est pas récupéré par `cleanup_run`** et croît sans borne — même posture
  différée que l'event log. Une politique de rétention reste **différée** ; `forget_run` est le seul
  reclaim v1.
- **Cas pathologique `effective_repo_root == $HOME` exact** : `run_dir == ~/.pdo/runs/<id>` et la
  copie retomberait *dans* la zone supprimée. Gardé par un skip explicite ; un repo simplement *sous*
  `$HOME` est sain.
- **Si la copie échoue** (pas de `$HOME`), les handlers 404ent pour ce Run archivé : dégradation
  honnête.
- **Le prompt *rendu* par itération n'est pas préservé.** À distinguer de l'inspecteur de prompts
  *template* (`pipeline.prompts/`, bien préservé) : le prompt rendu vit dans le working dir du nœud,
  détruit à l'archivage. Le préserver imposerait de parcourir chaque sous-worktree *avant* la boucle
  de suppression — coût disproportionné ; différé, avec dégradation propre côté UI.

## Relations

- Révise CONTEXT.md §*Cleanup vs archive* et §*Blackboard*.
- Motif frère : snapshot de pane survivant au reap (#205).
- Indépendant d'ADR-0012 ; suit ADR-0004 (test couche ≥ 3 requis).
