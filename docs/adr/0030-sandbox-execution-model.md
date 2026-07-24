# ADR-0030 — Modèle d'exécution de la Sandbox (conteneur par Run)

> Statut : accepted (#407, tracer bullet du PRD #403). Vocabulaire : CONTEXT.md § « Sandbox ».
> Consolide aussi la rationale du tag image content-hashé (#405) — pas d'ADR séparé.

Un Run en mode `copy`/`pure` exécute **toutes ses tails** (nœuds agents, manager, merge-resolver,
nœuds `script`, run-shell) dans un unique conteneur long-vécu `pdo-sbx-<run-id>` (`sleep infinity`,
PID 1 = tini). Les guards de Trigger restent hôte (décision de fiançailles, pas de travail de Run).

## Ce qu'on décide

1. **Identity mounts.** Le repo cible est bind-monté rw à son **chemin absolu hôte** (un seul mount
   couvre repo + tous les worktrees de nœuds sous `.pdo/runs/` + `.pdo/prompts`) ; le *staged Claude
   home* → `$HOME/.claude`, son `.claude.json` sibling → `$HOME/.claude.json`, le binaire `pdo` hôte
   → `/usr/local/bin/pdo:ro`. Le conteneur adopte l'**uid/gid hôte** (`--user` numérique). Résultat :
   le chemin de travail est identique des deux côtés → le dirname encodé des transcripts matche
   (pré-requis du merge-back, câblé en #408).

2. **Staging par Run.** `~/.pdo/sandbox/<run-id>/` (jamais le vrai `~/.claude`), seedé par `prepare`
   selon le mode, purgé par `teardown` au `cleanup_run`. En `pure`, la confiance (`hasTrustDialogAccepted`)
   est pré-accordée à la **racine du repo** — l'ancêtre commun du worktree de pipeline ET de tous les
   worktrees de nœuds, donc héritée par chaque cwd de session.

3. **Réseau = host-gateway + `PDO_DAEMON_URL`.** Le conteneur joint le daemon hôte via
   `--add-host host.docker.internal:host-gateway` + `PDO_DAEMON_URL=http://host.docker.internal:<port>`
   posé **au create** (jamais re-passé à l'exec — un `-e` nu re-forwarderait le `localhost` hôte et
   clobbererait la gateway). C'est ce qui permet au `pdo complete`/`fail`/`skip` in-container de
   rappeler le daemon.

4. **Préparation eager fail-fast.** Image + conteneur + staging sont garantis prêts **avant le premier
   spawn** ; toute indisponibilité de Docker → `RunFailed` explicite. **Jamais de fallback hôte
   silencieux** pour le travail d'un Run sandboxé. La prep tourne sur une tâche détachée (le
   `docker build` du 1er run machine ne doit pas bloquer le 201 — cohérent ADR-0023) ; `ensure_ready`
   étant bloquant, il vit dans un `spawn_blocking` (panic isolée en `JoinError` → `RunFailed`).

5. **Wrapping au chokepoint unique.** Toutes les familles de tails funnel par `build_tmux_script`
   (+ `build_resume_script` pour le `claude --continue`) : quand le Run est sandboxé, la tail est
   enveloppée en `docker exec … pdo-sbx-<run> bash -lc '<tail>'`. Les exports d'env de base restent
   côté **hôte** (inoffensifs) — d'où l'invariant `off` **byte-identique** quand le wrapping est absent.
   Le catalogue d'env **dynamique** d'un nœud `script` (`PDO_ARTIFACTS_DIR`/`PDO_INPUT_*`/`PDO_OUTPUT_*`/
   `PDO_VAR_*`) traverse l'exec en `-e KEY=VALUE` **explicites** (un `-e` nu ne forwarderait que la
   valeur du shell hôte, que la sandbox n'exporte pas) — **jamais `PDO_DAEMON_URL`**.

6. **Kill ciblé.** Un kill de session est doublé d'un `docker exec` séparé qui scanne `/proc/*/environ`
   pour le marqueur de session (`PDO_SBX_SESSION` = le nom de session tmux) et `TERM`→`KILL` le seul
   arbre porteur ; les sessions sœurs survivent (le client `docker exec` tué côté tmux ne tue pas le
   process conteneur, reparenté sur PID 1).

7. **Tag image adressé par contenu + fourniture hybride (#411).** `pdo-sandbox:h-<hash>` où
   `<hash>` = SHA-256[..12] des octets exacts du Dockerfile sur disque. Deux Dockerfiles identiques →
   même tag ; une édition → rebuild. `.gitattributes` épingle `eol=lf` pour la reproductibilité.
   C'est l'identité qui rend une image **tirée d'un registry** et une image **buildée localement**
   interchangeables sous le même nom. Un réglage **par-daemon** `image_source`
   (`registry` défaut | `dockerfile`, précédence `stored → env → default`, ADR-0015) pilote
   `ensure_image` : en `registry`, si l'image n'est pas déjà locale, `docker pull
   ghcr.io/loulen/pdo-sandbox:h-<hash>` puis **retag** sous le ref local, avec **fallback build** si
   le pull échoue (offline / 404 tag absent / registry down) ; en `dockerfile`, build direct, jamais
   de pull. La valeur de retour est **toujours le ref local** → `sandbox_container` inchangé (reçoit
   `pdo-sandbox:h-<hash>` que l'image vienne d'un pull ou d'un build). Le pull est **anonyme** sur une
   image **publique** : aucune nouvelle surface d'auth, le trou d'auth #260 reste **inchangé**. La
   release publie l'image sur GHCR (job additif indépendant, multi-arch `amd64`+`arm64`, tags
   `h-<hash>` + `latest` informatif) ; le hash CI (bash `sha256sum | cut`) est byte-identique au Rust,
   gardé par un self-check de parité.

8. **Mode immuable par Run.** `off`|`copy`|`pure` est porté par `RunStarted`, projeté une fois, jamais
   muté. Un Run reste sandboxé (ou non) toute sa vie : sinon `claude --continue` (resume) ne
   retrouverait pas son transcript (indexé par chemin de travail). En #407 le mode n'arrivait que par
   le paramètre de l'API `POST /runs`. Depuis **#410** il est **résolu** au chokepoint unique de
   création (`create_run_inner`, où les trois parcours — JSON, multipart, fire de Trigger —
   convergent) par le résolveur **pur** `event_log::effective_sandbox(explicit, trigger,
   instance_default)`, précédence **choix explicite du Run → défaut par-Trigger → `default_sandbox`
   d'instance** (plancher `off`). Le paramètre filaire devient `Option<SandboxMode>` (absent = `None`,
   **distinct** d'un `off` explicite qu'un défaut `copy`/`pure` ne doit jamais surclasser).
   `default_sandbox` est lu **frais** en base au bord (précédence `stored → env → default(off)`,
   ADR-0015, miroir d'`image_source`). L'invariant `off` byte-identique tient : un mode résolu `off`
   n'injecte rien dans le payload (chokepoint inchangé).

9. **Observabilité (câblée #408).** `merge_back` est câblé dans le run-advance — à la **transition
   terminale** (chokepoint `append_event`, tâche détachée pour ne pas coupler la latence/l'échec de la
   transition, cohérent ADR-0023) **et** à `cleanup_run` (avant `teardown`, synchrone, pour capter la
   croissance post-terminale : resume, flushs tardifs de sous-agents). Double merge = état identique
   (copy-if-absent-or-larger idempotent). Coût (`run_cost`) et stale/AutoComplete (`stale_detector`)
   deviennent sandbox-conscients via le seam `transcripts_root(mode, run_id, home_root, sandbox_root)`,
   consommé par les **deux** (source unique, pas d'encodeur dupliqué — leçon #373) : Run sandboxé
   **vivant** → le staging ; après `cleanup_run` → `~/.claude/projects/`. Dispatch **keyé sur
   l'existence du staging dir** (pas le statut terminal : reste correct si le merge terminal best-effort
   a échoué). `resume_run` re-arme d'abord le conteneur (`ensure_ready`-ou-échec, miroir du run-shell)
   car sans `--restart` il est down après un reboot hôte. **session-died** reste
   transcript-indépendante.

10. **Visibilité de la préparation (#410).** La fenêtre de prep eager (point 4) devient observable via
    deux événements **additifs et informationnels** — `SandboxPrepStarted` (en tête de la tâche
    détachée, avant `ensure_ready`) / `SandboxPrepReady` (juste avant le 1er spawn) — émis au
    chokepoint `append_event` (donc broadcast WS + `refreshRun` gratuits, aucune allowlist à toucher),
    **jamais** pour un Run `off` (invariant `off` byte-identique préservé). Ils se projettent dans un
    champ **additif** `RunState.sandbox_prep` (`pending`|`ready`) **sans** toucher `status` (qui reste
    `running` : `is_live`/overlap/admission/reconcilers inchangés — même grain que `NodeBlockedOnLimit`
    / `NodeAutoCompleteObserved`). On **écarte** un statut `Preparing` (blast radius sur toute la
    machine à états + tous les consommateurs de statut) et l'**inférence client** (`running` + 0
    session vive ⇒ preparing : faux positifs pendant la fenêtre d'advance détaché ADR-0023, le
    throttling #159, et l'attente d'un successeur). L'échec de prep reste porté par `RunFailed`
    (`fail_run_sandbox_prep`), **sans** événement de prep dédié. Fast-path (image locale) : la paire
    Started/Ready bascule instantanément. **Non émis** au ré-armement (`resume_run`/`open_run_shell`) :
    hors « premier usage ». Le marqueur `ready` survit à un restart daemon par replay du log.

## Pourquoi (le trou d'auth assumé v1)

Le daemon expose une API HTTP **non authentifiée**, liée à `0.0.0.0` (lib.rs, #260 CLOSED — choix
délibéré d'accès LAN). N'importe quel code dans le conteneur (y compris un agent prompt-injecté) peut
appeler **tout** endpoint via la gateway, pas seulement sa propre complétion.

On l'accepte pour v1 **parce que ce n'est pas net-new** : un nœud hôte **non** sandboxé tourne déjà
en `claude --dangerously-skip-permissions` avec exactement le même accès non authentifié au daemon
(`PDO_DAEMON_URL=http://localhost:<port>`). Le conteneur n'est qu'un client de plus sur un socket
déjà atteignable depuis tout le LAN — un **sous-ensemble strict** de l'exposition que #260 assume, pas
une extension.

On **ne prétend donc pas** que la sandbox est une frontière de sécurité réseau/creds en v1 : elle
tourne en uid/gid hôte, bind-monte le repo rw à son chemin hôte, et stage de vraies credentials
Claude (`.credentials.json`) avec réseau sortant ouvert. Sa **seule** valeur sécurité v1 est un
**blast radius filesystem réduit par défaut** (pas d'accès ambiant au reste de `$HOME`, aux autres
repos, à `~/.ssh`) + le **containment de l'arbre de process** (kill ciblé). Utile, mais inutile face
à un adversaire déterminé ou injecté.

Fermer le trou (auth de l'API daemon, ou tokens de complétion scopés par Run) est **différé au
chantier d'auth du daemon, lié à #260**. D'ici là, un Run sandboxé n'est pas plus fiable vis-à-vis de
l'hôte qu'un Run hôte.

## Alternatives écartées

- **`docker run -d` par session** (conteneur éphémère par nœud) : rejeté — un conteneur par-Run
  long-vécu rend kill et destruction ciblés, partage les mounts, et amortit le coût de démarrage.
- **Fallback hôte si Docker absent** : rejeté frontalement (#403 US-16) — masquerait l'isolation
  demandée ; fail-fast `RunFailed` à la place.
- **`--restart unless-stopped`** : rejeté — PDO possède le cycle de vie ; ressusciterait des
  conteneurs que PDO croit finis.
- **Envelopper `wrap_with_env` entier dans le `docker exec`** (au lieu d'`-e` explicites) : rejeté —
  ré-exporterait `PDO_DAEMON_URL=localhost` dans le conteneur et casserait la gateway.

## Limites acceptées

- **uid hôte ≠ 1000.** `sudo` (getpwuid avant NOPASSWD) et `claude` (`os.userInfo()`) peuvent casser
  faute d'entrée `/etc/passwd` ; ubuntu:24.04 livre `ubuntu`=1000 → le cas laptop courant résout.
  Injection `/etc/passwd`+`/etc/group` différée à une issue de suivi (ne PAS éditer le Dockerfile,
  content-hashé).
- **run-shell in-container** peut être *moins* fidèle pour l'inspection statique (les mounts identité
  donnent déjà la parité fichiers) et perd les outils `sudo`-installés éphémères. On garde le
  wrapping pour l'uniformité + zéro divergence hôte silencieuse (`ensure_running`-or-fail).

## Relations

- **ADR-0004** (stratégie de test) : golden des tails wrappées (unit) + layer-3 (Docker indispo →
  RunFailed, off inchangé, cleanup/boot/kill) via les seams `docker_cmd_override` +
  `sandbox_home_override` (per-daemon, #181) — jamais d'`std::env` global ni de vrai Docker en CI.
- **ADR-0009** (3 couches) : le wrapping vit au chokepoint `build_tmux_script` ; `ensure_ready` est un
  effet atomique qui ne réentre jamais le scheduler.
- **ADR-0012** (autonomie gagnée) : la sandbox réduit le blast radius par défaut du travail autonome ;
  le cap global reste la primitive de sûreté.
- **ADR-0015** (précédence config) : la source du mode est **réalisée en #410** — résolveur pur
  `effective_sandbox` (run → trigger → `default_sandbox`, plancher `off`), `default_sandbox` = nouvelle
  colonne nullable d'`instance_config` (`stored → env → default`, résolveur `default_sandbox_with`
  partagé create/`GET /settings`, 0 drift #373) ; défaut par-Trigger = colonne nullable `sandbox` sur
  `triggers`, clearable via `deserialize_double_option` (précédent `max_concurrent` #239). La **sonde
  Docker** (`docker version`, TTL-cachée, `GET /settings.sandbox_docker`) est **advisory** : le
  fail-fast (point 4) reste le gate autoritaire.
- **ADR-0020 / ADR-0021** (archivage / run-shell) : le conteneur vit de la création au `cleanup_run`
  (= archive), coextensif à la fenêtre d'éligibilité du run-shell ; après un reboot hôte,
  `open_run_shell` ressuscite le conteneur (`ensure_running`-or-fail), car `boot_recovery` saute les
  Runs terminaux.
- **ADR-0023** (advance détaché) : la prep eager suit la même forme détachée + panic-isolée →
  `RunFailed`.
- **#260** : trou d'auth du daemon ; fermeture de la sandbox liée à ce chantier.
