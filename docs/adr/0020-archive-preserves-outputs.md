# L'archivage préserve les outputs (Blackboard archivé, hors run_dir)

Jusqu'ici, `cleanup_run` (« Cleanup » sur un Run terminé) faisait un `remove_dir_all` du
`run_dir` repo-local — worktree pipeline, sous-worktrees **et** `worktree/.pdo/artifacts`
(le Blackboard). Seul l'event log SQLite survivait (CONTEXT.md, *Cleanup vs archive*). Cliquer
un Run `archived` ne restaurait donc rien d'exploitable : le canvas 404ait (`pipeline.yaml`
supprimé) et ouvrir un node terminé 404ait aussi (artefacts supprimés). #315 veut qu'un Run
archivé reste **consultable** : voir sa pipeline en lecture seule et rouvrir les outputs de
ses nodes.

**Décision : à l'archivage, `cleanup_run` copie les sorties du Run vers un store durable
*global* `~/.pdo/runs/<run-id>/` — `artifacts/` (le Blackboard) + `pipeline.yaml` +
`pipeline.prompts/` — *avant* de détrure le worktree repo-local ; les handlers de lecture
(`/artifact`, `/nodes/<n>/io`, `/pipeline`) repointent vers cette copie quand le Run est
`archived` ; le canvas se réhydrate en **lecture seule** via le chemin `/pipeline` existant.**

