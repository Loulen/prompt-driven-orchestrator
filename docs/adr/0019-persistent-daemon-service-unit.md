# Service unit persistant pour le daemon (systemd `--user` / launchd)

Sans cette ADR, on laisse le daemon en `pdo daemon` best-effort (ADR-0012) : fermer son laptop ou
rebooter arrête silencieusement toute autonomie planifiée — la différence entre « ça tourne tant que
tu es loggé » et un orchestrateur autonome.

**Décision : rendre le daemon installable comme service OS persistant via `pdo service
{install|uninstall|status}`.** systemd `--user` est le chemin **first-class** (Linux, testé) ; le
LaunchAgent launchd est **best-effort** (macOS, génération testée, chemin réel non testé). Infra
purement **additive** — aucun couplage au scheduler, à la projection ou au runtime.

## Ce qu'on décide

- **Trois lignes de l'unité sont load-bearing**, portées byte-fidèlement depuis l'unité prod éprouvée :
  - `KillMode=process` — le défaut `control-group` SIGKILL-erait tout le cgroup, donc le **serveur
    tmux enfant** (qui tient toutes les sessions live) mourrait ; `process` le laisse ré-adoptable
    (cohérent avec « tuer par nom de session, jamais par pid »).
  - `Environment=PATH=…` — le daemon shelle vers `claude`/`node`/`git`/`tmux` ; sous l'env minimal
    d'une unité, un PATH nu casse **silencieusement** les spawns. (Analogue macOS :
    `AbandonProcessGroup=true` + PATH explicite.)
  - `WorkingDirectory=` — le daemon dérive sa racine du cwd ; sans lui il tournerait depuis `/`.
    Depuis ADR-0033 cette racine est celle du **stockage**, **plus jamais** le dépôt qu'un Run mute
    (champ requis de chaque Run). Sans ce qualificatif, la phrase décrit la panne du 2026-07-29 :
    deux Runs avaient écrit du code dans `~/.pdo/app` faute de dépôt nommé.
- **Le vrai `systemctl` ne tourne jamais dans la suite automatisée** (ADR-0004 : l'adversité-hôte
  reste hors suite). Effets de bord injectables + `--dry-run` ; le `systemctl enable` réel reste une
  étape opérateur manuelle.
- **Garde de conflit de port** (remplace l'item « lazy-start » du scope original, qui reposait sur un
  mécanisme inexistant). `install` sonde le port : **libre** → `enable --now` ; **un daemon PDO
  répond déjà** → on écrit + `enable` pour le boot **sans** `--now` ; **process étranger / daemon
  nu** → **refus loud** (l'unité crash-looperait sinon sur `EADDRINUSE`).
- **Signal UI plié dans `GET /sessions`, pas une nouvelle route.** Un champ `service`
  (`{ supervisor, persistent }`) **calculé une fois au boot et caché** — zéro subprocess par poll
  (même décision que pour le champ `version`). `persistent` dégrade en `null`, **jamais** une erreur.
  La status-bar affiche une pastille ambre `ephemeral` seulement quand il vaut `false` : le seul
  signal que le dot de connexion ne peut structurellement pas exprimer — **joignable ≠ persistant**.
- **`enable-linger` sans sudo** sur le chemin heureux ; sur box durcie, on **catch et affiche**
  `sudo loginctl enable-linger $USER` plutôt que d'exiger sudo.

## Alternatives écartées

- **Flags d'install sur `pdo daemon`** — en ferait un variant à deux sens incompatibles (bloquer pour
  toujours vs one-shot).
- **Route dédiée `GET /service/status`** — même raison que `GET /version` : un champ near-static et
  cacheable suffit.
- **LaunchDaemon root (macOS headless vrai)** — sudo, keychain, env explicite : **différé,
  human-ratified**, pas auto-shippé non testé.

## Limites acceptées

- Chemin launchd réel non testé ; pas d'équivalent linger pour un LaunchAgent — **ne tourne pas
  déloggé**.
- La valeur `persistent` cachée peut être **stale** si on installe le service pendant qu'un daemon
  non-service tourne déjà (reflétée au prochain restart).
- Le bind réseau du daemon reste inchangé (durcissement = #260).

## Relations

Résout la limitation v1 d'**ADR-0012** (Triggers best-effort → persistants). Hérite d'**ADR-0004**.
Interagit avec **ADR-0015** (les réglages posés en prod via l'`Environment=` de l'unité). Ne
supersede aucun ADR.
