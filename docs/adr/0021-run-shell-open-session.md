# Shell de run — « Open session » (bash ad-hoc dans le worktree pipeline)

> Statut : **accepted** (#316). Implémenté : endpoint `POST /sessions/{run_id}/shell`, session `pdo-shell-<run-id>` (variant `SessionTail::Shell` + `ParsedSession::Shell`), 4 sites reaper coordonnés, kill `cleanup_run` + interlock `resume_run`, bouton « Open session » + `RunShellModal` (terminal inline).

L'inspection post-mortem d'un Run terminal (Completed/Failed/Skipped/Halted) se fait aujourd'hui à l'aveugle : le worktree pipeline `<repo>/.pdo/runs/<run-id>/worktree/` existe encore sur disque tant que le Run n'est pas archivé (cf. *Reapable run*, ADR-0020), mais l'utilisateur n'a aucun moyen depuis l'app d'ouvrir un vrai shell dedans pour lire les fichiers, faire un `git log`/`git diff`, relancer un test, comprendre pourquoi un merge a échoué. Le Pipeline Manager offre un `bash` complet mais c'est une **REPL Claude Code** attachée au Run (conversationnelle, prompt augmenté, coûteuse) — pas un terminal brut, et son cwd n'est pas nécessairement là où l'utilisateur veut fouiller.

#316 ajoute une action **« Open session »** sur les Runs terminaux non-archivés : elle spawn un **shell bash interactif ad-hoc** (`exec bash -i`) dans une session tmux dédiée `pdo-shell-<run-id>`, cwd = le worktree pipeline du Run, attachable via le terminal inline xterm.js existant.

## Ce qu'on décide

1. **Attache = terminal inline xterm.js (ADR-0005), pas spawn OS.** Le texte de triage de #316 (« Attach = OS terminal spawn for the MVP ») est **inversé** par rapport à ADR-0005, qui décide justement l'inline comme mécanisme primaire et le spawn OS comme escape hatch legacy. On tranche pour l'inline : (a) le pont PTY `WS /sessions/<id>/pty` est déjà **session-agnostique** — il exécute `tmux attach -t <session>` sur le socket privé du daemon pour n'importe quel nom de session, donc l'inline est *gratuit* ; (b) le spawn OS ouvre une fenêtre **sur l'hôte du daemon**, inutile pour un client distant (le daemon peut être joint via le LAN) et **cassé en prod headless** (`systemd --user`, pas de `DISPLAY`). Le bouton « détacher » de `TmuxTerminal` reste l'escape hatch OS, gratuit et inchangé.

2. **Endpoint = `POST /sessions/{run_id}/shell`, create-if-absent SEUL.** Il garantit l'existence de la session tmux et **renvoie son nom** (`{ ok, session, created }`) ; l'attache se fait ensuite par le `WS /sessions/<session>/pty` existant. Ce n'est **pas** un `run_command` kind : ouvrir un shell n'émet **aucun événement** et ne change **aucune projection** — c'est une opération de session side-band, comme `session_attach`/`manager_attach`, qui vivent sous `/sessions/…`. « Mirror manager/attach » n'est donc vrai qu'à moitié : `manager_attach` est *check-or-404-then-OS-spawn* et ne crée jamais (le manager est spawné au démarrage du Run) ; notre endpoint est l'inverse — il crée et n'ouvre pas de fenêtre OS.

3. **Un seul shell par Run**, nommé de façon fixe `pdo-shell-<run-id>` (helper `shell_session_name`, à côté de `manager_session_name`). Create-if-absent : un second clic ré-attache le shell existant. Idempotence race-free = *create-then-verify-on-failure* : `tmux new-session` rejette les doublons (`duplicate session`), donc en cas d'échec on re-teste `session_exists` → si vrai, un POST concurrent a gagné, on renvoie `created:false`. La cardinalité 1/Run **est** le garde-fou de population.

4. **Session persistante (comme le Manager), reapée uniquement sur run absent ou archivé.** Elle survit à la fermeture du terminal/onglet (tmux détache mais ne tue pas), pour ne pas perdre un `git bisect` en cours sur un WS coupé. Cela impose **4 sites reaper coordonnés** : variant `ParsedSession::Shell`, branche `shell-` de `parse_session_name`, arm `Shell` de `sweep_orphans` (miroir de l'arm Manager : reap ssi run absent OU archivé, **jamais de TTL**), branche `__shell__` de la closure de lookup du reaper (sinon → `None` → tuée comme « absente » à chaque balayage). Plus le kill dans `cleanup_run`.

5. **Exempt du cap d'admission, par construction.** Le shell n'est **pas un nœud projeté** (aucun `NodeStarted`) et n'appelle **pas** la gate : il appelle `tmux_session_manager::spawn`-like directement, exactement comme `spawn_manager_session`. `count_live_node_sessions` (qui n'itère que `run.nodes`) ne peut pas le voir. Même raison que l'exemption Manager (éviter un soft-deadlock où des sessions légères 1/Run saturent le budget du travail réel) ; et sur un Run terminal, 0 session nœud vivante par construction (#205), donc la charge marginale est d'un bash.

6. **Env-wrap obligatoire (`wrap_with_env`).** Le tail `exec bash -i` passe par `wrap_with_env` avec le marker `__shell__`, iter 0 — exportant `PDO_*` (le CLI `pdo` marche dans le shell) et surtout `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`. Sans lui, un `claude` tapé par l'utilisateur dans ce shell enregistre une worker session concurrente et **409/SIGKILL toutes les sessions node/manager vivantes** du même compte OAuth (le bug « Tester dies silently »). Le spawn du shell **ignore** `tmux_cmd_override` (comme un node `script` : bash est déterministe → le seam de test ne doit pas le clobber, sinon `sleep 600` au lieu d'un vrai shell).

