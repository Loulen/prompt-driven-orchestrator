# ADR-0030 — Modèle d'exécution de la Sandbox (conteneur par Run)

Sans cette ADR, on isolerait un Run par conteneur éphémère **par nœud**, ou on retomberait sur l'hôte
quand Docker manque — deux choix qui cassent respectivement le kill ciblé et la garantie d'isolation
demandée, sans qu'aucun test ne le signale.

> Statut : accepted (#407 ; amendements #405, #410, #414, #426, #431, #445, #447, #466, #467, #471
> repliés dans le corps). ADR-0031 dit *ce que* contient le home stagé ; celle-ci dit *où* et
> *comment* le Run s'exécute — y compris le contrat de l'image (pt 7).

Un Run en mode `minimal`/`full` exécute **toutes ses tails** (nœuds agents, manager, merge-resolver,
nœuds `script`, run-shell) dans un unique conteneur long-vécu par Run (`sleep infinity`, PID 1 = tini).
Les guards de Trigger restent hôte (décision de fiançailles, pas de travail de Run).

Le tri-état du mode est **`off | minimal | full`** (#426, ex-`pure`/`copy`), **sans alias de
compatibilité** : aucune valeur persistée n'existait dans les instances réelles. Corollaire assumé : un
token pré-renommage se dégrade en `off` — vers **moins** d'isolation — et les décodeurs le loggent.
`minimal` est plus juste que `pure` : le plancher de garanties y seede des consentements — le mode
n'est pas *pur*, il est *minimal*.

## Ce qu'on décide

1. **Identity mounts + injection d'identité.** Le repo cible est bind-monté rw à son **chemin absolu
   hôte** (un seul mount couvre repo + worktrees + prompts) ; le staged Claude home et son
   `.claude.json` sibling aux chemins `$HOME` correspondants ; le binaire `pdo` hôte en lecture seule.
   S'y ajoutent les **exceptions déclarées** par le profil (ADR-0031 §4) : la liste de mounts n'est pas
   fermée. Le conteneur adopte l'**uid/gid hôte** (`--user` numérique) pour que le chemin de travail
   soit identique des deux côtés — pré-requis du merge-back (pt 9), qui indexe par dirname encodé.

   **L'identité de l'uid hôte se pose sur le conteneur démarré, pas dans les mounts (#414).** L'image
   ne connaît de *nom* que pour l'uid 1000 ; après le start, un exec root ajoute les entrées manquantes
   aux `/etc/passwd`/`/etc/group` **réels**, derrière une garde — no-op exact en uid 1000, best-effort
   partout. Le **Dockerfile n'est pas modifié — pas même un commentaire** : il est hashé octet par
   octet pour dériver le tag (pt 7), toute édition périmerait l'image publiée.

2. **Staging par Run.** Un répertoire de staging par Run sous le sandbox root (jamais le vrai
   `~/.claude`), seedé à la prep, purgé au `cleanup_run`. En `minimal`, la confiance est pré-accordée à
   la **racine du repo** — l'ancêtre commun de tous les worktrees, donc héritée par chaque cwd.

3. **Réseau = host-gateway, et l'URL du daemon a un résolveur unique (#447).** L'URL est posée **au
   create**, jamais re-passée à l'exec : un `-e` nu re-forwarderait le `localhost` hôte et clobbererait
   la gateway. Le résolveur est **unique**, possédé par le module qui possède la gateway, et consommé à
   la fois par l'env du create **et** par le texte du préambule manager — les deux ne peuvent plus
   diverger. Avant #447, le préambule codait `localhost:<port>` en dur : toute la surface de commande du
   manager était inatteignable sur un Run sandboxé, en panne silencieuse et affirmative (le manager
   concluait de bonne foi que le daemon était mort). Nuance load-bearing : l'argument du résolveur nomme
   le **côté d'exécution**, pas le mode du Run — les exports d'env côté hôte du wrapper (pt 5) résolvent
   « hôte » même pour un Run sandboxé, parce qu'ils s'exécutent avant le `docker exec`.

4. **Préparation eager fail-fast, portée comme précondition du spawn (#445).** Image + conteneur +
   staging sont prêts **avant le premier spawn** ; toute indisponibilité de Docker → `RunFailed`
   explicite. **Jamais de fallback hôte silencieux** — règle étendue au profil inconnu (ADR-0031 §7) et
   au Dockerfile pointé invalide (pt 7). La prep tourne détachée (le build du premier run machine ne
   doit pas bloquer la création), panic isolée → `RunFailed`.

   - **La précondition est portée par le spawn lui-même, pas par ses appelants** — « un Run sandboxé
     dont la prep n'est pas prête n'est pas schedulable ». Corriger le site d'appel aurait laissé le
     prochain appelant réintroduire le défaut : le watcher de pipeline et le balayage d'admission
     atteignaient le spawn **pendant** la prep, et le Run mourait en `session_died`. Un `off` n'est
     jamais bloqué ; une prep *absente* bloque comme une prep *pendante* (fail-safe : un blocage à tort
     coûte un spawn rejoué, un passage à tort un nœud mort).
   - **Le refus n'écrit rien** — ni événement de démarrage (celui qui, seul, fait rendre `session_died`),
     ni réservation `Waiting` (qui sortirait le nœud de l'ensemble prêt et pourrait le coincer pour
     toujours). Sans état, le premier advance suivant la fin de prep le démarre.
   - **L'événement de fin de prep lève la précondition**, donc tout parcours qui rend le conteneur réel
     l'émet — resume et boot recovery inclus, sinon un Run échoué pendant sa prep resterait différé pour
     toujours. Le run-shell reste non émetteur (il ressuscite un Run terminal où rien ne sera spawné).
   - **La réconciliation de stall accorde une grâce plus longue (15 min), pas une exemption** : un Run en
     prep présente exactement la signature d'un spawn silencieusement avorté, et sans grâce le filet tuait
     précisément les Runs lents que la précondition venait de sauver. Au-delà, le Run échoue avec une
     cause qui **nomme la sandbox** au lieu d'accuser tmux ; différer indéfiniment échangerait un faux
     échec contre un stall silencieux.
   - **Une prep dont le Run est devenu terminal est abandonnée** : aucun event, aucun spawn, conteneur
     supprimé. Le staging est **conservé** — le détruire ici détruirait les transcripts que le merge-back
     moissonne.

   **Visibilité de la prep (#410).** Deux événements additifs et informationnels (début/fin), **jamais**
   émis pour un Run `off`, projetés dans un champ additif sans toucher le statut du Run (qui reste
   `running`). On écarte un statut `Preparing` (blast radius sur la machine à états) et l'inférence
   client « running + 0 session ⇒ preparing » (faux positifs pendant l'advance détaché et le throttling).

5. **Wrapping au chokepoint unique.** Toutes les familles de tails funnel par le même constructeur de
   script tmux ; quand le Run est sandboxé, la tail est enveloppée en `docker exec … bash -lc`. Les
   exports d'env de base restent côté **hôte** — d'où l'invariant `off` **byte-identique** quand le
   wrapping est absent. Le catalogue d'env dynamique d'un nœud `script` traverse l'exec en `-e KEY=VALUE`
   **explicites** (un `-e` nu ne forwarderait que la valeur du shell hôte) — jamais l'URL du daemon.

6. **Kill ciblé.** Un kill de session est doublé d'un exec séparé qui retrouve, via un marqueur de session
   dans l'environnement des process, le seul arbre porteur et le termine ; les sessions sœurs survivent
   (le client `docker exec` tué côté tmux ne tue pas le process conteneur, reparenté sur PID 1).

7. **Image : tag adressé par contenu, fourniture hybride, variantes, ref registry explicite.**

   **Le tag est le hash du contenu (#405).** `pdo-sandbox:h-<hash>` où `<hash>` = SHA-256 tronqué des
   octets exacts du Dockerfile sur disque, fins de ligne épinglées. C'est l'identité qui rend une image
   **tirée d'un registry** et une image **buildée localement** interchangeables sous le même nom : en
   source `registry` (défaut), pull anonyme depuis GHCR puis retag, avec **fallback build** si le pull
   échoue ; en source `dockerfile`, build direct, jamais de pull.

   **Le nom d'image est une donnée de la variante (#466).** Un ref est un couple `<nom>:h-<hash>` dont les
   deux moitiés sortent du même fichier : le hash de ses **octets**, le nom de son **nom de fichier**.
   Corollaire assumé : un Dockerfile de variante est **autonome** (steps dupliqués), jamais `FROM` l'image
   de base par hash — injecter le hash de la base obligerait à *générer* les octets de la variante, or ces
   octets **sont** la source de vérité de son propre tag. La duplication est le prix de l'adressage par
   contenu.

   **Le Dockerfile résolu se choisit par profil (#431, #467, #471)**, précédence
   **profil (si posé) → env → défaut de profil**. Le fichier seedé par défaut reste écrit : ce n'est pas
   un réglage, c'est la **matérialisation du défaut**. Trois précisions dont chacune a failli produire la
   mauvaise implémentation :

   - **Le prédicat de skip-pull porte sur le CHEMIN, pas sur les octets.** Le seed n'écrase jamais : une
     machine à jour garde le Dockerfile d'une release antérieure, dont le tag existe en amont. Un prédicat
     sur les octets classerait ce cas **dominant** en « custom » et imposerait un build local de plusieurs
     minutes. Le skip-pull est une optimisation, pas un gate de correction.
   - **Le contexte de build reste un répertoire dédié gardé vide**, y compris sous un chemin custom : un
     Dockerfile pointé doit être **auto-porteur** (pas de `COPY`/`ADD`). Suivre le répertoire parent
     rouvrirait le piège des siblings écrits concurremment et ferait du tag adressé par contenu un
     mensonge — le hash ne porte que sur les octets du Dockerfile.
   - **Un chemin résolu qui n'est pas un fichier régulier échoue fort au prep** (la cause nomme le chemin
     **et** le tier gagnant). Jamais de repli vers le seedé : ce serait builder silencieusement **une autre
     image que celle que l'équipe a versionnée**, sous un tag d'apparence saine.

   **Un ref registry explicite (profil `kind: registry`) sort de l'adressage par contenu, et l'assume.**
   Un ref libre n'a pas de Dockerfile, donc pas de hash, donc rien à builder : un « fallback » ne pourrait
   que builder une image **sans rapport** et la faire passer pour celle demandée. Donc **pas de repli
   build** (un pull en échec est une erreur dure qui nomme le ref et le profil), **pas de retag**,
   **fast-path conservé**, et **PDO ne vérifie pas que l'image contient `claude`** — c'est la
   responsabilité de qui fournit le ref ; une image sans lui échoue au premier exec.

   Vocabulaire, confusion assumée : la **source** `registry` tire l'image prébuild de VOTRE Dockerfile —
   qui reste **obligatoire** dans ce mode, sans ses octets l'image est innommable ; le **`kind: registry`**
   d'un profil est un ref arbitraire sans Dockerfile.

8. **Mode immuable par Run.** Porté par `RunStarted`, projeté une fois, jamais muté : sinon le resume
   (`claude --continue`) ne retrouverait pas son transcript, indexé par chemin de travail. Le mode est
   **résolu** au chokepoint unique de création — où les trois parcours (JSON, multipart, fire de Trigger)
   convergent — précédence **choix explicite du Run → défaut par-Trigger → défaut d'instance** (plancher
   `off`). Le paramètre filaire est optionnel : absent est **distinct** d'un `off` explicite, qu'un défaut
   `minimal`/`full` ne doit jamais surclasser.

9. **Observabilité des transcripts (#408).** Le merge-back est câblé à la transition terminale (détaché)
   **et** au `cleanup_run` (synchrone, avant teardown, pour capter la croissance post-terminale : resume,
   flushs tardifs de sous-agents) ; le double merge est idempotent. Le calcul de coût et la sonde de fin de
   tour deviennent sandbox-conscients via un seam unique `transcripts_root` consommé par les deux (source
   unique, pas d'encodeur dupliqué). Dispatch keyé sur l'**existence du staging dir**, pas le statut
   terminal — reste correct si le merge terminal best-effort a échoué. `resume_run` re-arme d'abord le
   conteneur car, sans politique de restart, il est down après un reboot hôte.

## Pourquoi (le trou d'auth assumé v1)

Le daemon expose une API HTTP **non authentifiée**, liée à `0.0.0.0` (#260, choix délibéré d'accès LAN).
N'importe quel code dans le conteneur — y compris un agent prompt-injecté — peut appeler **tout** endpoint
via la gateway. On l'accepte pour v1 **parce que ce n'est pas net-new** : un nœud hôte non sandboxé tourne
déjà en `claude --dangerously-skip-permissions` avec exactement le même accès.

On **ne prétend donc pas** que la sandbox est une frontière de sécurité réseau/creds en v1 : elle tourne en
uid/gid hôte, bind-monte le repo rw, et stage de vraies credentials avec réseau sortant ouvert. Sa seule
valeur sécurité v1 est un **refus par défaut du reste de `$HOME`** — devenu une liste d'exceptions déclarées
et visibles — plus le **containment de l'arbre de process**. Fermer le trou est différé au chantier d'auth
du daemon (#260).

## Alternatives écartées

- **Un conteneur éphémère par session/nœud** : un conteneur par-Run long-vécu rend kill et destruction
  ciblés, partage les mounts, et amortit le coût de démarrage.
- **Fallback hôte si Docker absent** : masquerait l'isolation demandée ; fail-fast `RunFailed` à la place.
- **`--restart unless-stopped`** : PDO possède le cycle de vie ; ressusciterait des conteneurs que PDO
  croit finis.
- **Envelopper le wrapper d'env entier dans le `docker exec`** : ré-exporterait l'URL `localhost` hôte et
  casserait la gateway.
- **Bind-monter un `/etc/passwd` + `/etc/group` générés à la prep** (le mécanisme que #414 prescrivait) :
  rejeté **sur mesure**. Un `/etc/passwd` bind-monté casse l'installation de tout paquet créant un
  utilisateur système — `useradd` fait un `rename()` par-dessus le point de montage, qui échoue **en `:ro`
  comme en `:rw`** — c'est-à-dire une **régression sur le chemin uid 1000 actuel**. S'y ajoute qu'un mount
  dont la source manque rend le staging de ~1 Go indélébile par le daemon.
- **`nss_wrapper`** : la lib devrait être *dans* l'image (circulaire : c'est le Dockerfile qu'on ne touche
  pas), et un `LD_PRELOAD` est sans effet sur un binaire statique.
- **`--user <nom>` une fois passwd peuplé** : pour un uid que l'image ne connaît pas, Docker résout le gid
  primaire via `/etc/passwd` et retombe sur **gid 0** — bug silencieux de propriété de fichiers. Écrit
  séparément parce qu'un nettoyage bien intentionné (« maintenant que passwd est peuplé, autant nommer
  l'utilisateur ») est exactement la forme que prendrait la régression.

## Limites acceptées

- **run-shell in-container** peut être *moins* fidèle pour l'inspection statique et perd les outils
  installés éphémères. On garde le wrapping pour l'uniformité + zéro divergence hôte silencieuse.
- Le réveil parasite du watcher de pipeline (la première *lecture* du YAML rapportée comme modification
  externe) est un défaut distinct, volontairement non traité ici : la précondition du pt 4 doit tenir quel
  que soit **qui** avance le Run.

## Relations

- **ADR-0031** : *ce que* le home stagé contient, là où cette ADR fixe *où* et *comment* le Run s'exécute.
- **ADR-0032** : le seam `transcripts_root` du pt 9 a deux consommateurs — le coût et la sonde de fin de
  tour. À lire aussi pour pourquoi la mort d'un nœud sandboxé est exacte par construction : le pane porte
  le client `docker exec`, qui rend la main dès que `claude` sort dans le conteneur.
- **ADR-0020 / ADR-0021** : le conteneur vit de la création au `cleanup_run` (= archive), coextensif à la
  fenêtre d'éligibilité du run-shell ; après un reboot hôte, le run-shell ressuscite le conteneur, car la
  boot recovery saute les Runs terminaux.
