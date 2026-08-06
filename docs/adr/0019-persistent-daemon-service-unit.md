# Service unit persistant pour le daemon (systemd `--user` / launchd)

ADR-0012 a laissé le daemon **best-effort** : les Triggers ne firent que tant que le
process `pdo daemon` vit — fermer son laptop ou rebooter arrête silencieusement toute
autonomie planifiée. C'est la différence entre « ça tourne tant que tu es loggé » et un
orchestrateur autonome fiable — le cœur du récit d'autonomie du produit. Une recette
d'install prod **qui marchait déjà** existait dans le `Makefile` : linger +
`KillMode=process` pour garder tmux vivant à travers un restart.

**Décision : rendre le daemon installable comme service OS persistant via un
sous-commande `pdo service {install|uninstall|status}`.** systemd `--user` est le chemin
**first-class** (Linux, testé) ; un LaunchAgent launchd est **best-effort** (macOS,
génération testée, chemin réel non testé en CI Linux). Infra purement **additive** —
aucun couplage au scheduler, à la projection ou au runtime.

## Ce qu'on décide

- **Trois lignes de l'unité sont load-bearing**, portées byte-fidèlement depuis l'unité
  prod éprouvée :
  - `KillMode=process` — le défaut systemd (`control-group`) SIGKILL-erait tout le cgroup
    à l'arrêt/restart, donc le **serveur tmux enfant** (qui tient toutes les sessions
    Claude live) mourrait ; `process` ne tue que le daemon, laissant tmux ré-adoptable
    (cohérent avec la règle tmux « tuer par nom de session, jamais par pid »).
  - `Environment=PATH=…` — le daemon shelle vers `claude`/`node`/`git`/`tmux` ; sous
    l'env minimal qu'une unité reçoit, un PATH nu casse **silencieusement** les spawns.
    (Analogue macOS : `AbandonProcessGroup=true` + PATH explicite.)
  - `WorkingDirectory=` — le daemon dérive sa racine du cwd ; une unité sans lui
    tournerait depuis `/` et résoudrait le mauvais dépôt. Depuis ADR-0033, cette racine
    est celle du **stockage** (pipelines, bibliothèque, prompts, base), **plus jamais** le
    dépôt qu'un Run mute — celui-là est un champ requis de chaque Run. Sans ce
    qualificatif, la phrase décrit exactement la panne reproduite le 2026-07-29 : deux
    Runs avaient écrit du code dans `~/.pdo/app` parce que personne n'avait nommé de
    dépôt.
- **Le vrai `systemctl` ne tourne jamais dans la suite automatisée** (ADR-0004 :
  l'adversité-hôte reste hors de la suite). Effets de bord injectables, plus un
  `--dry-run` qui imprime l'unité et le plan de commandes sans effet de bord ; le
  `systemctl enable` réel — mutateur d'hôte — reste une étape opérateur manuelle unique,
  jamais la CI.
- **Garde de conflit de port** (remplace l'item « lazy-start » du scope original). Deux
  daemons ne peuvent jamais partager un port (le bind est fatal sur `EADDRINUSE`), et il
  n'existe **aucun** auto-spawn/lazy-start dans le code — le scope original reposait sur
  un mécanisme inexistant. À la place, `install` sonde le port : **libre** →
  `enable --now` ; **un daemon PDO répond déjà** → idempotent (on écrit + `enable` l'unité
  pour le boot **sans** `--now`, pas de compétiteur — l'analogue honnête de « connecte au
  lieu de spawner ») ; **process étranger / daemon nu** → **refus loud** (l'unité
  crash-looperait sinon sur `EADDRINUSE`).
- **Signal UI plié dans `GET /sessions`, pas une nouvelle route.** Un champ `service`
  (`{ supervisor, persistent }`) **calculé une fois au boot et caché** — zéro coût
  subprocess par poll, aucune route de plus (même décision maison que pour le champ
  `version`, CONTEXT.md *Versioning*). `persistent` dégrade en `null`, **jamais** une
  erreur. La status-bar reste silencieuse quand `persistent` vaut `true`/`null`, et
  affiche une pastille ambre `ephemeral` quand il vaut `false` — le seul signal que le dot
  de connexion (joignabilité) ne peut structurellement pas exprimer : **joignable ≠
  persistant**.
- **`enable-linger` sans sudo** sur le chemin heureux ; sur box durcie/headless, on
  **catch et affiche** `sudo loginctl enable-linger $USER` plutôt que d'exiger sudo.

## Alternatives écartées

- **Flags d'install sur `pdo daemon`** — en ferait un variant à deux sens incompatibles
  (bloquer pour toujours vs one-shot) ; un sous-commande dédié est net.
- **Route dédiée `GET /service/status`** — écartée pour la même raison que `GET /version` :
  un champ near-static et cacheable sur `/sessions` suffit, sans coût hot-path malgré le
  poll.
- **LaunchDaemon root (macOS headless vrai)** — sudo, keychain, env explicite — **différé,
  human-ratified**, pas auto-shippé non testé.
- **Construire le lazy-start** — le mécanisme n'existe pas et le port-guard couvre le vrai
  hazard.

## Limites acceptées

- Chemin launchd réel non testé en CI ; pas d'équivalent linger pour un LaunchAgent —
  **ne tourne pas déloggé** (headless macOS vrai différé).
- La valeur `persistent` cachée peut être **stale** si on installe le service pendant
  qu'un daemon non-service tourne déjà (reflétée au prochain restart) ; le flux normal est
  install-puis-run.
- Le bind réseau du daemon reste inchangé (durcissement = #260) ; l'unité ne modifie pas
  le comportement de bind.

## Relations

Résout la limitation v1 d'**ADR-0012** (Triggers best-effort → persistants). Hérite
d'**ADR-0004** (adversité-hôte hors suite). Interagit avec **ADR-0015** (les réglages
posés en prod via l'`Environment=` de l'unité). Ne supersede aucun ADR.