C'est l'extension aux artefacts complets du motif déjà en place pour le **snapshot de pane**
(persisté hors du sous-worktree pour survivre à son reap, CONTEXT.md §*Reap*/#205). La
nouveauté : ici la copie survit non pas au *reap* d'un node mais au *cleanup* du Run entier.
Cela **révise** deux points de CONTEXT.md (*Cleanup vs archive* : « supprime le dossier des
artefacts » ; *Blackboard* : « part au cleanup ») : les outputs ne partent plus au cleanup ;
seuls la branche `pdo/run-<run-id>`, les worktrees et le `run_dir` repo-local sont récupérés.

## Ce qu'on décide

- **Store global sous `$HOME`, pas sous le repo.** La cible `~/.pdo/runs/<run-id>/` est
  résolue via `dirs_next_home()` (lecture de `$HOME`), à côté des stores existants
  `~/.pdo/pipelines` et `~/.pdo/library`. Elle est **disjointe** du `run_dir` que
  `remove_dir_all` supprime (`<effective_repo_root>/.pdo/runs/<id>`) — c'est *exactement*
  ce qui la fait survivre. Les `run-id` sont globalement uniques (timestamp UTC + préfixe
  UUIDv4), donc pas de collision inter-repos, même quand le daemon pilote plusieurs repos
  cibles (`effective_repo_root` = `target_repo`).

- **Copie *avant* la destruction, best-effort.** Le worktree principal est détruit par
  `git worktree remove --force` **avant** le `remove_dir_all` ; la copie doit donc précéder
  la suppression des worktrees, pas seulement le `remove_dir_all`. Un échec de copie (`$HOME`
  absent, I/O) `warn!` mais **n'avorte pas** l'archivage — la téléfonie de teardown ne doit
  jamais échouer sur un accessoire.

- **On copie *seulement* les sorties, jamais le checkout.** `artifacts/` (quelques Ko de
  markdown, plus d'éventuelles images), `pipeline.yaml` et `pipeline.prompts/` (< 10 Ko). On
  **ne copie pas** le worktree complet (~7 Mo de checkout git, `.git` = pointeur qui casserait
  à la copie) ni les sous-worktrees `nodes/*` ni les `pane-*.snapshot`.

- **La lecture se branche sur `status == Archived`.** Trois handlers (`artifact`, `node_io`,
  `get_run_pipeline`) résolvent leur `artifacts_dir`/`yaml_path`/`prompts_dir` vers le store
  durable quand le Run est archivé, vers le worktree vif sinon. `node_io_resolver::resolve`
  reste **inchangé** : il reçoit le `PipelineDef` parsé depuis le `pipeline.yaml` préservé.

- **`forget_run` est la soupape.** Le « permanent delete » (`DELETE /runs/<id>`, autorisé sur
  un Run déjà `archived`) supprime désormais aussi `~/.pdo/runs/<id>` (best-effort). Sans ça,
  les outputs préservés fuient à jamais après un forget.

## Pourquoi

- **Préserver le `pipeline.yaml` plutôt que reconstruire depuis la projection.** L'event log
  projette `node_defs` + `edges`, mais `PortBrief`/`EdgeInfo` **droppent** `port_type` et
  `repeated` — précisément les champs load-bearing dont `node_io_resolver::resolve` a besoin
  (le `port_type` choisit la stratégie de résolution ; `repeated` décide le glob `iter-*`).
  Reconstruire imposerait une **migration de schéma d'event log** + un backfill, et donnerait
  malgré tout un canvas *dégradé* (perte des loop-regions, notes, waypoints, prompts). Préserver
  les 3 fichiers fait marcher les 3 endpoints *et* le canvas *et* l'inspecteur de prompts sans
  toucher au front (le canvas rend déjà depuis `tab.pipeline`, pas depuis la projection).

- **Aucune interaction avec ADR-0012.** ADR-0012 décide *qui* peut initier un effet durable
  irréversible (le runtime jamais ; l'humain/pipeline oui). #315 garde l'archivage
  humain/pipeline-initié et ne change *que ce qui est retenu*. Amender ADR-0012 serait une
  erreur de catégorie.

## Alternatives écartées

- **Reconstruire le canvas côté front depuis `GET /runs/<id>` (node_defs/edges).** Écartée :
  migration de schéma + mapper `NodeDefInfo→NodeDef` + perte de loops/notes/variables, pour un
  résultat moins fidèle et *plus* de code front.

- **Direction 2 du triage (rester destructif, remplacer le 404 avalé par un message honnête
  « outputs supprimés pour récupérer le disque »).** Écartée par le mainteneur (#315,
  2026-07-04) : on veut *accéder* aux outputs, pas juste être honnête sur leur absence.

## Limites acceptées

- **Le store global n'est pas récupéré par `cleanup_run`** (repo-local) et croît sans borne —
  même posture différée que l'event log (CONTEXT.md : « peut grossir indéfiniment ; on évalue
  avant une politique de purge »). Un *permanent delete* / politique de rétention (TTL, purge
  des archived-jamais-forgotten) reste **différé**, à traiter avec la purge de l'event log.
  `forget_run` est le seul reclaim v1.

- **Cas pathologique `effective_repo_root == $HOME` exact** (le repo cible *est* le home) :
  `run_dir == ~/.pdo/runs/<id>` et la copie retomberait *dans* la zone supprimée. Gardé par
  un skip explicite ; un repo simplement *sous* `$HOME` (`~/projects/foo`) est sain.

- **Si la copie échoue** (pas de `$HOME`), les handlers 404ent pour ce Run archivé : dégradation
  honnête, identique au comportement actuel (le canvas montre le placeholder).

- **Le prompt *rendu* par itération n'est pas préservé.** À distinguer de l'inspecteur de
  prompts *template* (`pipeline.prompts/`, servi par `/pipeline` — bien préservé) : le prompt
  rendu (inputs substitués) que sert `/nodes/<n>/prompt` vit dans le working dir du node
  (`.../worktree` ou sous-worktree `nodes/<n>/iter-N`), détruit à l'archivage, et **hors** du
  set préservé. La section « Initial Prompt » du `NodeDetailPanel` dégrade proprement pour un
  Run archivé (pas de fetch 404, message « Prompt not preserved for archived runs. » au lieu
  d'un spinner figé). Le préserver imposerait de parcourir chaque sous-worktree *avant* la
  boucle de suppression — coût disproportionné pour un accessoire ; différé.

## Relations

- Révise CONTEXT.md §*Cleanup vs archive* et §*Blackboard* (« part au cleanup »).
- Motif frère : snapshot de pane survivant au reap (#205).
- Indépendant d'ADR-0012 (autonomie/effets durables) ; suit ADR-0004 (test couche ≥ 3 requis).
- Suit #136 (section « Archived » repliable de la liste de gauche).
