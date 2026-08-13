# ADR-0030 — Modèle d'exécution de la Sandbox (conteneur par Run)

> Statut : accepted (#407, tracer bullet du PRD #403 ; amendements #405, #410, #414, #426, #431,
> #445, #447, #466, #467, #471 repliés dans le corps). Vocabulaire : CONTEXT.md § « Sandbox ».
> ADR-0031 dit *ce que* contient le home stagé et l'environnement d'un Run sandboxé ; celle-ci dit
> *où* et *comment* le Run s'exécute — y compris le contrat de l'image, dont la rationale complète
> vit ici (pt 7).

Un Run en mode `minimal`/`full` exécute **toutes ses tails** (nœuds agents, manager,
merge-resolver, nœuds `script`, run-shell) dans un unique conteneur long-vécu par Run
(`sleep infinity`, PID 1 = tini). Les guards de Trigger restent hôte (décision de fiançailles, pas
de travail de Run).

Le tri-état du mode est **`off | minimal | full`** (#426, ex-`pure`/`copy`), **sans alias de
compatibilité** : aucune valeur persistée n'existait dans les instances réelles. Corollaire
assumé : un token pré-renommage retrouvé dans un log d'événements se dégrade en `off` — vers
**moins** d'isolation — et les décodeurs le loggent (le pt 4 interdisant tout fallback hôte
silencieux). `minimal` est plus juste que `pure` : le plancher de garanties (ADR-0031 §1) y seede
des consentements — le mode n'est pas *pur*, il est *minimal*.

## Ce qu'on décide

1. **Identity mounts + injection d'identité.** Le repo cible est bind-monté rw à son **chemin
   absolu hôte** (un seul mount couvre repo + worktrees de nœuds + prompts) ; le *staged Claude
   home* et son `.claude.json` sibling sont montés aux chemins `$HOME` correspondants ; le binaire
   `pdo` hôte en lecture seule. S'y ajoutent les **exceptions déclarées** par le profil de staging
   (ADR-0031 §4) : une entrée hors `.claude` est copiée dans le staging puis montée rw à
   `$HOME/<chemin>` — la liste de mounts n'est plus fermée. Le conteneur adopte l'**uid/gid hôte**
   (`--user` numérique). Résultat : le chemin de travail est identique des deux côtés → le dirname
   encodé des transcripts matche (pré-requis du merge-back, pt 9).

   **L'identité de l'uid hôte se pose sur le conteneur démarré, pas dans les mounts (#414).** La
   liste des mounts ne bouge pas et l'argv du create reste byte-identique : le `--user` numérique
   donne au process l'uid/gid hôte, mais l'image ne connaît de *nom* que pour l'uid 1000. Après le
   start, un exec root ajoute les entrées manquantes aux `/etc/passwd`/`/etc/group` **réels** de
   l'image, derrière une garde — no-op exact en uid 1000, best-effort partout (jamais un Run
   cassé). Le **Dockerfile n'est pas modifié — pas même un commentaire** : il est hashé octet par
   octet pour dériver le tag (pt 7), toute édition périmerait l'image buildée et publiée.

2. **Staging par Run.** Un répertoire de staging par Run sous le sandbox root (jamais le vrai
   `~/.claude`), seedé à la prep selon le profil, purgé au `cleanup_run`. En `minimal`, la
   confiance est pré-accordée à la **racine du repo** — l'ancêtre commun du worktree pipeline et de
   tous les worktrees de nœuds, donc héritée par chaque cwd de session.

3. **Réseau = host-gateway, et l'URL du daemon a un résolveur unique (#447).** Le conteneur joint
   le daemon hôte via la gateway Docker, l'URL étant posée **au create** (jamais re-passée à
   l'exec : un `-e` nu re-forwarderait le `localhost` hôte et clobbererait la gateway). C'est ce
   qui permet à `pdo complete`/`fail`/`skip` in-container de rappeler le daemon.

   Le résolveur d'URL est **unique**, possédé par le module qui possède la gateway (le hostname de
   la gateway et la gateway sont le même fait), et consommé à la fois par l'env du create **et**
   par le texte du préambule manager — les deux ne peuvent plus diverger. Avant #447, le préambule
   codait `localhost:<port>` en dur : toute la surface de commande du manager était inatteignable
   sur un Run sandboxé (reproduit 3/3 avec contrôle négatif `off`) — panne silencieuse, non
   déterministe et affirmative (le manager concluait de bonne foi que le daemon était mort).
   Nuance load-bearing : l'argument du résolveur nomme le **côté d'exécution**, pas le mode du
   Run — les exports d'env côté hôte du wrapper (pt 5) résolvent « hôte » même pour un Run
   sandboxé, parce qu'ils s'exécutent avant le `docker exec` et ne traversent pas.

