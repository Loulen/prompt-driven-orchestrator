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

## Amendement — Vocabulaire, exceptions de mount `$HOME`, maîtrise du Dockerfile (grilling 2026-07-24, PRD #403)

Quatre points de cette ADR sont amendés à l'issue de la validation manuelle du PRD, **avant** le
merge vers `main`. Le détail du modèle de contenu vit dans **ADR-0031** ; ici, ce qui change du
modèle d'*exécution*.

1. **Vocabulaire (réalisé en #426).** Le tri-état devient `off` | `minimal` | `full`
   (ex-`pure`/`copy`). Aucun alias de compatibilité : aucune valeur persistée n'existe dans
   l'instance prod ni dans l'instance dev (0/399 et 0/103 `run_started` ; ni l'une ni l'autre n'a même
   les colonnes `default_sandbox` / `triggers.sandbox`), un alias n'aurait donc servi que des
   instances de validation jetables. Corollaire assumé : un token pré-renommage retrouvé dans un log
   d'événements se dégrade en `off`, donc vers **moins** d'isolation — les trois décodeurs le
   **loggent** (#426), le point 4 interdisant tout fallback hôte silencieux. `minimal` est plus juste
   que `pure` depuis que le plancher de garanties (ADR-0031 §1) y seede des consentements — le mode
   n'est pas *pur*, il est *minimal*.

2. **Les identity mounts ne sont plus une liste fermée.** Aux quatre mounts du point 1 (repo,
   `.claude` stagé, `.claude.json` stagé, binaire `pdo`) s'ajoutent les **exceptions déclarées** par
   le profil de staging : une entrée hors `.claude` est copiée dans `<staging>/home/<chemin>` puis
   montée rw à `$HOME/<chemin>` (ADR-0031 §4, dédup des entrées internes à `.claude` incluse).
   `create_args` gagne donc une **queue variable** — le golden test qui fige l'ordre canonique doit
   l'accommoder plutôt que la figer.

3. **La valeur sécurité v1 est reformulée, pas retirée.** La section « Pourquoi » revendique comme
   seule valeur un « blast radius filesystem réduit par défaut (pas d'accès ambiant au reste de
   `$HOME`, aux autres repos, à `~/.ssh`) ». Ce n'est plus exact : le refus par défaut de `$HOME`
   devient une **liste d'exceptions déclarées et visibles**. Le défaut reste le refus — un profil
   vierge ne monte rien de plus — mais l'utilisateur peut déclarer `.ssh` s'il l'assume, et l'UI
   l'avertit sans l'interdire (ADR-0031 §3). La posture générale est inchangée : la sandbox n'est
   pas une frontière de sécurité en v1.

4. **Échec fort étendu au profil inconnu.** Le point 4 (« jamais de fallback hôte silencieux »)
   couvrait l'indisponibilité de Docker ; il couvre désormais aussi un nom de profil non résolu —
   400 à la création, tir de Trigger en échec visible, `RunFailed` en boot recovery (ADR-0031 §7).

5. **Le Dockerfile résolu devient un réglage.** Le point 7 supposait un Dockerfile unique, seedé à
   `~/.pdo/sandbox/Dockerfile`. Un réglage d'instance `dockerfile_path` (précédence
   `stored → env → défaut seedé`, ADR-0015) permet d'en pointer un autre — typiquement versionné
   dans le repo, donc partagé par l'équipe. Le tag reste le hash du contenu du fichier **pointé** :
   l'édition déclenche toujours le rebuild. Conséquence opérationnelle : quand le Dockerfile résolu
   **n'est pas à l'emplacement seedé par défaut** (`<sandbox_root>/Dockerfile`), `ensure_image`
   **saute le pull GHCR** (un hash custom ne peut pas exister en amont) et build directement.

   Trois précisions issues du grilling de la slice (#431), parce que la formulation initiale de ce
   point était ambiguë au point d'induire la mauvaise implémentation :

   - **Le prédicat de skip-pull porte sur le CHEMIN, pas sur les octets.** `seed_dockerfile`
     n'écrasant jamais, une machine ayant mis PDO à jour garde sur disque le Dockerfile d'une
     release **antérieure** — dont le tag `h-<hash>` *existe* sur GHCR, puisque `release.yml` publie
     le hash du Dockerfile de chaque arbre de release. Un prédicat comparant les octets au Dockerfile
     embarqué du binaire courant classerait ce cas (le cas dominant, pas un cas limite) en « custom »
     et lui refuserait un pull valide, imposant un build local de plusieurs minutes. Le skip-pull est
     une **optimisation, pas un gate de correction** : le fast-path local et le fallback build rendent
     un pull inutile inoffensif dans les deux sens, donc le prédicat le moins cher gagne. Corollaire
     assumé : un Dockerfile **édité sur place** au chemin par défaut continue de tenter un pull qui
     404 puis retombe sur le build — comportement inchangé depuis #411, une fois par content-hash.
   - **Le contexte de build reste `<sandbox_root>/.build-ctx`, gardé vide**, y compris sous un chemin
     custom. Donc **un Dockerfile pointé doit être auto-porteur : pas de `COPY`/`ADD`**. Suivre le
     répertoire parent du fichier pointé réouvrirait le piège D8 de #405 (les siblings de
     `sandbox_root` sont les staging dirs par-run, écrits concurremment) et, surtout, ferait du tag
     adressé par contenu un mensonge : le hash ne porte que sur les octets du Dockerfile, donc le
     fast-path figerait pour toujours une image dont le contexte a changé. Supporter `COPY`
     demanderait de hasher le contexte : autre contrat de tag, hors périmètre (même catégorie que le
     ref d'image tout fait ci-dessous).
   - **Un chemin résolu qui n'est pas un fichier régulier échoue fort au prep** (`RunFailed` dont la
     `reason` nomme le chemin **et** le tier gagnant), plus un `400` à `PUT /settings` comme gate
     précoce. Jamais de repli vers le seedé : ce serait builder silencieusement **une autre image que
     celle que l'équipe a versionnée**, symptôme reporté au fond d'un node (`gh: command not found`)
     avec un tag d'apparence saine. C'est le point 4 (« jamais de fallback hôte silencieux ») appliqué
     au Dockerfile. Le tier **env** contourne la validation `PUT` par construction — c'est
     l'échappatoire assumée pour un chemin sur volume amovible ; les deux tiers restent gatés au prep.

   Fournir un **ref d'image tout fait** reste hors périmètre : ça
   supprimerait le tag adressé par contenu, exigerait d'écrire le contrat d'image (bash, `claude`
   installé, auto-updater off, `$HOME` inscriptible au chemin hôte) et rouvrirait une question
   d'auth que le pull anonyme évite.

## Amendement — La garantie du point 4 devient une précondition du spawn (#445)

Le point 4 promet « image + conteneur + staging garantis prêts **avant le premier spawn** ». Cette
garantie n'était **pas** portée par le spawn : elle était rejouée par le seul parcours de création,
qui attend `ensure_ready` avant `spawn_ready_after_event`. Les autres déclencheurs d'avancement
n'en savaient rien — le watcher de pipeline (`handle_run_pipeline_modifications`) et le balayage
d'admission cross-Run (`retry_waiting_nodes`) atteignaient le spawn pendant la prep. La tail
`docker exec … pdo-sbx-<run>` tombait alors sur un conteneur inexistant : **exit 1 en ~30 ms**, la
commande de la fenêtre tmux se terminait, et ~25 s plus tard le détecteur de sessions mortes rendait
`session_died`. Reproduit 7 fois sur stack isolée ; le profil `full` était **inutilisable dès qu'on
regardait son Run** (l'onglet ouvert lit `<run>/pipeline.yaml`, ce que le watcher rapporte comme une
modification externe la première fois — inotify `OPEN`, pas d'édition).

1. **La précondition est portée par `spawn_node`, pas par ses appelants** : « un Run sandboxé dont la
   prep n'est pas `ready` n'est pas schedulable ». Décision pure
   (`event_log::RunState::sandbox_spawn_block`) évaluée sur la projection déjà chargée pour le garde
   de transition, **après** lui et **avant** l'admission comme avant toute création de sous-worktree.
   Corriger le site d'appel du watcher aurait laissé le prochain appelant réintroduire le défaut ;
   c'est le même argument qui a mis le garde de transition (#212) dans le spawn. Un `off` n'est
   **jamais** bloqué (invariant byte-identique). Une prep *absente* bloque comme une prep *pendante* :
   `RunStarted` et `SandboxPrepStarted` sont à ~100 ms l'un de l'autre et la course y tient déjà ;
   bloquer est en outre le sens fail-safe (un blocage à tort coûte un spawn rejoué, un passage à tort
   coûte un nœud mort).

2. **Le refus n'écrit RIEN — et c'est ce qui rend le rejeu possible.** Pas de `NodeStarted` (l'event
   qui, seul, fait rendre `session_died` 25 s plus tard), et pas de `NodeWaiting` non plus : un nœud
   `Waiting` sort de `compute_ready_to_spawn`, donc la réservation déplacerait le rejeu sur le
   balayage d'admission cross-Run et un Run dont le seul déclencheur était le watcher pourrait rester
   coincé pour toujours. Sans état, le nœud reste dans l'ensemble prêt et le premier `advance_run`
   suivant `SandboxPrepReady` le démarre. Nouvelle issue de `SpawnOutcome` (`Deferred`), distincte de
   `Refused` (rien à reprendre) et de `Throttled` (réservation posée, retry cross-Run).

3. **Le point 10 est amendé : `SandboxPrepReady` n'est plus seulement informationnel.** Il devient
   le fait qui lève la précondition, donc **tout** parcours qui rend le conteneur réel doit l'émettre
   — ce qui **renverse** le « non émis au ré-armement » du point 10 pour `resume_run` et la boot
   recovery. Sans ça, un Run qui a échoué *pendant* sa prep garde une projection `pending` pour
   toujours et chaque spawn est différé pour toujours : l'interblocage que la précondition ne doit
   pas créer. Émis seulement après un `ensure_ready` en `Ok` (l'event ne prétend jamais qu'un
   conteneur est là), et seulement si le Run était effectivement bloqué (un resume ou un boot de
   routine n'ajoute pas d'event no-op). `open_run_shell` reste **non émetteur** : il ressuscite un Run
   terminal, où rien ne sera spawné — un resume ultérieur repasse par `ensure_ready` de toute façon.

4. **La réconciliation de stall devient sandbox-consciente.** Un Run en prep présente *exactement* la
   signature #279 d'un spawn silencieusement avorté (nœud prêt, aucun nœud vivant, horloge d'inactivité
   qui monte — la prep n'émet rien pendant qu'elle travaille). Sans arme dédiée, `run_stall_reason`
   tuait donc précisément les Runs lents que la précondition venait de sauver (83-87 s mesurés pour un
   profil de 2 Go, davantage sur un `docker build` froid, contre une fenêtre de 120 s). D'où une
   **grâce plus longue et non une exemption** (`SANDBOX_PREP_STALL_GRACE_SECS`, 15 min) : au-delà, la
   tâche de prep est réellement perdue (morte avec un daemon précédent) et le Run échoue avec une
   cause qui **nomme la sandbox** au lieu d'accuser tmux. Différer indéfiniment aurait échangé un faux
   échec contre un stall silencieux, qu'ADR-0004 interdit tout autant.

5. **Le chemin force-spawn répète la précondition, en `409`.** `force_spawn_node` (bouton Start de
   l'UI, `start_node` du manager) ne passe pas par `spawn_node` : il pilote
   `node_primitives::start_node`. Il refuse donc explicitement plutôt que de différer — « démarrer
   maintenant » ne doit pas se mettre en file — en miroir du fail-fast au cap de sessions.

6. **Une prep dont le Run est devenu terminal est abandonnée.** Observé : `container Created` +27 à
   +35 s **après** `run_failed`, puis un `sandbox_prep_ready` sur un cadavre — un conteneur que
   personne n'exécutera jamais. Le point 1 supprime la cause ; il reste les cas légitimes (l'humain
   stoppe ou tue un Run en cours de prep). À la fin d'`ensure_ready`, si le Run est terminal : aucun
   event, aucun spawn, aucun manager, et `docker rm -f` du conteneur (idempotent, best-effort — un
   `resume_run` le recrée). Le staging est **conservé** : c'est `cleanup_run` qui le purge, et le
   détruire ici détruirait les transcripts que `merge_back` moissonne. **Résidu assumé** : la marche
   filesystem déjà lancée n'est pas interrompue — `sandbox_staging::prepare` est un module pur sans
   seam d'annulation, et y ajouter un jeton de cancellation pour ce seul cas coûterait plus que le
   gigaoctet qu'il économise. Le gaspillage est borné à une copie, sans conteneur ni event derrière.

Non traité ici, et **volontairement** : le réveil parasite du watcher lui-même. La première *lecture*
de `<run>/pipeline.yaml` est rapportée comme une modification externe (masque inotify `OPEN` armé par
`notify`, aucun filtrage d'`EventKind` par le debouncer, et `content_actually_changed` renvoie `true`
faute de baseline pour un Run neuf — `seed_run_mtimes` ne tourne qu'au boot ; `copy_pipeline_to_run`
est par ailleurs le seul écrivain de l'arbre qui n'appelle jamais `mark_self_write`). C'est un
`pipeline_modified` mensonger, une fois par Run, à traiter pour lui-même : supprimer ce déclencheur
n'aurait rien corrigé, puisque la précondition doit tenir quel que soit **qui** avance le Run.

Le corps de cette ADR (points 1-10) est laissé **tel quel**, dans le vocabulaire d'avant #426 : y
lire `full` partout où il dit `copy`, et `minimal` partout où il dit `pure`.

## Relations

- **ADR-0031** (profils de staging) : *ce que* le home stagé contient, là où cette ADR fixe *où* et
  *comment* le Run s'exécute. §1 (le plancher de garanties) est **réalisé en #426**, avec le point 1
  de l'amendement ci-dessus.
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
