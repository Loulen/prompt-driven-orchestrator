# ADR-0031 — Profils de staging (contenu du home stagé d'un Run sandboxé)

> Statut : accepted (grilling du 2026-07-24, PRD #403). Vocabulaire : CONTEXT.md § « Sandbox ».
> Complète ADR-0030 (modèle d'exécution) : ADR-0030 dit *où* tourne un Run sandboxé, celle-ci dit
> *avec quel contenu de home*. Implémentée par les slices « plancher » puis « profils » : §1 est
> **réalisé en #426** (avec l'amendement §1 d'ADR-0030), **§2-§7 en #432**, **§8 en #468**. Deux
> amendements en fin de document : un point de §6 relit de fait le réglage vivant dans un cas
> borné, et l'un des critères d'acceptation de #432 était factuellement faux.
>
> Le titre est devenu partiellement faux avec §8 : un profil ne décrit plus seulement le
> *contenu du home*, il décrit **le contenu du home et l'environnement d'exécution**. Le nom
> `sandbox_profiles` est conservé — le renommer coûterait une repointe des trois stockages qui
> comparent son nom (cf. « No rename in v1 » dans `sandbox_profile.rs`) pour un gain de prose.

Le contenu du *staged Claude home* cesse d'être une constante Rust invisible. Il devient un
**profil de staging** : une liste nommée, éditable, sélectionnable par Run et par Trigger.

## Ce qu'on décide

1. **Le plancher est une liste de garanties, pas de fichiers verrouillés** *(réalisé en #426)*. Quel que soit le
   profil, `prepare` garantit qu'au démarrage la session dispose de : credentials valides, managed
   settings de l'org **consentis**, bypass permissions accepté, confiance pré-accordée à la racine
   du Run, `projects/` vide. Chaque garantie est satisfaite **soit** par une entrée du profil,
   **soit** par une synthèse de repli. C'est ce qui rend le décochage sûr sans avoir à l'interdire.
   Formulé en fichiers, le plancher se contredirait dès le premier cas réel : `settings.json` est
   copié depuis l'hôte en `full` mais synthétisé à une seule clé en `minimal`, et un utilisateur qui
   décoche le sien (ses hooks hôte n'existent pas dans le conteneur) doit obtenir la synthèse, pas
   un refus.

2. **Un profil est un *diff* contre le défaut, jamais un instantané.** Le stockage retient
   l'intention de l'utilisateur (`disabled: […]`, `extras: […]`), pas la liste effective. Un
   instantané figerait l'install : le jour où une version de PDO ajoute une entrée au défaut — ce
   que le plancher vient précisément de faire — les profils existants ne la verraient jamais.
   Corollaire : `minimal` et `full` sont des **défauts virtuels** (aucune ligne en base) jusqu'à
   édition ; les éditer matérialise une ligne portant elle aussi un diff.

3. **Une entrée est un chemin relatif à `$HOME`.** `.claude/skills`, `.claude/settings.json`,
   `.gitconfig`, `.config/gh`. Refusés : chemin absolu, `..`, toute sortie de `$HOME`, et
   `projects/` sous `.claude` (puits de transcripts runtime : le copier casserait l'idempotence de
   `merge_back` et le calcul de coût). `.ssh`, `.aws`, `.gnupg` sont **autorisés avec
   avertissement** — les interdire serait du théâtre alors qu'ADR-0030 assume déjà l'uid hôte, le
   repo monté rw et de vraies credentials Claude.

4. **Les entrées hors `.claude` sont copiées puis montées, jamais bind-montées depuis l'hôte.**
   `<staging>/home/<chemin>` → `$HOME/<chemin>`, en **rw**. L'invariant « le vrai `~/.claude` n'est
   jamais monté » s'étend au reste de `$HOME`. Un bind direct exposerait l'hôte à l'écriture du
   conteneur : un agent en `--dangerously-skip-permissions` qui bute sur `unable to auto-detect
   email address` fait très naturellement `git config --global`, et réécrit le `~/.gitconfig` de
   l'utilisateur. Les écritures utiles du conteneur (refresh de token `gh`) sont perdues au
   `teardown` — assumé, `merge_back` ne remonte que les transcripts.
   **Dédup obligatoire** : une entrée sous `.claude/` ne reçoit **pas** son propre `-v`, elle est
   déjà servie par le mount `.claude`. Un double bind serait accepté par Docker et résolu par
   profondeur de chemin — un bug de dimanche.

5. **Le champ sandbox reste une valeur unique : `off` ou un nom de profil.** Pas de liste par Run
   ni par Trigger. La précédence existante (`effective_sandbox` : explicite → Trigger → défaut
   d'instance) ne bouge pas, et les sélecteurs du NewRunModal et du panneau Trigger restent des
   `<select>`. L'alternative — le réglage-liste sur les trois tiers — imposerait le widget d'édition
   à trois endroits et une composition de diffs entre tiers dont aucune sémantique n'est devinable.

6. **Le nom du profil ET la liste résolue sont gelés dans `RunStarted`.** `prepare` lit l'état du
   Run, jamais le réglage vivant. `ensure_ready` est appelé à quatre endroits (création, boot
   recovery, résurrection de session, run-shell) et `prepare` est additif — il copie ou écrase, il
   ne supprime jamais. Sans gel, un daemon redémarré après une édition du profil produirait un home
   incohérent entre deux nœuds du même Run, avec un `plugins/` physiquement présent malgré son
   décochage. Le gel de la **liste** en plus du **nom** évite en outre qu'éditer un profil réécrive
   rétroactivement ce qu'un Run passé a stagé. **L'env de §8 est gelé au même endroit et à la même
   création** (clé sœur `sandbox_env`, écrite au même `resolve` : deux lectures pourraient
   enjamber un PUT concurrent et geler une liste d'une révision avec un env d'une autre).

7. **Un nom de profil inconnu échoue fort, partout.** 400 à la création de Run, échec visible du tir
   de Trigger, `RunFailed` explicite en boot recovery. Jamais de retombée silencieuse sur le défaut
   d'instance — le comportement que produirait naturellement le `parse() → None` actuel, et que
   l'ADR-0030 §4 interdit déjà pour l'indisponibilité de Docker. Côté UI, supprimer un profil
   référencé liste ses référents avant confirmation : garde-fou souple, pas d'intégrité
   référentielle en base.

8. **Un profil porte aussi un `env`, posé au `docker create`** *(réalisé en #468)*. Une map
   `{CLÉ: "valeur"}`, posée en `-e CLÉ=valeur` **au create**, à côté des vars run-constantes
   (`HOME`, `PDO_DAEMON_URL`, `PDO_RUN_ID`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`). Cinq
   décisions imbriquées, chacune pour une raison distincte :

   - **Au create et non au `docker exec`.** Ce sont des constantes de *Run*, pas des variables de
     *nœud*. Le chemin `exec` a déjà sa liste par-nœud (`PDO_NODE_ID`, `PDO_NODE_ITER`, le
     catalogue des nœuds `script`) et n'est pas concerné. Conséquence gratuite et voulue : les
     nœuds suivants d'un Run héritent de l'environnement du conteneur, donc §6 est satisfait sans
     code supplémentaire.
   - **Ce n'est PAS un diff** (contrairement à `disabled`/`extras`). Il n'y a pas d'env par défaut
     à fold : PDO pose lui-même ses vars run-constantes, et les trois qu'il possède sont refusées
     comme clés de profil. La map stockée *est* la map effective ; une liste négative répondrait à
     une question que personne ne pose.
   - **Trois clés réservées, refusées par un 400 qui les NOMME** : `HOME`, `PDO_DAEMON_URL`,
     `PDO_RUN_ID`. Jamais un skip silencieux — l'éditeur afficherait `HOME` posé et le conteneur ne
     l'aurait pas. Un `HOME` qui passerait casserait les deux mounts `.claude` / `.claude.json`
     d'un coup, puisque tous deux sont calculés depuis lui. La liste est possédée par le module qui
     pose les `-e` (`sandbox_container`), consommée par le validateur : un seul littéral, comme
     pour `daemon_url` (#447). `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` n'est
     **volontairement pas** réservé — le surcharger est un usage légitime — et il est alors remplacé
     *en place*, pour qu'aucune clé ne soit posée deux fois (« laquelle gagne » dépend de la couche
     qui lit `environ` ; un invariant d'isolation ne peut pas dépendre de ça).
   - **Les VALEURS ne sont jamais loggées, les noms oui.** Le `info!` d'`ensure_ready` liste les
     entrées de staging en clair : un chemin relatif à `$HOME` n'est pas un secret, une valeur d'env
     l'est souvent. Le journal systemd survit au Run *et* au profil, donc une fuite y est un
     incident irréversible. Le rendu autorisé passe par une fonction nommée (`env_names`) pour que
     la règle soit testable au lieu d'être une consigne.
   - **Ce n'est pas un coffre-fort, et l'UI l'écrit.** Les valeurs atterrissent en clair dans
     SQLite, dans le payload `run_started` gelé (donc dans le fichier d'événements du Run) et dans
     `docker inspect --format '{{.Config.Env}}'`. Le sandbox n'étant pas une frontière de sécurité
     (ADR-0030), c'est cohérent ; les masquer dans l'éditeur serait du théâtre, et pire : ça
     laisserait croire que PDO protège quelque chose. La phrase est donc du texte **porteur**, pas
     un avertissement de forme — sans elle quelqu'un y met une clé API en croyant à un secret store.

   Ce que ça débloque : un serveur MCP dont le `.mcp.json` est fourni par un plugin est hors du
   contrôle de PDO et ne se configure que par variables d'environnement
   (`PUPPETEER_EXECUTABLE_PATH`, `CHROME_PATH`, proxys d'entreprise, endpoints clients). Sans §8, le
   profil décrivait le contenu du home et **rien** de l'exécution.

## Pourquoi (ce que le mode seul ne pouvait pas faire)

Le mode est un interrupteur à deux positions qui décide de *tout* d'un coup — skills, plugins,
agents, commands, settings, `.md` globaux. Or le poste de coût est **un seul** de ces éléments :
`full` pèse ~1 Go par Run, « dominé par `plugins/*/node_modules` », et le staging n'est purgé
qu'au `cleanup_run`. Un pipeline qui a besoin des skills mais pas des serveurs MCP n'avait aucune
option : il payait 1 Go ou il perdait tout. Sur une instance à Triggers horaires, ce choix
binaire alimente directement la récurrence disque connue.

Symétriquement, le staging ne pouvait transporter **que** du `~/.claude`, alors que ce qui manque à
un Run sandboxé pour faire le travail réel est ailleurs dans `$HOME` : l'identité git est globale
(`~/.gitconfig`), donc un agent qui commite dans le conteneur échoue ; l'auth `gh` vit dans
`~/.config/gh`. Le profil résout les deux besoins avec un seul concept.

## Alternatives écartées

- **Extras seulement, sans décochage.** Simple, mais ne règle pas le poste de coût — le seul
  élément qu'on veuille vraiment retirer est dans le défaut.
- **Override complet de la liste.** L'utilisateur réécrit tout ; le plancher est réinjecté. Plus
  souple, moins découvrable, et prive l'install des évolutions futures du défaut.
- **Réglage-liste sur les trois tiers** (Run / Trigger / instance). Écarté en §5.
- **Manifeste dans le staging dir** plutôt que gel dans `RunStarted`. Évite un champ de payload,
  mais éclate le contrat d'isolation d'un Run sur deux stockages, et disparaît au `teardown`.
- **Interdire `.ssh`/`.aws`/`.gnupg`.** Incohérent avec la posture v1 d'ADR-0030 (cf. §3).
- **Env posé au `docker exec` plutôt qu'au create** (§8). Ce serait traiter une constante de Run
  comme une variable de nœud : il faudrait la re-passer à chaque session, et le gel de §6
  demanderait alors du code, alors qu'au create il est gratuit (Docker ne réévalue pas l'env d'un
  conteneur existant).
- **Env chiffré / intégration à un secret store** (§8). Écarté pour v1, et pas par paresse : le
  sandbox n'est pas une frontière de sécurité (uid hôte, repo rw, vraies credentials Claude), donc
  un chiffrement au repos protégerait la base contre un attaquant qui a déjà le conteneur. Dire la
  vérité dans l'UI est plus honnête que du chiffrement décoratif. Un vrai secret store devient
  défendable le jour où ADR-0030 change de posture.
- **Env sur les trois tiers** (Run / Trigger / instance), comme le nom du profil. Même argument que
  §5 : le widget d'édition à trois endroits, plus une composition inter-tiers indevinable — et ici
  s'ajoute la question « une clé du tier bas est-elle écrasée ou fusionnée ? », dont aucune réponse
  n'est évidente.

## Limites acceptées

- Le blast radius filesystem n'est plus « rien d'autre que `.claude` » mais « ce que le profil
  déclare ». Le refus par défaut de `$HOME` devient une **liste d'exceptions déclarées et
  visibles** — voir l'amendement d'ADR-0030.
- Les profils vivent en base, pas sur disque : ils ne se versionnent pas avec le repo. Assumé, un
  profil référence des chemins spécifiques à la machine. Le Dockerfile, lui, reste sur disque
  précisément parce qu'il est fait pour être partagé.
- Une édition de profil ne rattrape pas les Runs en vol (conséquence directe du gel, §6).
- **Les valeurs d'env sont en clair, dans trois stockages** (§8) : SQLite, le fichier d'événements
  du Run, `docker inspect`. Le fichier d'événements est immuable, donc retirer une valeur du profil
  ne la retire pas des Runs qui l'ont gelée : la seule remédiation est de faire tourner le secret.
  C'est le prix du gel de §6, et c'est pourquoi l'UI dit que ce n'est pas un coffre-fort.
- **`env` n'est pas un diff** (§8), donc il ne bénéficie pas de la forward-compatibility de §2 : si
  une version future de PDO posait une var par défaut, un profil ne pourrait pas la « décocher », il
  faudrait la surcharger. Acceptable tant que PDO ne pose que des vars run-constantes — qui sont,
  elles, réservées.

## Relations

- **ADR-0030** — modèle d'exécution ; amendé pour les mounts d'exception `$HOME` et l'échec fort.
- **ADR-0015** — précédence `stored → env → default` des réglages d'instance ; les défauts virtuels
  `minimal`/`full` en sont l'application à une valeur non scalaire.
- **ADR-0001** — outil tranchant, pas outil sûr : fonde le choix « autoriser + avertir » (§3).
- **#403** — PRD Sandbox ; ces décisions sont livrées par les slices post-validation du PRD. §1 est
  livré par **#426**, §2-§7 par **#432**, §8 par **#468**.
- **#447** — un fait, un propriétaire (le résolveur unique de `PDO_DAEMON_URL`) : c'est le précédent
  qui fait posséder la liste des clés réservées de §8 par `sandbox_container`, pas par le validateur.

## Amendements (#432, à la livraison)

### A1 — §6 : un Run d'avant les profils relit le défaut vivant

Le gel dit « `prepare` lit l'état du Run, jamais le réglage vivant ». Une ligne de la table de
décision au replay y contrevient, **sciemment** : un payload `RunStarted` qui porte
`sandbox: "full"` (ou `"minimal"`) **sans** `sandbox_entries` fait **re-résoudre le défaut virtuel
maintenant**. Ce cas n'est atteignable que pour un Run créé par un daemon pré-#432, dont le staging a
été purgé, puis repris. Les deux alternatives sont pires : `RunFailed` sur un Run parfaitement
résoluble, ou figer dans le Rust pour toujours le défaut tel qu'il était en #426 — ce qui
contredirait §2. Toutes les autres lignes de la table respectent le gel à la lettre, et un nom
d'**utilisateur** sans liste gelée échoue dur (il est injoignable par construction : le chokepoint
unique écrit les deux clés ou aucune).

### A2 — le critère « `git config --global` modifie la copie stagée » était faux

Le corps de #432 affirmait : « `git config --global` **depuis le conteneur modifie la copie stagée**,
`~/.gitconfig` reste intact ». La seconde moitié est vraie et **doublement garantie** (copie-puis-mount
de §4, *et* l'échec net ci-dessous). La première est **fausse**, vérifié en Docker réel
(`pdo-sandbox:h-9a67637571a4`, `--user 1000:1000`) :

```
git commit                            -> OK, sous l'identité de l'hôte
git config --global user.email x@y    -> error: could not lock config file
                                         /home/probeuser/.gitconfig: Permission denied
ls -ld $HOME                          -> drwxr-xr-x 2 root root
```

Cause : `$HOME` **n'existe pas dans l'image** (`ubuntu:24.04` livre `/home/ubuntu`, pas
`/home/<user hôte>`), donc Docker le crée comme parent des mounts, en `root:root 0755`.
`git config --global` n'écrit pas en place — il crée `$HOME/.gitconfig.lock` puis renomme, et le
rename exige un `$HOME` inscriptible. **Condition pré-existante depuis #406**, déjà vraie pour la
liste `full` : ce n'est pas une régression des profils.

Le critère est donc reformulé en deux affirmations vraies et testées :

1. « `~/.gitconfig` de l'hôte n'est **jamais** muté » — assertion couche 3a (aucun chemin hôte réel
   en source de mount) + contrôle d'octets avant/après le Run ;
2. « une écriture du conteneur sous `$HOME` atterrit dans la copie stagée » — prouvée sur une entrée
   **répertoire** (`.config/gh`), qui est inscriptible (le mount porte l'ownership de la source
   stagée). Seul le motif *lock-file-puis-rename* est bloqué, et `git config` est exactement ça.

Rendre `$HOME` inscriptible dans le conteneur est un **arbitrage produit** : ça touche les identity
mounts d'ADR-0030 §1 et casse la propriété « queue de mounts vide ⇒ argv byte-identique à #406 ».
Hors périmètre de #432, à ficher en suivi. Sans lui, `git config --global`, `gh auth login` et tout
outil qui crée un dotfile *nouveau* échouent dans le conteneur.