7. **Éligibilité = prédicat « Reapable run ».** `is_terminal() && status != Archived && worktree_dir_for_run(...).exists()`. Vérifié **côté serveur** dans le handler (source de vérité — le client n'a pas le chemin worktree). Côté client : nouveau helper `isTerminalRun` + exclusion explicite d'`archived` (attention : `!isLiveRun` **inclut** `archived`).

8. **Interlock resume ↔ shell.** `resume_run` (Halted/Failed résumables) tue le shell best-effort **avant** `re_evaluate_after_command`. Sans ça, un shell laissant des edits non-commités dans le worktree pipeline casse le `git merge` (node_done merge le sous-worktree dans la branche du Run, checkout dans ce même worktree) ou la garde d'immutabilité doc-only → re-fail trompeur. **Refuser (409) déadlocke** : le shell ne meurt que sur archive, or archive n'est atteignable que depuis un état terminal → un Run failed inspecté une fois ne pourrait jamais être résumé. Kill-on-resume enlève le writer concurrent (pas les edits déjà posés — limite acceptée).

## Pourquoi

Choisi contre l'alternative *« spawn OS natif comme attache MVP » (le texte de triage)* parce qu'elle contredit ADR-0005, ne marche pas pour un client distant, et est cassée en prod headless — alors que l'inline est déjà payé (pont PTV session-agnostique + composant `TmuxTerminal` réutilisé par 3 sites). L'inline est le *plus petit* changement, pas le plus gros.

Choisi contre *« endpoint qui crée ET attache (mirror manager/attach) »* parce que l'attache est déjà servie par `WS /sessions/<id>/pty` : dupliquer une étape d'attache serveur-side (a) OS-spawn (cassé en distant) ou (b) ré-implémente le pont. Create-only renvoyant le nom = surface minimale.

Choisi contre *« session éphémère (kill au disconnect WS) »* parce que c'est à la fois pire UX (perte de scrollback / commande longue sur une coupure) et **plus** de code (détecter le *dernier* client qui part, multi-client étant gratuit côté tmux) que la persistance (4 arms additifs recopiés du Manager).

Choisi contre *« compter le shell dans le cap »* parce que la back-pressure (409 « cap atteint » ou file `waiting` sans scheduler pour retry) n'a aucun sens pour une requête humaine synchrone « donne-moi un terminal », et rouvrirait le soft-deadlock que l'exemption Manager évite.

## Alternatives écartées

- **Shell = session `claude` conversationnelle** plutôt que `bash -i` : rejeté, #316 demande explicitement un « fresh ad-hoc bash shell » distinct de la REPL Manager. (Le tail est le seul point de bascule ; changer d'avis = trivial.)
- **Éligibilité incluant les Runs live (Running/AwaitingUser/Paused)** : différé. Un edit utilisateur concurrent dans le worktree pipeline d'un Run vivant casse le `git merge` d'un node_done en vol. Terminal-only borne le rayon de souffle (le MVP de #316).
- **Reap TTL du shell** (comme les nodes) : rejeté — un outil interactif ne doit pas être arraché à un utilisateur parti 5 min. Persistance liée à l'existence durable du Run, comme le Manager.

## Limites acceptées

- **Kill-on-resume enlève le writer, pas la saleté déjà posée.** Un utilisateur qui commit/édite puis resume peut toujours faire échouer le merge/garde sur le résidu. Recovery = c'est son propre fait, le Run était terminal ; le fix robuste (`git stash`/reset au resume) est hors MVP.
- **Origin check WebSocket = localhost/127.0.0.1** (ADR-0005) : un browser réellement distant est rejeté 403 sur le PTV. Le shell hérite exactement de la même contrainte que tous les terminaux inline — pas un nouveau trou, un élargissement (app-wide) hors scope #316.
- **Shell exempt = charge tmux non comptée.** Une boucle scriptée contre l'endpoint pourrait empiler des bash ; borné par la cardinalité 1/Run. Pas de soft-cap numérique MVP (defer jusqu'à preuve d'abus).

## Relations

- **ADR-0005** (terminal inline xterm.js) : le shell réutilise `WS /sessions/<id>/pty` + `TmuxTerminal` verbatim ; l'inline est primaire, OS-spawn l'escape hatch.
- **ADR-0009** (trois couches de primitives runtime) : ouvrir un shell est une opération atomique side-effect-light **qui ne réentre jamais le scheduler** — pas un nœud, absent du resolver.
- **ADR-0012** (autonomie gagnée) : le shell est une surface d'attache pilotée par l'humain ; le *runtime* n'initie aucun effet durable en l'ouvrant. Le cap global reste la primitive de sûreté (manager & shell exemptés).
- **ADR-0020** (l'archivage préserve les outputs) : définit la frontière « non-archivé » — un Run archivé n'a plus de worktree (seule la copie durable read-only survit, non-shellable), d'où le gate. L'interlock resume est le pendant lifecycle côté shell.
