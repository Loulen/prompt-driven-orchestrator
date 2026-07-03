# Service unit persistant pour le daemon (systemd `--user` / launchd)

ADR-0012 a laissé le daemon **best-effort** : les Triggers ne firent que tant que le
process `pdo daemon` vit — fermer son laptop ou rebooter arrête silencieusement toute
autonomie planifiée (CONTEXT.md, *Daemon best-effort*, la limitation v1 exacte qu'ADR-0012
signalait, « service persistant systemd/launchd → #156 »). C'est la différence entre « ça
tourne tant que tu es loggé » et un orchestrateur autonome fiable — le cœur du récit
d'autonomie du produit. Une recette d'install prod **qui marche déjà** existe dans le
`Makefile` (`service-install`) : linger + `KillMode=process` pour garder tmux vivant à
travers un restart.

**Décision : rendre le daemon installable comme service OS persistant via un nouveau
sous-commande `pdo service {install|uninstall|status}`.** systemd `--user` est le chemin
**first-class** (Linux, testé) ; un LaunchAgent launchd est **best-effort** (macOS,
génération golden-testée, chemin `launchctl` réel non testé sur la CI Linux). Le travail est
de l'infra purement **additive** — aucun couplage au scheduler, à la projection ou au
runtime.

## Ce qu'on décide

- **CLI : un sous-commande top-level `Service`, pas des flags sur `Daemon`.** Chaque verbe
  existant est *un verbe → une fn `run_*`* ; `Daemon` construit un runtime tokio et bloque
  pour toujours. Surcharger `Daemon` avec des flags d'install en ferait un variant à deux
  sens incompatibles (une exclusion mutuelle runtime que clap ne peut pas exprimer).
  `run_service` est un one-shot bloquant comme `complete`/`fail`/`skip` — **pas de runtime
  tokio**.

- **Génération de l'unité = fonction pure, portage byte-fidèle du `Makefile`.**
  `render_systemd_unit(exe, port, working_dir, path_env) -> String`, sans I/O,
  golden-testée (couche 1, ADR-0004). Trois lignes sont **load-bearing** (assertées dans le
  test golden) :
  - `KillMode=process` — le défaut systemd `control-group` SIGKILL-erait tout le cgroup à
    l'arrêt/restart, donc le **serveur tmux enfant** (qui tient toutes les sessions Claude
    live) mourrait ; `process` ne tue que le pid du daemon, laissant tmux ré-adoptable
    (cohérent avec la règle #234 « `kill-session -t <name>` seulement, jamais `kill <pid>` »).
  - `Environment=PATH=…<dir de node>…` — le daemon shelle vers `claude`/`node`/`git`/`tmux` ;
    sous l'env minimal qu'une unité reçoit, un PATH nu casse **silencieusement** les spawns.
    L'analogue macOS (`AbandonProcessGroup=true`) et `EnvironmentVariables/PATH` jouent le
    même rôle.
  - `WorkingDirectory=` — le daemon dérive `repo_root` du cwd (pas d'un flag) ; une unité
    sans lui tournerait depuis `/` et résoudrait le mauvais repo.

- **Effets de bord injectables et redirigeables ; le vrai `systemctl` ne tourne jamais dans
  la suite automatisée** (ADR-0004, l'adversité-hôte reste hors de la suite). Le runner de
  commandes (`systemctl`/`loginctl`/`launchctl`) et les lookups env/port/fichier passent par
  un trait `ServiceEnv` — réel en prod, **fake enregistreur** en test (assertion de la
  séquence d'argv exacte + des octets du fichier écrit, dans un `TempDir`), à l'image du seam
  `tmux_cmd_override` (#181). Couplé à `--dry-run` (imprime l'unité + le plan de commandes,
  zéro effet de bord), le vrai `systemctl enable` (mutateur d'hôte) devient une **étape
  opérateur manuelle** unique, jamais la CI.

- **Garde de conflit de port** (remplace l'item « lazy-start » de l'issue). Deux daemons ne
  peuvent jamais partager un port (`run_daemon` bind sans `SO_REUSEADDR` ni retry ;
  `EADDRINUSE` est fatal). Il n'existe **aucun** auto-spawn/lazy-start dans le code — le
  scope original reposait sur un mécanisme inexistant. À la place, `install` sonde
  `127.0.0.1:<port>` : **libre** → `enable --now` ; **un daemon PDO répond déjà** →
  idempotent (on écrit + `enable` l'unité pour le boot **sans** `--now`, pas de compétiteur —
  l'analogue honnête de « connecte au lieu de spawner ») ; **process étranger / daemon nu** →
  **refus loud** (l'unité crash-looperait sur `EADDRINUSE` sous `Restart=on-failure`).

- **Signal UI plié dans `GET /sessions`, pas une nouvelle route.** Un champ `service`
  (`{ supervisor, persistent }`) **calculé une fois au boot et caché** dans `AppState`, donc
  zéro coût subprocess par poll et **aucune entrée de plus dans la whitelist du proxy vite**
  — cohérent avec la décision maison pour le champ `version` (CONTEXT.md, *Versioning*).
  `supervisor` = détection best-effort par marqueurs d'env (hint) ; `persistent` =
  `systemctl --user is-enabled pdo.service` (timeout ~1s, dégrade en `null`, **jamais** une
  erreur). La status-bar reste silencieuse quand `persistent` vaut `true`/`null`, et affiche
  une pastille ambre `ephemeral` (même token `text-st-await` que le dot reconnecting) quand
  il vaut `false` — le seul signal que le dot de connexion (joignabilité) ne peut
  structurellement pas exprimer : joignable ≠ persistant. Le seam d'observation
  `PDO_SERVICE_HEALTH` (sibling de `PDO_DEBUG_PANIC_*`, `None` en prod) force l'état pour
  démontrer la branche `ephemeral` sur une box où une unité est déjà enabled.

- **`enable-linger` sans sudo** sur le chemin heureux (le polkit `set-self-linger` est
  `allow_active=yes`) ; sur box durcie/headless, on **catch et affiche** `sudo loginctl
  enable-linger $USER` plutôt que d'exiger sudo.

## Pourquoi

Fidélité à l'unité prod éprouvée (les bits risqués — `KillMode=process`, linger — sont
prouvés, pas inventés) ; décision-docs d'éviter une entrée de proxy de plus (le champ sur
`/sessions`) ; adversité-hôte hors de la suite (ADR-0004 → le vrai `systemctl` = étape
manuelle). Mettre l'état service dans un champ near-static caché satisfait *à la fois* le
coût-zéro-par-poll et le zéro-nouvelle-route.

## Alternatives écartées

- **Flags sur `Daemon`** — un variant à deux sens incompatibles ; `Service` top-level est net.
- **Route dédiée `GET /service/status`** — écartée pour la même raison que `GET /version`
  (CONTEXT.md) : un champ rétro-compatible sur `/sessions`, d'autant qu'il est near-static
  et cacheable → pas de coût hot-path malgré le poll.
- **LaunchDaemon root (macOS headless vrai)** — nécessite `/Library/LaunchDaemons`, clé
  `UserName`, env explicite, caveats keychain, sudo — **différé, human-ratified**, pas
  auto-shippé non testé.
- **Construire le lazy-start** — le mécanisme n'existe pas et le port-guard couvre le vrai
  hazard (D1).

## Limites acceptées

- Chemin launchd réel **non testé sur la CI Linux** (génération golden-testée seulement).
- Pas d'équivalent linger pour un LaunchAgent — **ne tourne pas déloggé** ; headless macOS
  vrai différé (LaunchDaemon root).
- La valeur `persistent` cachée peut être **stale** si on installe le service pendant qu'un
  daemon non-service tourne déjà (il ne reflètera qu'au prochain restart) ; le flux normal
  est install-puis-run.
- Le bind `0.0.0.0` du daemon reste inchangé (durcissement = #260) ; l'unité ne modifie pas
  le comportement de bind.

## Relations

Résout la limitation v1 d'**ADR-0012** (Triggers best-effort → persistant). Hérite du
layering de test d'**ADR-0004** (centre de gravité couche 3 ; adversité-hôte hors suite).
Interagit avec **ADR-0015** (`PDO_SESSION_CAP` posé en prod via l'`Environment=` de l'unité)
et **#234** (`KillMode=process` découple le cycle de vie du daemon de son serveur tmux
enfant). Ne supersede aucun ADR.
