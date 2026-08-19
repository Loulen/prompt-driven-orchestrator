# Shell de run — « Open session » (bash ad-hoc dans le worktree pipeline)

> Statut : **accepted** (#316).

L'inspection post-mortem d'un Run terminal (Completed/Failed/Skipped/Halted) se faisait à l'aveugle :
le worktree pipeline existe encore sur disque tant que le Run n'est pas archivé (cf. ADR-0020), mais
aucun moyen depuis l'app d'ouvrir un vrai shell dedans — lire les fichiers, `git log`/`git diff`,
relancer un test, comprendre pourquoi un merge a échoué. Le Pipeline Manager offre un `bash` complet
mais c'est une REPL Claude Code attachée au Run (conversationnelle, prompt augmenté, coûteuse), pas
un terminal brut. #316 ajoute une action **« Open session »** sur les Runs terminaux non archivés :
un shell bash interactif ad-hoc dans une session tmux dédiée au Run, cwd = le worktree pipeline,
attachable via le terminal inline existant.

## Ce qu'on décide

1. **Attache = terminal inline (ADR-0005), pas spawn OS.** Le texte de triage (« OS terminal spawn
   for the MVP ») est inversé par rapport à ADR-0005, qui décide l'inline comme mécanisme primaire.
   On tranche pour l'inline : le pont PTY est déjà session-agnostique, donc l'inline est gratuit ;
   le spawn OS ouvre une fenêtre sur l'hôte du daemon — inutile pour un client distant (le daemon
   est joignable via le LAN) et cassé en prod headless (pas de `DISPLAY`). Le bouton « détacher »
   du terminal reste l'escape hatch OS.

2. **Endpoint create-if-absent seul** : il garantit l'existence de la session tmux et renvoie son
   nom ; l'attache se fait ensuite par le pont PTY existant. Ce n'est **pas** un `run_command` :
   ouvrir un shell n'émet aucun événement et ne change aucune projection — opération de session
   side-band, comme les attaches de session existantes.

3. **Un seul shell par Run**, nom fixe dérivé du run-id. Un second clic ré-attache le shell
   existant ; l'idempotence est race-free (create-then-verify-on-failure). La cardinalité 1/Run
   **est** le garde-fou de population.

4. **Session persistante (comme le Manager), reapée uniquement sur run absent ou archivé — jamais
   de TTL.** Elle survit à la fermeture du terminal/onglet, pour ne pas perdre un `git bisect` en
   cours sur un WS coupé. **Le tail est une boucle de respawn, pas un `bash -i` nu** : un `bash -i`
   sort sur EOF (Ctrl-D, `exit`, ou le pont PTY qui pousse un EOF à la fermeture du WS), et comme
   cette fenêtre est la seule de la session, sa sortie détruisait toute la session — promesse de
   persistance cassée, reproduite en validation (navigateur + client WS brut). `claude`/`sleep`
   survivent parce qu'ils ne sortent pas sur EOF ; `bash -i` si. La boucle relance un bash frais
   dans le **même** pane (scrollback conservé) ; gardé par un test de régression déterministe.

5. **Exempt du cap d'admission, par construction.** Le shell n'est pas un nœud projeté et n'appelle
   pas la gate — même raison que l'exemption Manager (éviter un soft-deadlock où des sessions
   légères 1/Run saturent le budget du travail réel) ; sur un Run terminal, zéro session nœud
   vivante par construction, donc la charge marginale est d'un bash.

6. **Env-wrap obligatoire.** Le tail exporte l'environnement PDO (le CLI `pdo` marche dans le
   shell) et surtout la désactivation du trafic non essentiel de Claude Code : sans elle, un
   `claude` tapé dans ce shell enregistre une worker session concurrente et tue toutes les sessions
   node/manager vivantes du même compte OAuth. Le spawn ignore le seam de test tmux, comme un nœud
   `script` : bash est déterministe, le seam ne doit pas le clobber.

7. **Éligibilité = Run terminal, non archivé, worktree présent** — vérifiée côté serveur (source de
   vérité : le client n'a pas le chemin du worktree) ; le client exclut explicitement `archived`.

8. **Interlock resume ↔ shell.** `resume_run` tue le shell best-effort avant de réévaluer le Run.
   Sans ça, des edits non commités laissés dans le worktree pipeline cassent le merge du nœud
   suivant ou la garde d'immutabilité doc-only. Refuser en 409 déadlockerait : le shell ne meurt
   que sur archive, or archive n'est atteignable que depuis un état terminal — un Run failed
   inspecté une fois ne serait plus jamais résumable. Kill-on-resume enlève le writer concurrent,
   pas les edits déjà posés (limite acceptée).

## Alternatives écartées

- **Shell = session `claude` conversationnelle** : rejeté, #316 demande un bash brut distinct de la
  REPL Manager.
- **Éligibilité incluant les Runs live** : différé — un edit concurrent dans le worktree d'un Run
  vivant casse un merge en vol. Terminal-only borne le rayon de souffle du MVP.
- **Session éphémère (kill au disconnect WS)** : pire UX (perte de scrollback / commande longue sur
  une coupure) **et** plus de code (détecter le dernier client qui part) que la persistance.
- **Compter le shell dans le cap** : la back-pressure n'a aucun sens pour une requête humaine
  synchrone « donne-moi un terminal », et rouvrirait le soft-deadlock que l'exemption Manager évite.
- **Reap TTL du shell** : un outil interactif ne doit pas être arraché à un utilisateur parti 5 min.

## Limites acceptées

- Kill-on-resume enlève le writer, pas la saleté déjà posée ; le fix robuste (stash/reset au
  resume) est hors MVP.
- Origin check WebSocket = allowlist explicite, défaut localhost et extensible par configuration (ADR-0005, #564) : même contrainte que tous les terminaux inline.
- `exit`/Ctrl-D respawn un shell frais au lieu de fermer : la persistance prime — la fermeture
  réelle passe par l'archivage ou le reaper, jamais par le shell lui-même.

## Relations

- **ADR-0005** — le shell réutilise le pont PTY et le terminal inline verbatim ; l'inline est
  primaire, l'OS-spawn l'escape hatch.
- **ADR-0009** — opération atomique side-effect-light qui ne réentre jamais le scheduler.
- **ADR-0012** — surface d'attache pilotée par l'humain ; le cap global reste la primitive de
  sûreté (manager & shell exemptés).
- **ADR-0020** — définit la frontière « non archivé » : un Run archivé n'a plus de worktree
  shellable ; l'interlock resume est le pendant lifecycle côté shell.
