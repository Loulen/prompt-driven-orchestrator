# Shell de run — « Open session » (bash ad-hoc dans le worktree pipeline)

> Statut : **accepted** (#316).

Sans cette ADR, on ouvrirait l'inspection post-mortem d'un Run terminal soit via la REPL Claude du
Pipeline Manager (conversationnelle, coûteuse, pas un terminal brut), soit via un spawn de terminal
OS — cassé pour un client distant et en prod headless. **« Open session » ouvre un bash interactif
dans une session tmux dédiée au Run, cwd = le worktree pipeline (encore présent tant que le Run
n'est pas archivé, ADR-0020), attaché par le terminal inline existant (ADR-0005).**

## Ce qu'on décide

1. **Attache = terminal inline, pas spawn OS.** Le texte de triage (« OS terminal spawn for the
   MVP ») est inversé : le pont PTY est déjà session-agnostique, donc l'inline est gratuit ; le
   spawn OS ouvre une fenêtre sur l'hôte du daemon (inutile en LAN, pas de `DISPLAY` en headless).
   Le bouton « détacher » reste l'escape hatch OS.

2. **Endpoint create-if-absent seul**, pas un `run_command` : ouvrir un shell n'émet aucun événement
   et ne change aucune projection — opération de session side-band.

3. **Un seul shell par Run**, nom fixe dérivé du run-id. La cardinalité 1/Run **est** le garde-fou
   de population.

4. **Session persistante, reapée uniquement sur run absent ou archivé — jamais de TTL**, pour ne pas
   perdre un `git bisect` en cours sur un WS coupé. **Le tail est une boucle de respawn, pas un
   `bash -i` nu** : `bash -i` sort sur EOF (Ctrl-D, `exit`, ou l'EOF que le pont PTY pousse à la
   fermeture du WS) et, cette fenêtre étant la seule de la session, sa sortie détruisait toute la
   session — promesse de persistance cassée, reproduite en validation. `claude`/`sleep` survivent
   parce qu'ils ne sortent pas sur EOF ; `bash -i` si. La boucle relance un bash frais dans le
   **même** pane (scrollback conservé).

5. **Exempt du cap d'admission** : même raison que l'exemption Manager (éviter un soft-deadlock où
   des sessions légères 1/Run saturent le budget du travail réel) ; sur un Run terminal, zéro
   session nœud vivante par construction.

6. **Env-wrap obligatoire.** Le tail exporte l'environnement PDO et surtout la désactivation du
   trafic non essentiel de Claude Code : sans elle, un `claude` tapé dans ce shell enregistre une
   worker session concurrente et tue toutes les sessions node/manager vivantes du même compte
   OAuth. Le spawn ignore le seam de test tmux (bash est déterministe).

7. **Éligibilité = Run terminal, non archivé, worktree présent** — vérifiée côté serveur, seule
   source de vérité du chemin de worktree. Les Runs live sont exclus : un edit concurrent dans le
   worktree d'un Run vivant casse un merge en vol.

8. **Interlock resume ↔ shell.** `resume_run` tue le shell best-effort avant de réévaluer le Run :
   des edits non commités laissés dans le worktree cassent le merge du nœud suivant ou la garde
   d'immutabilité doc-only. Refuser en 409 déadlockerait — le shell ne meurt que sur archive, or
   archive n'est atteignable que depuis un état terminal, donc un Run failed inspecté une fois ne
   serait plus jamais résumable.

## Limites acceptées

- Kill-on-resume enlève le writer concurrent, pas les edits déjà posés ; le fix robuste
  (stash/reset au resume) est hors MVP.
- Origin check WebSocket = allowlist explicite, défaut localhost (ADR-0005, #564).
- `exit`/Ctrl-D respawn un shell frais au lieu de fermer : la fermeture réelle passe par
  l'archivage ou le reaper, jamais par le shell lui-même.

## Relations

- **ADR-0005** — pont PTY et terminal inline réutilisés verbatim.
- **ADR-0009** — opération atomique side-effect-light qui ne réentre jamais le scheduler.
- **ADR-0012** — cap global = primitive de sûreté (manager & shell exemptés).
- **ADR-0020** — définit la frontière « non archivé ».