4. **Préparation eager fail-fast, portée comme précondition du spawn (#445).** Image + conteneur +
   staging sont garantis prêts **avant le premier spawn** ; toute indisponibilité de Docker →
   `RunFailed` explicite. **Jamais de fallback hôte silencieux** — règle étendue au profil inconnu
   (400 à la création, tir de Trigger en échec visible, `RunFailed` en boot recovery, ADR-0031 §7)
   et au Dockerfile pointé invalide (pt 7). La prep tourne sur une tâche détachée (le build du
   premier run machine ne doit pas bloquer la création — cohérent ADR-0023), panic isolée →
   `RunFailed`.

   La garantie était d'abord rejouée par le seul parcours de création : le watcher de pipeline et
   le balayage d'admission cross-Run atteignaient le spawn **pendant** la prep, la tail tombait sur
   un conteneur inexistant et ~25 s plus tard le Run mourait en `session_died` (reproduit 7 fois ;
   le profil `full` était inutilisable dès qu'on regardait son Run — l'onglet ouvert déclenchait le
   watcher). D'où :

   - **la précondition est portée par le spawn lui-même, pas par ses appelants** — « un Run
     sandboxé dont la prep n'est pas prête n'est pas schedulable » ; corriger le site d'appel
     aurait laissé le prochain appelant réintroduire le défaut (même argument que le garde de
     transition #212). Un `off` n'est **jamais** bloqué ; une prep *absente* bloque comme une prep
     *pendante* (fail-safe : un blocage à tort coûte un spawn rejoué, un passage à tort un nœud
     mort) ;
   - **le refus n'écrit rien** — ni événement de démarrage (celui qui, seul, fait rendre
     `session_died`), ni réservation `Waiting` (qui sortirait le nœud de l'ensemble prêt et
     pourrait le coincer pour toujours). Sans état, le premier advance suivant la fin de prep le
     démarre ;
   - **l'événement de fin de prep lève la précondition**, donc tout parcours qui rend le conteneur
     réel l'émet — y compris resume et boot recovery (sinon un Run échoué pendant sa prep resterait
     différé pour toujours). Émis seulement après une prep en succès, et seulement si le Run était
     effectivement bloqué ; le run-shell reste non émetteur (il ressuscite un Run terminal où rien
     ne sera spawné) ;
   - **la réconciliation de stall accorde une grâce plus longue (15 min), pas une exemption** : un
     Run en prep présente exactement la signature d'un spawn silencieusement avorté, et sans grâce
     le filet tuait précisément les Runs lents que la précondition venait de sauver (83-87 s
     mesurés pour un profil de 2 Go, davantage sur un build froid, contre une fenêtre de 120 s).
     Au-delà, le Run échoue avec une cause qui **nomme la sandbox** au lieu d'accuser tmux ;
     différer indéfiniment échangerait un faux échec contre un stall silencieux (ADR-0004) ;
   - **une prep dont le Run est devenu terminal est abandonnée** : aucun event, aucun spawn, et le
     conteneur est supprimé (idempotent, best-effort). Le staging est **conservé** — c'est le
     cleanup qui le purge, et le détruire ici détruirait les transcripts que le merge-back
     moissonne.

   **Visibilité de la prep (#410).** La fenêtre est observable via deux événements additifs et
   informationnels (début/fin de prep), **jamais** émis pour un Run `off`, projetés dans un champ
   additif sans toucher le statut du Run (qui reste `running`). On écarte un statut `Preparing`
   (blast radius sur toute la machine à états) et l'inférence client « running + 0 session ⇒
   preparing » (faux positifs pendant la fenêtre d'advance détaché et le throttling). L'échec de
   prep reste porté par `RunFailed`. Le marqueur survit à un restart daemon par replay du log.

5. **Wrapping au chokepoint unique.** Toutes les familles de tails funnel par le même constructeur
   de script tmux : quand le Run est sandboxé, la tail est enveloppée en `docker exec … bash -lc`.
   Les exports d'env de base restent côté **hôte** (inoffensifs) — d'où l'invariant `off`
   **byte-identique** quand le wrapping est absent. Le catalogue d'env dynamique d'un nœud `script`
   traverse l'exec en `-e KEY=VALUE` **explicites** (un `-e` nu ne forwarderait que la valeur du
   shell hôte, que la sandbox n'exporte pas) — jamais l'URL du daemon.

6. **Kill ciblé.** Un kill de session est doublé d'un exec séparé qui retrouve, via un marqueur de
   session dans l'environnement des process, le seul arbre porteur et le termine ; les sessions
   sœurs survivent (le client `docker exec` tué côté tmux ne tue pas le process conteneur,
   reparenté sur PID 1).

7. **Image : tag adressé par contenu, fourniture hybride, variantes, ref registry explicite.**

   **Le tag est le hash du contenu (#405).** `pdo-sandbox:h-<hash>` où `<hash>` = SHA-256 tronqué
   des octets exacts du Dockerfile sur disque. Deux Dockerfiles identiques → même tag ; une
   édition → rebuild ; fins de ligne épinglées pour la reproductibilité. C'est l'identité qui rend
   une image **tirée d'un registry** et une image **buildée localement** interchangeables sous le
   même nom (#411) : en source `registry` (défaut), si l'image n'est pas locale, pull anonyme
   depuis GHCR puis retag sous le ref local, avec **fallback build** si le pull échoue (offline /
   tag absent / registry down) ; en source `dockerfile`, build direct, jamais de pull. Le pull
   anonyme d'une image publique n'ouvre aucune surface d'auth. La release publie l'image
   (multi-arch), avec un self-check de parité du hash entre CI et daemon.

   **Le nom d'image est une donnée de la variante (#466).** Un ref est un couple `<nom>:h-<hash>`
   dont les deux moitiés sortent du même fichier : le hash de ses **octets**, le nom de son **nom
   de fichier** (`Dockerfile` → nom de base, `Dockerfile.<variante>` → nom suffixé ; tout autre nom
   retombe sur le nom de base — un Dockerfile pointé par l'utilisateur n'a pas à suivre la
   convention, son tag reste le hash de ses octets). Corollaire assumé : un Dockerfile de variante
   est **autonome** (steps de la base dupliqués), jamais `FROM` l'image de base par hash — injecter
   le hash de la base obligerait à *générer* les octets de la variante, or ces octets **sont** la
   source de vérité de son propre tag. La duplication est le prix de l'adressage par contenu.
   Première variante livrée : chrome-dev (#466).

   **Le Dockerfile résolu se choisit par profil (#431, #467, #471).** Précédence
   **profil (si posé) → env → défaut de profil** : la source d'image n'est plus un réglage
   d'instance — ce qu'elle valait par défaut est devenu une constante de la couche des profils
   (registre hash-dérivé sur le Dockerfile seedé), et les deux variables d'env survivent,
   repointées sur ce défaut (une instance headless fraîche n'a pas d'UI ; l'env est son seul moyen
   de changer d'image sans POSTer un profil). Le fichier seedé par défaut reste écrit : ce n'est
   pas un réglage, c'est la **matérialisation du défaut** — l'utilisateur l'édite pour changer le
   hash, donc l'image. Trois précisions dont chacune a failli produire la mauvaise
   implémentation :

   - **Le prédicat de skip-pull porte sur le CHEMIN, pas sur les octets.** Le seed n'écrase
     jamais : une machine à jour garde le Dockerfile d'une release antérieure, dont le tag existe
     en amont. Un prédicat sur les octets classerait ce cas **dominant** en « custom » et
     imposerait un build local de plusieurs minutes. Le skip-pull est une optimisation, pas un gate
     de correction : fast-path local et fallback build rendent un pull inutile inoffensif dans les
     deux sens, donc le prédicat le moins cher gagne.
   - **Le contexte de build reste un répertoire dédié gardé vide**, y compris sous un chemin
     custom : un Dockerfile pointé doit être **auto-porteur** (pas de `COPY`/`ADD`). Suivre le
     répertoire parent du fichier pointé rouvrirait le piège des siblings écrits concurremment
     (staging dirs par-Run) et ferait du tag adressé par contenu un mensonge : le hash ne porte que
     sur les octets du Dockerfile, donc le fast-path figerait pour toujours une image dont le
     contexte a changé.
   - **Un chemin résolu qui n'est pas un fichier régulier échoue fort au prep** (la cause nomme le
     chemin **et** le tier gagnant), plus un refus précoce à l'écriture du réglage. Jamais de repli
     vers le seedé : ce serait builder silencieusement **une autre image que celle que l'équipe a
     versionnée**, symptôme reporté au fond d'un node avec un tag d'apparence saine. Le tier env
     contourne la validation d'écriture par construction — échappatoire assumée ; il reste gaté au
     prep.

   **Un ref registry explicite (profil `kind: registry`, ADR-0031 §9) sort de l'adressage par
   contenu, et l'assume.** Un ref libre (ex. `ghcr.io/acme/agent:1.4`) n'a pas de Dockerfile, donc
   pas de hash, donc rien à builder : un « fallback » ne pourrait que builder une image **sans
   rapport** et la faire passer pour celle demandée. Donc : **pas de repli build** — un pull en
   échec est une erreur **dure** qui nomme le ref et le profil, jamais un build silencieux ; **pas
   de retag** — le ref local est le ref demandé, tel quel ; **fast-path conservé** — un ref déjà
   local est réutilisé offline (la seule propriété du chemin hash-dérivé qui survit intacte) ; et
   **PDO ne vérifie pas que l'image contient `claude`** (ni au write — un aller-retour réseau dans
   un handler d'écriture — ni au prep) : c'est la responsabilité de qui fournit le ref ; une image
   sans lui échoue au premier exec, avec le stderr de docker.

   Vocabulaire, confusion assumée : la **source** `registry` tire l'image prébuild de VOTRE
   Dockerfile — qui reste **obligatoire** dans ce mode, le tag étant le hash de ses octets, sans
   eux l'image est innommable ; le **`kind: registry`** d'un profil est un ref arbitraire sans
   Dockerfile. Les deux se choisissent dans le même sélecteur de l'éditeur de profil, ce qui rend
   la distinction énonçable en une phrase. Fournir un ref d'image tout fait au niveau *instance*
   reste hors périmètre ; le profil `kind: registry` est la seule porte, avec les contreparties
   ci-dessus.

8. **Mode immuable par Run.** `off`|`minimal`|`full` est porté par `RunStarted`, projeté une fois,
   jamais muté : sinon le resume (`claude --continue`) ne retrouverait pas son transcript (indexé
   par chemin de travail). Le mode est **résolu** au chokepoint unique de création — où les trois
   parcours (JSON, multipart, fire de Trigger) convergent — par un résolveur pur, précédence
   **choix explicite du Run → défaut par-Trigger → défaut d'instance** (plancher `off`) (#410). Le
   paramètre filaire est optionnel : absent est **distinct** d'un `off` explicite, qu'un défaut
   `minimal`/`full` ne doit jamais surclasser. Le défaut d'instance suit ADR-0015
   (`stored → env → default`), lu frais au bord. L'invariant `off` byte-identique tient : un mode
   résolu `off` n'injecte rien dans le payload.

9. **Observabilité des transcripts (#408).** Le merge-back des transcripts est câblé à la
   transition terminale (tâche détachée, cohérent ADR-0023) **et** au `cleanup_run` (synchrone,
   avant teardown, pour capter la croissance post-terminale : resume, flushs tardifs de
   sous-agents) ; le double merge est idempotent. Le calcul de coût et la sonde de fin de tour
   deviennent sandbox-conscients via un seam unique `transcripts_root` consommé par les deux
   (source unique, pas d'encodeur dupliqué — leçon #373) : Run sandboxé **vivant** → le staging ;
   après cleanup → le home hôte. Dispatch keyé sur l'**existence du staging dir**, pas le statut
   terminal (reste correct si le merge terminal best-effort a échoué). `resume_run` re-arme
   d'abord le conteneur (prep-ou-échec) car sans politique de restart il est down après un reboot
   hôte. La détection de mort de session reste transcript-indépendante.

## Pourquoi (le trou d'auth assumé v1)

Le daemon expose une API HTTP **non authentifiée**, liée à `0.0.0.0` (#260 CLOSED — choix délibéré
d'accès LAN). N'importe quel code dans le conteneur (y compris un agent prompt-injecté) peut appeler
**tout** endpoint via la gateway, pas seulement sa propre complétion.

On l'accepte pour v1 **parce que ce n'est pas net-new** : un nœud hôte non sandboxé tourne déjà en
`claude --dangerously-skip-permissions` avec exactement le même accès non authentifié au daemon. Le
conteneur n'est qu'un client de plus sur un socket déjà atteignable depuis tout le LAN — un
sous-ensemble strict de l'exposition que #260 assume.

On **ne prétend donc pas** que la sandbox est une frontière de sécurité réseau/creds en v1 : elle
tourne en uid/gid hôte, bind-monte le repo rw à son chemin hôte, et stage de vraies credentials
Claude avec réseau sortant ouvert. Sa seule valeur sécurité v1 est un **refus par défaut du reste de
`$HOME`** — devenu, avec les profils, une **liste d'exceptions déclarées et visibles** (le défaut
reste le refus ; l'utilisateur peut déclarer `.ssh` s'il l'assume, et l'UI l'avertit sans
l'interdire, ADR-0031 §3) — plus le **containment de l'arbre de process** (kill ciblé). Utile, mais
inutile face à un adversaire déterminé ou injecté.

Fermer le trou (auth de l'API daemon, ou tokens de complétion scopés par Run) est **différé au
chantier d'auth du daemon, lié à #260**. D'ici là, un Run sandboxé n'est pas plus fiable vis-à-vis
de l'hôte qu'un Run hôte.

## Alternatives écartées

- **Un conteneur éphémère par session/nœud** : rejeté — un conteneur par-Run long-vécu rend kill et
  destruction ciblés, partage les mounts, et amortit le coût de démarrage.
- **Fallback hôte si Docker absent** : rejeté frontalement (#403 US-16) — masquerait l'isolation
  demandée ; fail-fast `RunFailed` à la place.
- **`--restart unless-stopped`** : rejeté — PDO possède le cycle de vie ; ressusciterait des
  conteneurs que PDO croit finis.
- **Envelopper le wrapper d'env entier dans le `docker exec`** (au lieu d'`-e` explicites) :
  rejeté — ré-exporterait l'URL `localhost` hôte dans le conteneur et casserait la gateway.
- **Bind-monter un `/etc/passwd` + `/etc/group` générés à la prep** (le mécanisme que #414
  prescrivait) : rejeté **sur mesure**. Un `/etc/passwd` bind-monté casse l'installation de tout
  paquet créant un utilisateur système — `useradd` fait un `rename()` par-dessus le point de
  montage, qui échoue **en `:ro` comme en `:rw`** (dpkg laissé à moitié configuré) — c'est-à-dire
  une **régression sur le chemin uid 1000 actuel**. S'y ajoutent deux coûts de conception :
  construire le fichier exige de connaître la baseline de l'image (éditable, voire un ref registry
  arbitraire), et un mount dont la source manque rend le staging de ~1 Go indélébile par le daemon.
- **`nss_wrapper`** (`LD_PRELOAD` + passwd de substitution) : rejeté — la lib devrait être *dans*
  l'image (circulaire : c'est justement le Dockerfile qu'on ne touche pas), et un `LD_PRELOAD` est
  sans effet sur un binaire statique.
- **`--user <nom>` une fois passwd peuplé** : rejeté — pour un uid que l'image ne connaît pas,
  Docker résout le gid primaire via `/etc/passwd` et retombe sur **gid 0** — bug silencieux de
  propriété de fichiers. Écrit séparément parce qu'un nettoyage bien intentionné (« maintenant que
  passwd est peuplé, autant nommer l'utilisateur ») est exactement la forme que prendrait la
  régression.

## Limites acceptées

- **run-shell in-container** peut être *moins* fidèle pour l'inspection statique (les mounts
  identité donnent déjà la parité fichiers) et perd les outils installés éphémères. On garde le
  wrapping pour l'uniformité + zéro divergence hôte silencieuse.
- Le réveil parasite du watcher de pipeline (la première *lecture* du YAML d'un Run rapportée comme
  modification externe) est un défaut distinct, volontairement non traité ici : la précondition du
  pt 4 doit tenir quel que soit **qui** avance le Run.

## Relations

- **ADR-0031** (profils de staging) : *ce que* le home stagé contient et l'env du conteneur, là où
  cette ADR fixe *où* et *comment* le Run s'exécute. La rationale image (§9 de 0031) vit ici, pt 7.
- **ADR-0032** (liveness) : le seam `transcripts_root` du pt 9 a deux consommateurs — le coût et la
  sonde de **fin de tour** (le verdict stale/AutoComplete n'existe plus depuis #469). À lire aussi
  pour la raison pour laquelle la mort d'un nœud sandboxé est exacte par construction : le pane
  porte le client `docker exec`, qui rend la main dès que `claude` sort dans le conteneur.
- **ADR-0004** : golden des tails wrappées (unit) + couche 3 (Docker indispo → RunFailed, `off`
  inchangé, cleanup/boot/kill) via des seams d'override per-daemon — jamais de vrai Docker en CI.
- **ADR-0009** : le wrapping vit au chokepoint de construction des tails ; la prep est un effet
  atomique qui ne réentre jamais le scheduler.
- **ADR-0012** : la sandbox réduit le blast radius par défaut du travail autonome ; le cap global
  reste la primitive de sûreté.
- **ADR-0015** : précédence du mode (run → trigger → instance) et des réglages d'instance.
- **ADR-0020 / ADR-0021** : le conteneur vit de la création au `cleanup_run` (= archive),
  coextensif à la fenêtre d'éligibilité du run-shell ; après un reboot hôte, le run-shell
  ressuscite le conteneur (prep-ou-échec), car la boot recovery saute les Runs terminaux.
- **ADR-0023** : la prep eager suit la même forme détachée + panic-isolée → `RunFailed`.
- **#260** : trou d'auth du daemon ; la fermeture côté sandbox est liée à ce chantier.
