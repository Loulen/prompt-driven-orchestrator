# Changelog

Ce fichier ne consigne que les changements **cassants** et les notes de version qui ne se
déduisent pas d'un titre de commit. L'historique complet vit dans le git log et dans les
[Releases GitHub](https://github.com/Loulen/prompt-driven-orchestrator/releases), générées
depuis les commits.

Le projet suit le versionnement sémantique. Il n'a **aucun objectif de compatibilité
ascendante** : la casse se signale ici et par un bump majeur, jamais en gardant des champs
morts. Seule contrainte non négociable — les **données historiques restent lisibles** : un Run
archivé s'ouvre et se chiffre quelle que soit la version qui a écrit son payload.

## 1.54.0

**Import de skills depuis une Source** (#670 ; story #666, spec #667). Depuis la banque, *+ Add ▾ ›
Import from a source…* accepte une URL de dépôt GitHub (racine, branche, `/tree/<branche>/<chemin>`),
une URL SSH ou un dossier local : PDO clone en shallow avec les credentials git de l'utilisateur du
daemon, scanne récursivement les `SKILL.md`, valide chaque frontmatter et présente une liste cochable
(invalides grisés avec la raison, collisions de nom à résoudre explicitement : remplacer / renommer /
ignorer — rien n'est écrit tant qu'un choix manque). Les skills cochés atterrissent dans un **dossier
Source** nommé d'après la Source, qui porte sa provenance (URL, ref, commit, chemin) ; chaque skill
importé garde la sienne, même déplacé. *Update from source…* re-scanne, montre le diff (mis à jour,
inchangé, nouveau à la source, sorti du dossier, disparu) et met à jour après confirmation. Les
fichiers de référence sont copiés intégralement. Endpoints : `POST /settings/skills/scan`,
`/import`, `/settings/skill-folders/{id}/rescan`, `/update`. Schéma : la provenance des skills passe
de `(source, source_commit)` à un objet `{url, ref, commit, path}` (colonnes additives, base 1.51
lisible telle quelle).

Retouches après le Feature Path #670 : *Update from source* ne touche que les skills du dossier
ciblé (deux dossiers importés du même dépôt ne s'écrasent plus l'un l'autre ; un skill de la même
source rangé ailleurs est signalé « already in “<dossier>” ») ; le clone d'un scan est isolé par
processus daemon ; un skill « same commit » coché propose remplacer / renommer / ignorer au lieu
d'échouer à l'import ; la modale avertit quand un dossier homonyme existe déjà à la destination et
propose d'importer dedans ; `Esc` ferme la modale après un import partiel ; le diff d'update libelle
« N reference files changed ».

## 1.53.0

**Skills : sélection par tier et skills effectifs avec origine** (#669 ; story #666, spec #667 ;
ADR-0062). Un skill se coche à trois niveaux — Instance (réglages), Projet (fiche projet) et Nœud
(inspecteur du pipeline) — et un lancement de Run hérite des deux premiers, avec des skills RUN
ajoutés à la volée (cocher un dossier coche ses skills). L'inspecteur d'un nœud affiche la liste des
*skills effectifs* avec l'origine de chacun (INSTANCE / PROJECT / NODE / RUN) et la liste figée au
spawn (`NodeStarted.skills`). Supprimer un skill de la banque liste ses référents (projets, nœuds) ;
les références orphelines gardent l'id, s'affichent barrées avec un avertissement (inspecteur, bandeau
de lint du pipeline, modale New Run) et le Run se lance quand même, le skill étant ignoré
(`missing_skills`). API : `GET /settings/skills/{id}/referents`.

## 1.52.0

**Fichiers de référence d'un skill** (#671 ; story #666, spec #667 ; ADR-0062). Un skill peut
embarquer des fichiers à côté de son `SKILL.md` : glisser-déposer dans la modale de collage
(fichiers stagés avant la création, « Create skill + N files ») ou dans l'onglet *Files* du détail,
explorateur multi-sélection (chemin hôte), suppression avec confirmation inline, édition texte brut
avec sauvegarde explicite (`⌘S`). Un `SKILL.md` déposé remplace le texte courant (annulable). Limite
10 MB par fichier, sous-dossiers conservés, chemins traversants refusés. API REST sous
`/settings/skills/{id}/files`.

## 1.51.0

**Banque de skills** (#668 ; story #666, spec #667 ; ADR-0062). L'instance gère une banque de
skills depuis *Instance settings › Manage skills…* : création par collage d'un `SKILL.md`
(frontmatter validé en direct — nom kebab-case, description obligatoire —, refus sans écriture
disque), rangement en dossiers par glisser-déposer, renommage inline (unicité insensible à la
casse) et suppression précédée d'un inventaire des référents (instance, projets, pipelines, runs).
Chaque skill vit sous `<repo>/.pdo/skills/<id>/SKILL.md`, à côté de `pdo.db`. API REST sous
`/settings/skills`.

## 1.50.0

**Provisionnement déclaratif des worktrees** (#630 ; ADR-0061). Un worktree de Run ou de Node
isolé reçoit les ressources du dépôt primaire qu'un checkout Git neuf ne contient pas
(`.env`, `node_modules`, modèles, fixtures…), déclarées en patrons `.gitignore` sur trois listes —
copie, lien physique, lien symbolique — aux niveaux Instance, Projet, Run et Node isolé. Les
règles se composent de manière additive ; le mode le plus spécifique l'emporte ; un conflit de
mode au même niveau est refusé avant le lancement. La recette est **gelée par Run** : un Node
redémarré sur la même itération garde son sous-worktree et ses éditions. Une erreur de
provisionnement interrompt le Run avant tout spawn et nomme le chemin, le mode et la cause.

## 1.49.0

**Un seul contrat de livraison pour les Agents et les Scripts** (#654 ; ADR-0060). *Cassant.*
Un Node ne reçoit plus aucune consigne Git : il modifie des fichiers, et le runtime livre ce
qu'il laisse. À la complétion, PDO conserve les commits que le Node a pris lui-même, stage le
reste avec la sémantique de `git add -A` et crée **un** commit `<node-id> iter-<N>: completed`
sous l'identité Git configurée, sans trailer. Un Node isolé voit ensuite son sous-worktree
mergé dans la branche du Run ; un Node non isolé a déjà commité sur place. Aucun changement
restant ⇒ aucun commit, donc jamais de commit vide.

Cette livraison est **une seule opération** que traversent les quatre chemins de complétion :
la complétion automatique (hook `Stop` et veille de fin de tour), `pdo complete`, la complétion
manuelle (`mark_node_done`) et la fin d'un Script. Elle s'exécute **avant** l'événement terminal,
donc avant tout départ de l'aval : un Node isolé forké ensuite voit le travail d'un Node non
isolé amont. La complétion manuelle ne faisait auparavant ni merge ni commit — l'asymétrie
qu'ADR-0035 consignait comme limite acceptée disparaît.

**Le refus `doc_violated_code_immutability` est supprimé, avec toutes ses surfaces.** Un Node non
isolé qui modifie des fichiers suivis n'est plus refusé : son travail est livré. Le slug disparaît
du corps de refus, de l'`awaiting_reason_code`, du message de `pdo complete` et du client. Le
refus `merge_failed` est remplacé par **`delivery_failed`** (`500`), qui couvre désormais le
staging, le commit et le merge : il appende un `NodeInterrupted`, parque le Run `awaiting_user`
et **conserve le travail sur disque**, sans rien annuler ni nettoyer.

**Le staging n'exclut rien.** PDO ne retire aucun de ses propres chemins runtime : le `.gitignore`
du dépôt cible est l'unique politique d'exclusion. Un dépôt qui n'ignore que `.pdo/runs/` verra
donc le blackboard (`.pdo/artifacts/`, `.pdo/prompts/`) entrer dans les commits de livraison de
ses Nodes non isolés — ignorez `.pdo/` comme le fait ce dépôt.

**Nouvel événement `node_delivered`** (`before` / `after`, les deux têtes de la branche du Run),
projeté sur `nodes.<id>.delivery`. Le **diff par NodeRun** s'y appuie : il existe désormais pour
tout Agent ou Script qui a livré des changements, isolé ou non, et n'est plus réservé aux Nodes
qui possédaient une branche `pdo/sub-*`. Le sélecteur de diff de l'UI liste ce que les NodeRuns
ont livré, plus ce qu'ils sont. Comme le diff du Run, il exclut `.pdo/`.

**Posture d'outil tranchant conservée.** Plusieurs Nodes non isolés peuvent tourner en même temps :
le runtime n'ajoute aucune sérialisation. Le premier qui complète commite tout l'état non ignoré
alors visible, sous son propre message — l'isolation reste le moyen d'obtenir une attribution
fiable.

## 1.48.0

**L'isolation voyage par la bibliothèque et par l'import** (#655 ; ADR-0060). Une entrée de
bibliothèque `agent` ou `script` porte désormais `isolated_worktree`, l'instanciation le restitue
tel quel, et l'aperçu de la bibliothèque nomme le workspace de chaque entrée. Sans ça, une entrée
étoilée retombait sur le défaut de son type à chaque dépôt sur le canvas : un Agent garé dans le
worktree du Run en forkait un à lui, en silence.

L'isolation entre du même coup dans l'identité de l'entrée. **Conséquence à l'upgrade** : une
entrée écrite avant cette version ne dit rien de son workspace ; elle se lit au défaut de son type
(Agent isolé, Script partagé) — donc rien ne bouge pour la majorité des entrées, mais un Node que
vous aviez sorti de son isolation lira `out of sync` face à son entrée jusqu'à ce que vous la
mettiez à jour. C'est la divergence réelle, pas un faux positif.

Import de workflow : tout rôle importé, placeholder annoté compris, devient un `agent` **isolé** et
le brouillon écrit la ligne. L'import ne déduit jamais l'isolation du prompt, du nom du rôle, de ses
sorties ni de son appartenance à une région `collection`.

## 1.47.0

**`agent` remplace `doc-only` et `code-mutating` ; l'isolation devient explicite** (#653 ;
ADR-0060). *Cassant.* Les deux anciens types nommaient un *effet* alors que le runtime n'y lisait
qu'un répertoire de travail. Un seul type agentique subsiste, `agent`, et l'endroit où le NodeRun
travaille s'écrit sur le Node : `isolated_worktree: true|false`.

La rupture est franche — **ni alias, ni migrateur, ni diagnostic dédié**. `doc-only` et
`code-mutating` prennent le chemin de n'importe quelle valeur invalide : coercition vers `agent`
avec l'avertissement générique de type inconnu, donc un Node qui perd son sous-worktree sans le
dire. **Les Pipelines existantes se convertissent à la main** : un ancien `doc-only` devient un
`agent` avec `isolated_worktree: false`, un ancien `code-mutating` un `agent` avec
`isolated_worktree: true`. Les Pipelines livrées dans ce dépôt sont réécrites ; **celles de
l'instance (`~/.pdo/pipelines/`) ne le sont pas** — elles sont hors du dépôt.

Le Document écrit toujours le choix pour un `agent` et un `script`, même à la valeur par défaut
(Agent isolé, Script partagé) : on lit où le Node travaille au lieu de se rappeler un défaut.
`merge` reste isolé d'office et n'expose aucun réglage ; `start` et `end` n'en portent aucun.
L'isolation est **gelée au spawn du NodeRun** — une édition déplace le prochain lancement, jamais
une exécution vivante, et une reprise retrouve le répertoire qu'elle avait quitté.

Côté éditeur, l'inspecteur remplace le sélecteur de type par un type figé et une section
« Workspace » qui nomme les deux lieux et affiche le répertoire résolu ; le canvas remplace les deux
marqueurs `doc-only` / `code-mutating` par un glyphe de branche sur les Nodes qui forkent un
worktree (l'`agent` isolé et le `merge`). L'import de workflow ne devine plus : un rôle importé
devient un `agent` isolé, sans heuristique de prompt, de nom ni de sortie.

## 1.46.0

**Les pipelines appartiennent à l'instance et voyagent par document** (#572 ; ADR-0059). *Cassant.*
La distinction `repo` / `user` / `library` disparaît de toutes les surfaces — liste, édition,
sauvegarde, suppression, lancement — et `GET /library/pipelines` n'existe plus. Les pipelines des
anciens emplacements (`.pdo/pipelines` d'un dépôt, pipelines utilisateur, `.pdo/library/pipelines`)
sont migrés une fois au boot vers le registre de l'instance, collisions renommées, prompts inclus ;
rien ne relit ensuite les anciens fichiers.

Le partage devient explicite : l'onglet YAML d'une Pipeline — et d'un Run — expose un **Document de
pipeline transportable** (`pdo_pipeline: 1`) avec copie et téléchargement, que la modale d'import
reconstitue en une pipeline indépendante. Le document embarque le graphe, les positions, les notes,
les variables et les prompts, développe les nodes partagés en nodes ordinaires, et n'emporte ni
secret, ni environnement, ni valeur d'exécution, ni configuration d'instance : un profil agentique
nommé revient à **Inherit**. Un document invalide ou de version inconnue est refusé sans rien créer.

## 1.45.0

**Un seul fold de coût attribué pour le Run, ses Nodes et Stats** (#647 ; ADR-0058). Les coûts des
subagents Claude et Copilot appartiennent au Node parent et chaque message Claude est dédupliqué à
l'échelle du Run. `GET /runs/:id` expose désormais un coût dérivé à la lecture sur chaque Node
agentique ; une valeur inconnue reste `null` et s'affiche « — », jamais zéro.

## 1.37.0

**Assistant de bibliothèque : un seul assistant, focus par message, reap par inactivité** (#594 ;
ADR-0051, qui amende ADR-0048 §1/§3/§4). Aucune migration de données ; la colonne SQLite
`libassist_idle_ttl_secs` est ajoutée par un `ADD COLUMN` idempotent au boot, comme ses voisines.

L'unité de durée de vie de l'assistant était la mauvaise : un `claude` par pipeline, vivant le temps
où l'onglet **Assistant** était affiché. Elle devient **l'humain** : un assistant pour tout le
daemon, qui apprend à chaque message quelle template est ouverte, et qui meurt quand plus personne
n'édite.

Changements cassants sur l'API HTTP :

- **`POST` / `DELETE /sessions/{pipeline_id}/libassist` → `POST` / `DELETE /sessions/libassist`.**
  L'id de pipeline disparaît du chemin, et le paramètre `?scope=` avec lui : il n'y a plus qu'une
  session, `pdo-libassist-shared` au lieu de `pdo-libassist-<id>`. Un client qui appelle l'ancienne
  route reçoit un 404.
- **Nouveau `PUT` / `GET /sessions/libassist/focus`.** L'UI y déclare `{pipeline_id, scope}` — jamais
  un chemin, que le daemon résout lui-même — et le répète toutes les 20 s tant qu'une vue d'édition
  est ouverte. `GET` sert la même chose en JSON, ou en une phrase avec `?format=text` (la forme que
  le hook `UserPromptSubmit` de l'assistant injecte dans son contexte à chaque message).
- **Nouveau `POST /sessions/libassist/save`.** Corps `{yaml, prompts}` — **ni id, ni scope** : le
  daemon écrit dans le fichier que le focus désigne, puis diffuse le `pipeline_changed` qui fait
  relire le canvas. C'est désormais le **seul** chemin d'écriture de l'assistant ; `POST
  /library/pipelines` reste celui de la Library de l'UI et n'est plus documenté à l'assistant.
- **`GET` / `PUT /settings` exposent `libassist_idle_ttl_secs`** (`stored → env
  `PDO_LIBASSIST_IDLE_TTL_SECS` → 120 s), le délai au bout duquel le sweep ramasse un assistant sans
  terminal attaché et sans édition en cours.
- **`DELETE /sessions/libassist` vide aussi le focus.** Un client qui envoyait `PUT focus: null` puis
  `DELETE` peut se contenter du `DELETE`.

Changements de comportement visibles :

- **Une seule conversation, partagée par toutes les templates.** Passer d'une pipeline à l'autre ne
  jette plus l'historique — c'est le gain ; en contrepartie il n'y a plus d'isolation entre deux
  templates éditées en alternance, et l'assistant se resitue via le focus à chaque message.
- **Le sweep peut désormais tuer l'assistant** (`LibAssistIdle`), ce qu'ADR-0048 §4 lui interdisait.
  Trois verdicts dans l'ordre : session attachée → gardée ; focus frais → gardée ; sinon tuée après
  la TTL d'inactivité. C'est ce filet qui borne enfin le cas réellement cassé — un reload ou une
  fermeture d'onglet n'envoie aucun `DELETE` (React ne joue pas ses cleanups au déchargement), et la
  session survivait donc sans borne.
- **Le cwd de l'assistant devient `<repo>/.pdo/pipelines`** au lieu du *library store*. C'était un
  bug : un onglet de scope `repo` ou `user` édite un fichier de `.pdo/pipelines/`, où le `<id>.yaml`
  annoncé par le primer n'existait pas. Le chemin réel du fichier édité arrive maintenant par le
  focus, en absolu.
- **Le save de l'assistant écrit dans le fichier ouvert, point.** Le mot *scope* désigne deux arbres
  différents — `.pdo/pipelines/` pour un onglet d'édition, `.pdo/library/pipelines/` pour le library
  store — et faire porter ce mot par l'assistant le faisait écrire un doublon dans le mauvais, laisser
  le fichier édité intact et annoncer « Sauvé ». L'argument disparaît, la classe de bug avec.
- **Aller voir un Run ne reape plus l'assistant** tant qu'un onglet de template reste ouvert. « Quitter
  toute vue d'édition » se lit sur les onglets ouverts, pas sur l'onglet actif.

Dégradation assumée : le hook `UserPromptSubmit` n'existe que sur un harnais dont le gabarit de lancement
expose `--settings` (le `claude` de la registry ; ni `opencode`, ni `pi`). Ailleurs, la consigne
équivalente du primer — aller lire le focus avant d'agir — est le seul mécanisme, et le daemon le
dit en `warn!` au spawn.

## 1.35.0

**Résilience des runs — observabilité & véracité de l'état** (#601, lot 4/4 de la spec #596 ;
ADR-0025/0035/0037/0038/0049/0050). Aucune migration : le nouveau champ d'état est additif
(`skip_serializing_if`), les logs historiques restent lisibles (le code machine se dérive du préfixe
de prose quand il manque).

L'état d'un run devient **auto-diagnostiquable** : la raison de tout non-avancement vit dans l'état,
la détection de stall est exhaustive par construction, et les entrées d'API disent la vérité.

Changements de comportement (non cassants sur les lecteurs de payload historiques, mais visibles des
clients d'API) :

- **Raison machine + prose sur tout non-avancement.** Un park / give-up / `Interrupted` / `unrouted`
  porte désormais un **`awaiting_reason_code`** (slug stable : `session_died`, `spawn_aborted`,
  `boot_recovery`, `run_stalled`, `unrouted`, `region_exhausted`, `region_ended_unrouted`,
  `merge_conflict`, `merge_resolution_failed`, `merge_resolver_spawn_failed`,
  `script_validation_failed`, `frontmatter_retry_exhausted`, `doc_violated_code_immutability`,
  `agent_fail_awaiting`) **à côté** de la prose `awaiting_reason` — même contrat slug+prose qu'un
  refus (ADR-0035). Exposé sur `GET /runs/:id` **et** `GET /runs` (l'entrée de liste porte enfin
  `awaiting_reason`), lu par le manager (préambule) et l'UI. Plus besoin de `journalctl`.
- **`run_stall_reason` exhaustif par construction.** L'attente sur région ouverte passe par un
  `match` sans joker sur `RegionStateKind` (loop / foreach / collection) : un futur type de région
  ne peut plus rouvrir un run figé en `stalled=false` (fin de la classe #453).
- **`loop_states` non ambigu.** Une région bornée a une entrée dès le **lap 1** (comme le nœud
  `Loop` legacy) : « pas d'entrée » signifie « pas de boucle », plus « premier tour » (amende la
  mise en garde ADR-0025 §4, répercutée dans le préambule manager).
- **`POST /runs` : champ inconnu → `400` nommant le champ**, avant tout effet (JSON *et* multipart).
  Fin du succès silencieux qui jetait un champ mal orthographié (ex. `target_repos`). Doctrine
  ADR-0033 (validation à l'écriture), volontairement plus étroite que l'ignorance des champs de
  *config* d'ADR-0015 #471 (validation explicite, pas de `deny_unknown_fields`).
- **Corps d'erreur de `/commands` normalisés en JSON** (#491) — plus de `text/plain` avalé par le
  `.catch(() => null)` du front (`load_projected`, échecs d'append, sondes `restart_node`,
  `cleanup_run`). `{ "error": … }` partout.
- **Commande valide mais sans effet → `200 {noop, reason}` honnête** (déjà porté par le socle via la
  porte de complétion partagée ; couvre `skip` d'un nœud déjà terminal).

## 1.34.0

**Résilience des runs — lot 3/4 : débloquer un run coincé sans désamorçage** (#600, spec #596,
ADR-0011/0025/0038/0049, Sharp tool ADR-0001). Aucune migration, données historiques lisibles :
les nouveaux champs de projection (`region_max_iter_overrides`, `forced_routes`) se déduisent du log
append-only et sont **absents du fil** tant qu'aucune commande ne les pose (runs existants sérialisés
à l'identique) ; le port `required:` et le marqueur `skipped:` sont eux aussi rétro-compatibles.

De nouvelles primitives de pilotage, toutes exposées sur `POST /runs/:id/commands` (l'humain force,
PDO obéit — Sharp tool) :

- **`set_region_max_iter(region_id, max_iter)`** — relève le plafond d'une région bornée **en vol**,
  valeur **absolue** (≠ le delta de `bump_region`), **uniforme** pour un cap littéral et un cap `$var`.
  Le scheduler lit l'override (folded depuis le log) à la place du `max_iter` déclaré, donc la région
  repart pour N tours sans éditer le YAML ni redémarrer, et l'effet tient après une ré-ouverture.
- **`force_route(from, target)`** — sortie explicite d'un **node** ou d'une **région** vers une cible,
  qui **court-circuite les `when:`** des edges. Le lever d'un run bloqué `unrouted` (verdict non-`PASS`,
  CI verte, MR mergeable) : `force_route <reviewer> -> End` fait atteindre Finalize sans amender le
  verdict à la main sur tous les iters. Folded depuis le log ⇒ **non re-décidé** par les `when:` au
  tour suivant ni après une ré-ouverture.
- **`skip_node(node_id, [iter], [overrides])`** — **skip local** : marque un node satisfait avec un
  **output vide par défaut** (surchargeable par port via `overrides`), le run **continue** (jamais de
  `RunSkipped` qui tuerait tout, contrairement à `pdo skip`/#245). Compté satisfait pour la
  re-projection : une ré-ouverture ne le re-spawn pas.
- **`overrides` sur `start_node`** (#486) — lance un node avec un **input factice** par port (contenu
  inline, écrit en artefact), sans attendre l'upstream. Couvre aussi les ports **émergents** (nodes
  `doc-only`/`code-mutating`/`script` sans `inputs:` déclarés).

Et deux comportements du moteur :

- **Atteignabilité + auto-skip** (#589) — un input `required:` (nouveau champ de port) **structurellement
  inatteignable** (branche either/or non prise) fait **auto-skip** le node avec une **raison** dans
  l'event, au lieu de le laisser pendre ; l'aval avance sur l'output vide. Balayage borné (`advance_run`),
  qui cascade proprement (skip d'un node ⇒ ré-évaluation d'un either/or aval).
- **Diagnostic `unrouted` enrichi** (AC4) — la raison portée dans `awaiting_reason` **liste les edges
  candidats**, leur garde (`when:`/`else`), s'ils ont firé, et **la valeur réellement lue** pour chaque
  champ testé (`verdict=minor_changes` …). Lisible depuis l'état du run, sans `journalctl`.
## 1.33.0

**Résilience des runs — récupérer un node `Interrupted` & sous-worktree résilient à l'environnement**
(#599, lot 2/4 de #596, ADR-0049/0050/0045/0036). Aucune migration, données historiques lisibles
(log append-only ; un Run pré-#599 se ré-attache ou restart comme avant).

Récupération d'un node `Interrupted` par deux mécanismes, déclenchés à la main (ADR-0049 §3) :

- **Ré-attache de session** (optimal) — nouvelle commande **`recover_node`** : reprend la **même**
  session dans le sous-worktree existant (`claude --continue`), sans relancer le run.
  **Conditionnée à une capacité déclarée du harnais** (`HarnessDescriptor::can_resume()`, ADR-0045) —
  la mécanique de reprise (exclusion `script`, réservation d'admission #487 §3, trace `NodeStarted`)
  est désormais partagée avec `GET …/pane` (`reattach_node_session`).
- **Repli automatique** — si le harnais ne sait pas reprendre, `recover_node` retombe **tout seul**
  sur le **restart-avec-artefacts** (décision pure `recovery::choose_recovery`) : un agent frais
  reçoit les artefacts partiels du node en **input**, jamais réécrits par-dessus. Le préambule du
  spawn expose la sortie partielle survivante (`## Partial output from an interrupted attempt`) —
  elle n'est jamais wipe sur un re-spawn de même itération.

Sous-worktree résilient à ce que l'agent a fait à son git (Sharp tool, ADR-0001) :

- **`classify_sub_worktree`** : un worktree à **notre propre chemin** sur une branche nommée
  ≠ `pdo/sub-*` (l'agent a fait `git checkout -b feature/…`) est désormais **`Reusable`**, plus
  `Occupied`. `Occupied` reste réservé à « la branche est checkoutée dans un **autre** worktree
  vivant » (ADR-0050 §3).
- **Merge-back suit le HEAD réel** du sous-worktree (`node_tip`), plus le **nom** `pdo/sub-*`
  (ADR-0036 amendé) : le travail commité sur `feature/…` entre dans la branche pipeline au lieu
  d'être perdu par un « Already up to date » silencieux.

Déjà couverts par le socle #598/#489/#516, confirmés ici : le reap d'une branche `pdo/sub-*`
survivante avant recréation (fin de #498), et l'**inventaire** d'une opération git interrompue
(`index.lock`, `MERGE_HEAD`, `rebase-merge/`) dans la réponse du restart **et** le préambule du
re-spawn — jamais supprimée en aveugle (#516).

## 1.32.0

**Résilience des runs — socle « retomber sur ses pattes »** (#598, ADR-0049/0050, ADR-0032/0009/0036
amendés). Aucune migration, données historiques lisibles (log append-only, `resume_run` reste projeté).

Changement de comportement, non cassant sur le fil : **le runtime ne déclare plus jamais forfait de
lui-même**. Un incident infra (mort de session, boot recovery, spawn-abort) met le node en
`Interrupted` (nouveau statut de node, **non terminal**, distinct de `Failed`) et le run en
`AwaitingUser` avec une **raison** (`awaiting_reason`), jamais `RunFailed`. Les give-up runtime
(stall run-level, refus de validation d'output, conflit de merge, `unrouted`) parkent de même en
`AwaitingUser`. `Failed` ne provient plus que d'un `pdo fail` **délibéré** d'un agent (opt-in
`auto_fail`) ou d'un abandon humain.

Notes qui ne se déduisent pas du titre :

- **`auto_fail`** — opt-in résolu `nœud < Run < Projet < instance` (défaut décoché) : décoché, un
  `pdo fail` d'agent parke le run pour confirmation humaine ; coché, il termine direct en `Failed`.
  Ne concerne QUE le `pdo fail` d'agent ; tout give-up runtime parke quoi qu'il arrive. Colonnes
  `auto_fail` ajoutées (idempotent) à `instance_config` et `projects` ; clé gelée dans `RunStarted`
  et lisible par nœud (`auto_fail:` YAML). Env `PDO_AUTO_FAIL`.
- **Ré-ouverture** — `terminal ≠ verrouillé`. `reopen_run` (bouton **Play** de la toolbar de niveau
  Run) re-projette n'importe quel run terminal (`Completed`/`Skipped`/`Failed`/`Halted`) vers
  `Running` : les `(node, iter)` satisfaits restent gelés (jamais re-spawnés, anti-#221), seul le
  travail non satisfait repart. Les commandes ciblées (retry/restart/start/mark-complete/inject)
  **embarquent** leur propre ré-ouverture — plus de « resume the run first », plus de course de
  re-fail. Le label terminal précédent reste dans l'event log.
- **Spawn idempotent** — un `SpawnOutcome::Failed`/abort sur le chemin scheduler appende désormais un
  événement `NodeInterrupted` nommant le node + la cause (fin du run figé `running` de #498) ; le
  reap d'un worktree/branche survivant reste porté par `ensure_sub_worktree`.
- Front : statut de node `interrupted` (ambre), raison d'interruption dans la sidebar du run, groupe
  d'actions « run terminé » (Reopen · Retry-all · Open shell) dans la toolbar du canvas (Variante A).

## 1.31.0

Rien de cassant, aucune migration. **Assistant IA d'authoring des templates de bibliothèque**
(#302, ADR-0048). Un glyphe Bot « Pipeline assistant » apparaît dans la barre d'outils du canvas
**uniquement sur un template de bibliothèque** (jamais sur un Run) ; il ouvre un onglet **Assistant**
qui attache une session `claude` amorcée, streamée dans le terminal embarqué.

Notes de version qui ne se déduisent pas du titre : la session s'ouvre avec pour **cwd le dossier
des templates** (`.pdo/library/pipelines`), pas la pipeline courante — c'est délibéré (ADR-0048),
les templates voisins servent d'exemples few-shot et le canvas↔fichier est réconcilié par le store
disk-first à la sauvegarde. Conséquence assumée : ouvert sur une pipeline de travail sans jumelle en
bibliothèque, l'assistant décrit un dossier vide (job d'authoring *from scratch*). Cycle de vie
**create-on-open / reap-on-leave** : la session est créée au premier affichage de l'onglet et
**récoltée dès qu'on le quitte** (changement d'onglet ou fermeture du panneau) ; la ré-ouverture
ré-attache la même session (create-if-absent).

## 1.30.0

**Changement de comportement observable (durcissement sécurité).** Le contrôle d'`Origin`
WebSocket s'étend au second endpoint : `/ws` (le flux d'événements du dashboard) **vérifie
désormais l'`Origin`** comme le fait déjà le terminal PTY. Jusqu'ici `/ws` n'avait aucune garde —
n'importe quelle page ouverte dans le navigateur de l'opérateur pouvait s'y abonner et exfiltrer
passivement tous les événements (repos, prompts, verdicts). Un `Origin` hors allowlist reçoit
maintenant un `403`.

L'allowlist est **configurable** via `PDO_ALLOWED_WS_ORIGINS` (liste d'origines exactes séparées
par des virgules), **additive** aux défauts localhost — nécessaire derrière un reverse-proxy / ALB
sur domaine public (cf. README, *Behind a reverse proxy*). Env-only et jamais réglable depuis l'UI
(le HTTP du daemon n'est pas authentifié).

Échappatoire en développement : le proxy Vite réécrit déjà l'`Origin` (`rewriteWsOrigin` dans
`vite.config.ts`), donc `make dev` fonctionne sans configuration. Pour un front dev servi sur un
port non standard, poser `PDO_ALLOWED_WS_ORIGINS=http://localhost:<port>`. Bonus : ce même correctif
répare le terminal PTY, cassé en dev Vite depuis toujours (même cause d'`Origin`).

## 1.29.0

Rien de cassant, aucune migration. Les **branches remote entrent dans la sélection de branche
source** du formulaire de nouveau Run (#571). `list_branches` liste désormais les réfs de suivi
(`git for-each-ref refs/remotes`) en plus des locales ; l'endpoint `GET /repos/branches` renvoie un
tableau `{name, kind}` (`local` | `remote`), locales d'abord. La note de version qui ne se déduit pas
du titre : la branche source choisie est **stockée verbatim** (une remote reste `origin/xxx`), le
worktree est **coupé directement sur la réf de suivi sans `git fetch`** ni matérialisation d'une
branche locale, la **jumelle locale d'une remote est dédupliquée**, le symref `origin/HEAD` est filtré,
et le défaut proposé n'est **jamais une remote** tant qu'une locale existe. Une réf source inconnue
est rejetée en 400 (nommant branche et dépôt), jamais un Run à moitié né.

## 1.28.0

Rien de cassant, aucune migration (payload d'event schemaless, blob JSON de trigger — le champ
voyage tel quel). Les **dépôts secondaires d'un Run multi-repo sont modifiables par défaut** ;
`read-only` devient une **case à cocher opt-in par dépôt** (#565, ADR-0047 révisant ADR-0042). Un
flag `read_only: bool` (défaut `false`) sur `RepoPin`/`TargetRepoInput` (`#[serde(default)]` +
`skip_serializing_if` ⇒ un pin historique se relit *modifiable*, byte-identique sur le fil). La garde
`secondary_repo_dirtied` (409) ne se déclenche plus que sur un secondaire **coché read-only** ; un
secondaire modifiable voit son `.git` monté **rw** en sandbox (`-v <g>:<g>:rw`, chemin identique) pour
que `git` y fonctionne, et son préambule l'invite à écrire/committer/livrer (env
`PDO_WRITABLE_SECONDARY_REPOS`). **PDO ne livre toujours rien lui-même** : la livraison reste le fait
de l'agent (`Ship It`, `gh pr create` / `git merge`), par dépôt et indépendamment — aucun merge-back
multi-repo n'est réintroduit. Limite assumée : un secondaire rendu modifiable **mid-run** n'a son
`.git` monté qu'après recréation du conteneur (mount figé à la création, cohérent avec la visibilité
au spawn).

## 1.27.0

**Cassant, migration automatique.** Le **harnais agentique** (PRD #549, quatre tranches — #550,
#551, #552, #553) : le programme qui fait tourner l'agent d'un nœud (`claude`, `opencode`) devient
un axe à quatre tiers (`node → Run → Projet → instance → plancher claude`), résolu **une fois au
spawn** et **gelé** dans l'événement de démarrage — la reprise re-pose ce qui a été lancé, jamais ce
que le YAML dit maintenant (ADR-0007). L'axe entier est livré ici : le nœud épingle (#550), le Run
choisit (#551), le Projet porte le tier du milieu (#552), et les capacités sont dispatchées par
harnais (#553).

Le tail n'est plus composé de flags en dur : un **descripteur** (ADR-0045) porte deux templates
d'argv (lancement, reprise) et un bloc d'env, et un module **pur** (`harness_argv`) rend la chaîne
avec une seule règle — *un token dont un trou est vide disparaît en entier*. Le tail `claude` reste
**identique au byte** quand rien de neuf n'est posé (goldens). `claude` et `opencode` sont dans le
**plancher embarqué** ; rien n'est écrit sur disque.

**Migration du schéma de nœud** : les champs plats `model:` / `effort:` deviennent une carte par
harnais `harnesses.<nom>.{model, effort}` (le modèle et l'effort se lisent dans l'entrée du harnais
gagnant, pas d'axe propre — ADR-0046). Le migrateur de pipelines les replie sous `harnesses.claude`
au démarrage ; un `pin_harness:` scalaire épingle le harnais d'un nœud. La carte est **sémantique**
(diff + `content_hash`). Un défaut de harnais **et** un défaut de modèle **par harnais** rejoignent
la Configuration d'instance (`stored → env → default`, ADR-0015 amendée). Un binaire de harnais
introuvable au spawn échoue **fort** (jamais un 2xx, ADR-0037) en nommant le harnais.

**Le Run est un tier** (#551) : le harnais choisi à la création est **gelé dans l'événement de
création** — même posture d'immuabilité que le mode de sandbox — proposé par le dialogue de
lancement, et porté par un Trigger **par construction** (le template d'un Trigger *est* la charge
utile d'une création de Run, donc aucun tier Trigger séparé). Un nœud qui a **épinglé** résiste au
choix du Run. Les **sessions d'infra** (Pipeline Manager, résolveur de merge) suivent le harnais du
Run : « ce Run tourne sur X » devient vrai sans exception — y compris pour l'outil de déblocage, ce
qu'un A/B sur un harnais neuf met donc aussi à l'épreuve (ADR-0046).

**Le Projet est une entité** (#552) : un regroupement **nommé** de dépôts qui se travaillent
ensemble, et le tier du milieu de la précédence. **Rien n'est seedé** — tant que personne n'a nommé
un groupe, il n'existe aucune ligne de Projet et les listes se groupent **par les mêmes chemins
qu'avant**, dérivés côté client ; c'est le crayon sur un en-tête de groupe qui **crée** l'entité,
même posture que la table de prix (ADR-0034). Un chemin appartient à **au plus un** Projet, comparé
**verbatim** (aucune canonicalisation — ADR-0033), et un ajout conflictuel est un **refus nommant**
le Projet propriétaire. Le Projet d'un Run est celui de son **dépôt primaire** : ajouter ou retirer
un secondaire ne change ni le Projet ni le harnais résolu (ADR-0042). Le seuil « ne grouper qu'à
partir de 2 » porte désormais sur les **projets**.

**Changement visible : l'ordre des en-têtes de groupe.** Les groupes se trient désormais sur le
**libellé affiché** (départagé par la clé), là où #258 triait sur le **chemin complet** du dépôt.
C'est ce qui permet à un Projet — dont le nom n'a aucun chemin — de prendre sa place dans le même
ordre que les groupes-chemins. Conséquence à connaître : à contenu identique, une liste peut
apparaître dans un ordre différent de la version précédente, sans qu'aucun Projet ait été créé.

**Les capacités sont du code par harnais, et leur absence est dite** (#553). Les cinq — source de
coût, résolution du transcript, substrat de fin de tour, ancre de menu de limite, plancher de
staging — passent derrière une fabrique (`harness_probes`) dont **toutes les méthodes ont un défaut
« absent »** : un harnais déclaré en donnée n'obtient aucune capacité sans qu'on puisse l'oublier.
`claude` garde les cinq à l'identique. Conséquences visibles : un Run dont le harnais n'a pas de
source de coût affiche **« — » et une raison**, jamais `$0` ni un `partial` muet (même veine que
`unpriced_models`, #425) ; les sondes de fin de tour et de menu de limite **ne tournent pas** sans la
capacité, donc aucun nœud n'est auto-complété sur une heuristique inventée ; un Run sandboxé sur un
harnais sans plancher de staging **le dit une fois** (ADR-0031). Cette tranche livre aussi le **tier
disque** des descripteurs : on déclare un harnais inconnu et on le lance sans recompiler, par fusion
**par nom** avec le plancher embarqué, et un descripteur illisible reste **inerte et diagnostiqué** —
sa clé retombe sur le tier suivant, jamais partiellement appliquée.

**Hors périmètre, assumé** : la sandbox (le plancher de staging reste propre à `claude`), les
capacités d'`opencode` (il se lance, s'attache, se reprend et se complète par `pdo complete` — aucun
coût, aucune fin de tour automatique), l'effort sur `opencode` (aucun axe d'effort au lancement), et
le catalogue de modèles (le modèle reste du texte libre).

## 1.26.0

### Cassant — un spawn tmux échoué fait échouer le nœud et le Run, plus jamais `Spawned` (#508, ADR-0037)

`spawn_node` avalait l'`Err` de `tmux_session_manager::spawn` et rendait `Spawned` alors que
`NodeStarted` était déjà durable : le nœud se projetait `Running` **sans session**, puis la veille de
vivacité le réécrivait `Failed` ~30 s plus tard avec une **cause fausse** (`session_died`), et
`restart_node` répondait un `200 {"spawned":[…]}` menteur. Désormais le bras `Err` appende `NodeFailed`
(légal : l'itération est `Running`) **puis** un reap gaté (le seul sous-worktree que *ce* spawn a créé,
jamais un réutilisé) **puis** `RunFailed`, et rend `SpawnOutcome::Failed`. Effets filaires (véracité,
ADR-0037 §1/§3) : `restart_node` répond **`500 {"error":"spawn_failed","recoverable":false,"run_failed":true}`**
au lieu de `200 {"spawned":[…]}` ; les routes `resume`/`pause` comptent le nœud en **`noop`** (avec sa
raison) au lieu de le déclarer `spawned`. Ferme la « Limite acceptée » homonyme d'ADR-0037.

## 1.25.0

Rien de cassant, aucune migration (`CREATE TABLE IF NOT EXISTS`, aucun backfill). PDO gagne un
**troisième journal** : `audit_log`, le foyer des **mutations de configuration hors-Run** —
create/patch/delete de Trigger et pause globale — jusqu'ici invisibles à l'`event_log` (dont
l'`Event.run_id` est obligatoire). Table dédiée sans `run_id` dans `pdo.db`, écrite au **handler
HTTP** après commit (best-effort : sous-rapport possible, sur-rapport jamais), lue par `GET /audit`
(feed global décroissant, filtrable par cible et fenêtre `[from, to)`). L'origine est un indice
**déclaratif et falsifiable** (`actor_hint` via l'en-tête `X-PDO-Actor`, jamais un gate — bind
0.0.0.0 sans auth). Referme la cause de la fausse #505 : un Trigger coupé à la main laisse désormais
une trace. AC5 (signal *vivant* `overdue`) différé hors v1 (#507, ADR-0044).

## 1.24.0

Rien de cassant, aucune migration. La **complétion sur fin de tour gagne un substrat primaire,
event-driven, côté agent** : un hook `Stop` de Claude Code (`pdo complete --auto; exit 0`), injecté
au spawn/resume via `claude --settings`, complète le nœud dès que l'agent finit son tour ; le
balayage daemon devient le repli (#433, ADR-0043 amendant ADR-0032 §2). Opt-in, **décoché par
défaut** (`autocomplete_turn_end`), réutilise l'event `NodeAutoCompleted` avec un libellé distinct,
sûr par construction (`; exit 0` avale le refus 3 des outputs manquants — jamais de boucle), immunisé
côté `script` (bash, jamais `claude`). Bump **re-posé au next-free** (1.24.0 contre `origin/main`
1.23.0) après collision avec #473.

## 1.23.0

Rien de cassant, aucune migration. Le **transcript d'un nœud est résolu par identité de session
épinglée** (`<uuid>.jsonl`) au lieu du plus récent fichier d'un dossier projet partagé : un nœud
non code-mutating ne collisionne plus avec le transcript de la session `__manager__` (#473). Le
spawn d'un nœud agent épingle un `--session-id` UUID v4 (jamais pour un `script`), la veille et le
resume résolvent par ce nom exact, les sessions infra restent sans id (donc jamais sondées ni
reprises). ADR-0032 amendé (invariant byte-identité du tail levé pour un nœud agent, conservé pour
`None`). Bump **re-posé au next-free** (1.23.0 contre `origin/main` 1.22.1) après collision avec
#437/#465.


Rien de cassant. La **liste de dépôts d'un Run devient éditable en cours de Run** — ajout/retrait de
dépôts secondaires read-only, primaire verrouillé (#465 slice 2, ADR-0042). Bump **re-posé au
next-free** (1.22.0 contre `origin/main` 1.21.0) après collision avec #528, comme prévu à la pose
initiale.

### Édition mid-run de la liste de dépôts (#465, ADR-0042)

La slice 1 gelait la liste de secondaires à la création du Run. Cette slice livre le dernier besoin
explicite de #465 : « la liste de repos doit rester modifiable à tout moment ». Purement additive —
on reste dans le modèle snapshot read-only (aucun merge-back, aucun `base_sha`, aucun `commit-tree`).

- `PATCH /runs/{run_id}/repos` (`{ add, remove }`) ajoute/retire des secondaires sur un Run **vivant**,
  depuis le panneau de détail du Run (primaire verrouillé). Refus typés (`RepoEditRefusal`) :
  `run_not_editable`, `secondary_is_primary`, `secondary_already_pinned`, `bad_secondary_repo`, …
- Nouvel event `run_repos_edited` portant la **liste active complète re-gelée** ;
  `RunState.target_repos` n'est plus figé à `RunStarted`. Le réducteur écrase la liste et **no-op sur
  un Run terminal** (garde #221 en double, avec le refus 409 du handler).
- **Visibilité au spawn** : une édition touche les nœuds lancés **après** elle ; les nœuds déjà
  vivants gardent leur contexte figé. Un ajout matérialise le snapshot (0 mount neuf) ; un retrait le
  laisse sur disque jusqu'au cleanup, désormais **piloté par le disque** (balaie `repos/*`, couvre
  les snapshots retirés-mais-persistants et orphelins).
- Le panneau « Repositories » reste atteignable **pendant qu'un nœud tourne** : un bouton dédié de la
  barre d'outils du canvas ouvre le détail du Run sans que l'auto-sélection du nœud vivant ne le
  reprenne aussitôt (sinon l'édition « à tout moment » était infaisable au navigateur).
- Restent différés : le sélecteur `repo:` par nœud, le `git` in-sandbox sur un secondaire, et
  l'écriture / MR dans un secondaire.

## 1.21.0

Rien de cassant. Un champ **purement additif, lecture seule** : `GET /stats/cost` porte désormais un
tableau **`resolved`** — une entrée par clé de famille, avec le **tier gagnant** (`manual` / `fetched`
/ `embedded`) et le **`$/MTok`** effectivement appliqué (#528). Rendu dans l'onglet **Stats → Cost**,
à côté du bouton **« Sync costs »** : on synchronise les prix et on lit ce que PDO sait tarifer au
même endroit, et le refetch déclenché par la synchro rafraîchit la table. C'est la même `PriceTable`
que celle avec laquelle le fold de coût chiffre, donc la vue ne peut jamais diverger de ce qui tarifie
réellement (#373). **Dans le cadre d'ADR-0034** : cette slice **amende sa hors-scope** (qui déclinait
un `GET /prices` en le jugeant redondant avec `manual_keys` + `fetched_rows`) — **pas de nouvel ADR**,
**pas de route dédiée** (champ additif sur un endpoint déjà consommé par l'onglet Cost, rétro-compatible,
zéro taxe proxy vite dev, cf. *Versioning*). Bump re-posé contre `origin/main` (1.20.0, #527) après
collision de bump : la vue `resolved` reflète désormais aussi le plancher gen-5 introduit par #527
(quatorze familles embarquées au lieu de onze).

## 1.20.0

Rien de cassant. Le **plancher de prix embarqué** amorce désormais la génération courante
(`claude-opus-5` / `claude-sonnet-5` / `claude-fable-5`), si bien qu'une instance **jamais
synchronisée, hors ligne** chiffre son modèle par défaut au lieu d'afficher `~$0.00 †` (#527,
amende ADR-0034). Le plancher reste un **plancher, pas un miroir** : un sync surcharge encore
chaque clé (p. ex. `sonnet-5` à son intro live). Bump posé contre `origin/main` (1.19.0) ; à
re-poser au next-free si un autre Run livre entre-temps.

## 1.18.0

Rien de cassant. Une capacité **purement additive** : un Run peut désormais lire plusieurs dépôts
(#465, ADR-0042). Bump posé contre `origin/main` (1.17.0) ; à re-poser au next-free si un autre Run
livre entre-temps.

### Multi-repo par Run — dépôts secondaires en lecture seule (#465, ADR-0042)

Un Run pouvait ne cibler qu'**un** dépôt (`target_repo`, ADR-0033). Cette slice ouvre le
**multi-repo par Run** dans son incrément le plus étroit : **lecture multi-repo, écriture
mono-repo**.

- On sélectionne, à la création du Run, N **dépôts secondaires** en plus du **primaire**
  (`target_repos[0]` = le primaire, sémantique de `target_repo` inchangée). Chaque secondaire porte
  une *target branch* (défaut : la ref **locale**, pas `origin/main` — il n'y a aucun `git fetch`).
- Chaque secondaire est figé à un **SHA au démarrage** (`git worktree add --detach`), sous
  `<primaire>/.pdo/runs/<id>/repos/<alias>/`. Reproductible : muter le checkout local du secondaire
  ne bouge plus le Run.
- Les nœuds **lisent** les secondaires par **chemin absolu** (injecté au préambule + env
  `PDO_SECONDARY_REPOS`) — les sous-worktrees n'héritent pas des fichiers du snapshot.
- Garde **409 `secondary_repo_dirtied`** si un nœud salit un fichier **suivi** d'un secondaire (les
  *untracked* sont tolérés) ; non terminale (revert + re-complétion passent).
- Nettoyage : `git worktree remove --force` **+ `prune`** dans **chaque** secondaire au teardown
  (sans le prune, registration `--detach` fantôme — classe #498).
- Les Triggers portent la liste (colonne `target_repos`, forwardée au fire).

Différé (slices ultérieures) : écriture/MR dans un secondaire, édition mid-run de la liste,
`git` in-sandbox sur un secondaire, merge-back multi-repo (rejeté définitivement), sélecteur
`repo:` par nœud, grouping list/cost multi-bucket.

## 1.14.0

Rien de cassant. Un champ **purement additif** sur l'estimation de coût (périmètre AC#4 de #425,
dans le cadre d'ADR-0034 — pas de nouvel ADR).

### L'estimation de coût nomme désormais le modèle non tarifé (#425)

Jusqu'ici, un Run (ou un bucket de la modale Stats) dont une session avait tourné sur un modèle
qu'aucun tier ne tarife affichait `~$0 †` — ou, pire, un nombre plausible mais faux-bas — avec un
tooltip générique « an unpriced model was excluded ». Le `†` seul ne disait **jamais quel** modèle,
si bien que `claude-fable-5` (le plus cher) a pu rester invisible des semaines dans `/stats/cost`.

Désormais `CostStat` porte `unpriced_models: Vec<String>` (clés de famille **dé-datées**, triées,
uniques) ; `partial` en est **dérivé** (`partial ⟺ !unpriced_models.is_empty()`). Le champ voyage
sur `GET /runs/:id` et, unioné par bucket, sur `GET /stats/cost` ; l'UI nomme le(s) modèle(s) dans le
tooltip du `†`. La clé du memo de coût est **inchangée** (l'information voyage dans la valeur), donc
aucune régression du chemin `/stats/cost`.

Ce qui **n'est pas** dans cette version : amorcer le plancher embarqué avec la gen-5 (opus-5,
sonnet-5, fable-5) — cela réviserait le principe de membership d'ADR-0034 et relève d'une décision du
propriétaire (#527). Sur une instance non-syncée, le modèle par défaut du compte continue donc de
contribuer $0 ; la différence est qu'il est maintenant **nommé**, donc actionnable (un clic « Sync
coûts » ou une ligne dans `~/.pdo/prices/models.yaml`).

## 1.13.0

**Cassant** (`feat(#516)!`) : le champ de réponse `stale_git_lock` disparaît, remplacé par
`interrupted_git_ops`. Amende ADR-0037, pas de nouvel ADR.

### `stale_git_lock` (`string|null`) → `interrupted_git_ops` (`array`, `[]` si rien) (#516)

Quand `restart_node` réutilise le sous-worktree d'un nœud `code-mutating`/`merge`, une session tuée
**au milieu d'une opération git** y laisse des marqueurs dans le gitdir privé (`index.lock`,
`MERGE_HEAD`, `rebase-merge/`, `rebase-apply/`). Le daemon les **détectait** — mais n'en remontait
qu'**un seul**, le premier du scan. Chaîne mesurée sur le code 1.12.0 : un `index.lock` **masquait** un
`MERGE_HEAD` coexistant, l'agent retirait le verrou dont on l'avait averti, lançait `pdo complete`, et
le merge-back faisait un `git commit` avec le `MERGE_HEAD` resté en place → **un commit de merge à deux
parents que personne n'a voulu, en silence** (`MergeResult::Success`, zéro événement). Le filet d'ADR-0037
§7 est en aval du commit ; il ne l'attrape pas.

Trois changements :

- **Inventaire complet.** Le scanner remonte désormais **tous** les marqueurs présents, dans l'ordre du
  scan (`index.lock` en tête), au lieu du premier seul.
- **Migration filaire cassante.** Le corps de succès de `restart_node` remplace
  `"stale_git_lock":"index.lock"|null` par `"interrupted_git_ops":["index.lock",…]|[]` — **toujours** un
  tableau, jamais `null` ni absent (un client lit `body.interrupted_git_ops.length` sans garde).
  « Stale git lock » était faux pour trois marqueurs sur quatre : un `MERGE_HEAD` ou un rebase interrompu
  n'est pas un verrou, et le nommer ainsi a masqué le second marqueur derrière le premier. Le champ,
  interne comme filaire, se propage du scanner (`SubWorktreeState::Reusable`) jusqu'à la réponse HTTP.
- **Routage vers le préambule.** La consigne différenciée (retirer `index.lock` d'abord ; inspecter puis
  finir **ou** avorter un `MERGE_HEAD`/rebase, au jugement de l'agent) arrive maintenant **dans le
  préambule du nœud re-spawné lui-même**, plus seulement dans le corps que voit le manager. L'agent frais
  n'attend plus qu'on la lui relaie, et le daemon ne supprime **jamais** un marqueur (il ne peut pas
  prouver que l'écrivain est mort — #485 est le précédent qui coûte cher).

Aucun travail frontend : l'UI jette déjà le corps de la réponse (`responseMode:"void"`), et le trou
d'observabilité reste la propriété de #492.

## 1.12.0

Rien de cassant. Un nouveau réglage d'instance **purement additif** (application directe d'ADR-0015,
pas de nouvel ADR).

### Le nommage automatique des Runs par le manager est désormais configurable (#338)

Jusqu'ici, un Run créé sans nom était **toujours** nommé par le Pipeline Manager (depuis son input,
ou via un placeholder renommé best-effort), et les Triggers l'étaient sans réglage possible. #338
livre trois choses, sans rien casser :

- un défaut d'instance `default_auto_name` (booléen, résolu `stored → env PDO_DEFAULT_AUTO_NAME →
  défaut **true**`), exposé dans `SettingsModal` avec la divulgation de source habituelle. Colonne
  `instance_config.default_auto_name` NULLABLE, migration `ALTER … ADD COLUMN` PRAGMA-guardée ;
- un override par-Run — champ optionnel `auto_name` sur `POST /runs` (JSON **et** multipart). **La
  compat est préservée** : un appelant qui passe un `name` sans le flag garde son nom exactement
  comme avant (le flag ne se résout sur le défaut d'instance que lorsqu'il est absent ET qu'aucun
  nom n'est fourni) ;
- un override par-Trigger — colonne `triggers.auto_name` (`NOT NULL DEFAULT 1`, donc les Triggers
  existants continuent d'auto-nommer), figée à la création depuis le défaut d'instance et lue au
  fire. Désactivée, chaque Run né du Trigger porte un *nom placeholder* stable (`Untitled run <id>`)
  et le manager n'est pas instruit de renommer.

## 1.9.0

Rien de cassant. Une note, parce qu'elle change **rétroactivement** ce qu'un opérateur peut croire de
ses incidents passés.

### Le balayage d'orphelins tuait les sessions qu'il venait de spawner (#485, ADR-0038)

Le reaper prenait ses deux observations dans le mauvais ordre : instantané de **tous** les Runs
(N+1, 21 s mesurées sur 437 Runs en production) **puis** énumération des sessions tmux. Une session née
dans cet intervalle était vivante dans tmux et absente de l'instantané — donc classée orpheline, et
tuée. Dans une occurrence, **150 ms après son propre spawn**. Neuf occurrences en huit jours, deux Runs
perdus dans la nuit du 2026-07-30 ; la probabilité croissait avec le nombre de Runs conservés, ce qui
en faisait un défaut qui empire tout seul.

**Ce qu'il faut relire dans vos incidents passés** : la veille de vivacité imputait ces morts à tmux
sous un `session_died: tmux session pdo-… no longer exists` parfaitement crédible, suivi d'un
`run_stalled` et d'un Run `Failed`. Tout verdict `session_died` antérieur à cette version sur un nœud
qui venait de démarrer est donc suspect — la cause nommée accusait tmux, la RAM ou l'API pour un bug
d'ordonnancement d'observations. Le seul témoin du kill était `journalctl`.

L'ordre est désormais porté par les types plutôt que par la discipline des appelants :
`decide_sweep` est pure et reçoit l'inventaire tmux comme **donnée d'entrée** clé par session, si bien
que l'ordre inverse n'est plus exprimable ; la lecture du log vient après, et la preuve est par
contraposée (le log ne fait que croître, donc une absence constatée *après* l'inventaire garantit
l'absence *au moment* de l'inventaire). Le N+1 disparaît par construction : seuls les Runs qui tiennent
une session vivante sont projetés. Côté spawn, `start_node` ne spawne plus — elle rend une intention
que l'appelant exécute **après** l'append de `NodeStarted`, de sorte qu'aucune session n'existe avant
sa réservation.

Aucun changement de politique de reap : les trois motifs d'orphelinage, le shell sans TTL (#316), le
Manager sans TTL (#458) et l'aveuglement à `iter` sont inchangés, et les huit messages de kill sont
byte-identiques. Deux ajouts d'observabilité : les kills pour **absence** et pour **nom non reconnu**
passent en `warn!` (le ménage nominal — archivé, TTL — reste en `info!`), donc `journalctl -p warning`
les trouve sans grep intégral ; et `GET /sessions` porte `reaper: { last_sweep_at, killed,
killed_for_absent_run }`. Les deux compteurs sont **cumulés depuis le
démarrage du daemon** (non persistés : un redémarrage les remet à zéro), parce qu'un kill est un
*événement* — une jauge par passe répondait « le *dernier* balayage a-t-il tué ? », donc `0` quelle que
soit la vitesse à laquelle on regarde, la passe qui tue étant suivie en quelques secondes d'une passe à
vide. `killed_for_absent_run` doit rester **plat** : après ce correctif, une absence constatée sur une
session vivante est un « ne peut plus arriver », et le cumul est ce qui rend cette affirmation
vérifiable. Le détail par session reste dans `journalctl`.

Ne ferme pas #498 : le sous-worktree et la branche d'une session tuée survivent et condamnent le nœud
au respawn ; ce correctif n'en supprime que le producteur principal.

> **Note de numérotation** : cette ADR est **0038**. Elle a été écrite et revue sous le numéro 0037,
> que #489 (1.8.0) a pris entre-temps ; le renommage est purement éditorial.

## 1.8.0

Un changement cassant livré sous un bump **mineur**, dans la ligne des précédents posés en 1.2.0, 1.3.0
et 1.6.0 : la surface quotidienne est identique (`restart_node` s'appelle au même endroit avec les mêmes
arguments), et le comportement retiré était un **mensonge** — la commande répondait `200 {"ok":true}`
sur les cinq issues possibles de son spawn, dont l'échec, et sur les nœuds `code-mutating`/`merge` elle
échouait à **100 %** en affirmant avoir réussi. Aucune configuration vivante ne peut en dépendre sans
être déjà cassée. **Si le mainteneur préfère la lettre du préambule ci-dessus (« la casse se signale ici
et par un bump majeur »), c'est `2.0.0` : un mot à changer dans `Cargo.toml`.**

### Cassant — un `restart_node` qui n'a pas spawné n'est plus jamais un `200` (#489, ADR-0037)

Le bras tuait la session tmux, appendait son `CommandIssued`, puis découvrait le Run, le pipeline et le
nœud, et **jetait** le `SpawnOutcome` de `spawn_node` — pas même un `let _ =`. Voir **ADR-0037**.

| Cause | Statut avant | Statut après | Slug (`error`) | `recoverable` |
|---|---|---|---|---|
| Garde de transition rejette (3 raisons atteignables) | `409`, prose dans `error`, pas de `recoverable` | `409` | `restart_refused` | `true` |
| `node_id` absent du pipeline **du Run** | **`200 {"ok":true}`** | `400` | `node_not_found` | `true` |
| Conteneur sandbox pas encore prêt (#445) | **`200 {"ok":true}`** | `409` | `sandbox_prep_not_ready` | `true` |
| Sous-worktree tenu par un autre worktree vivant | **`200 {"ok":true}`** | `409` | `sub_worktree_occupied` | `true` |
| Le spawn a échoué (`SpawnOutcome::Failed`) | **`200 {"ok":true}`** | `500` | `spawn_failed` | `!run_failed` |
| Throttle du cap de sessions | `200 {"ok":true}` | `200 {"ok":true,"waiting":true,"reason":…}` | — | — |
| Spawn réussi | `200 {"ok":true}` | `200 {"ok":true,"spawned":[…],"reused_sub_worktree":…,"base_sha":…,"stale_git_lock":…}` | — | — |

- **Les clients discriminent sur `error`, jamais sur le statut** — même règle qu'en 1.6.0, même patron de
  type (`RestartVerdict` ne porte aucun statut ; sa projection en est la seule propriétaire, avec un
  `match` exhaustif sans joker).
- **Tout refus connaissable est rendu AVANT le kill de la session.** ADR-0025 §2 disait « valider avant
  d'écrire » ; cette version l'étend au kill. Un `4xx` rendu après la destruction d'une session n'est pas
  une validation, c'est un constat.
- **`session_killed` est le champ neuf, et le seul qui porte un bit utile sur un refus.** `false` = rien
  n'a été touché, corrigez la cause et rappelez. `true` = la session est morte et **rien ne l'a
  remplacée** (deux courses post-kill seulement) : ce nœud demande un autre levier, pas un retry de
  celui-ci. `recoverable` est uniformément `true` sur les refus — sa définition (ADR-0035) est « le
  daemon a-t-il déjà enregistré l'issue terminale ? », et aucun refus de restart n'enregistre rien.
- **`Throttled` reste un `2xx`, et n'est pas un `noop`.** Un `NodeWaiting` **a** été appendé, il a flippé
  le nœud en `waiting`, et le balayage d'admission le reprend réellement. Appeler ça « no-op » serait le
  petit mensonge symétrique de celui que ce lot corrige — d'où le vocabulaire `waiting`, qui amende
  ADR-0025 §3 pour cette classe de commandes.
- **`spawn_failed` est un `500`, pas un `409`** : ce n'est pas un verdict, c'est une panne. `run_failed`
  est **re-projeté** après le spawn plutôt que deviné, parce que les quatre producteurs de `Failed`
  divergent — et un `500` route la CLI vers `pdo fail`, conseil catastrophique si `RunFailed` est déjà
  au log.

### Le travail non commité d'un sous-worktree survit désormais à un `restart_node`

C'est une **garantie neuve**, et elle ne se déduit pas du titre. Le sous-worktree d'un nœud est nommé
purement à partir de `(run, node, iter)` et `restart_node` re-spawne sur le même `iter` : le re-spawn
rejouait donc `git worktree add -b <branche déjà existante>`, que git refuse (exit 255). Sur les nœuds
`code-mutating` et `merge` le levier échouait à 100 %, en silence, et la veille de vivacité inventait
30 s plus tard un `session_died` — une cause fausse qui envoyait l'opérateur sur la piste tmux pour un
bug git.

Le sous-worktree est maintenant **réutilisé en place** quand il est là et sur la bonne branche : aucun
appel git mutant, le travail en vol de la session morte reste intact, et c'est ce qui distingue
`restart_node` de `node_retry` côté disque (`node_retry` reste l'outil table-rase, et le seul qui donne
une base fraîche). Un résidu sans valeur — enregistrement prunable, branche orpheline — est recyclé ;
un sous-worktree **tenu ailleurs** est refusé en nommant ce qui le tient, jamais reapé. Le corps de
succès porte `reused_sub_worktree`, et `stale_git_lock` quand un verrou git traîne dans le worktree
(signalé, jamais supprimé : PDO ne peut pas prouver que l'écrivain est mort).

Deux corrections que cette réutilisation rend obligatoires, parce qu'elle promeut deux bugs latents en
bugs systématiques : `reap_orphan_sub_worktree` prune désormais avant de supprimer la branche (sans
quoi il laissait les deux verrous en place quand le répertoire avait déjà disparu — c'est aussi le
levier 2 de #498) ; et `commit_and_merge_sub_worktree_inner` vérifie enfin le **statut** de son
`git add -A`. Ce dernier valait, avec un `index.lock` résiduel : `add` échoue (128) →
`diff --cached --quiet` répond 0 → aucun commit → `git merge` dit « Already up to date » →
`MergeResult::Success` sur **100 % du travail perdu**, sans conflit, sans événement, sans trace. Il
`bail!`e maintenant, et ressort en `merge_failed`.

### Fin de l'auto-throttle d'un restart (#489-C)

À `live == cap`, un `restart_node` sur une itération vivante se throttlait **contre lui-même** : le bras
tue la session mais n'appende aucun événement de cycle de vie, donc le nœud projetait encore `Running`
quand le comptage passait. Le gel était **définitif** — `retry_waiting_nodes` n'a aucun timer,
`resume_run` est un no-op sur un throttlé, le boot recovery ne regarde que `Running`/`AwaitingUser`, et
le bouton Stop répond `409`. Le comptage d'admission exclut désormais le slot que le spawn est en train
de reprendre, sur la clé `(run_id, node_id, iter)` — jamais `(node_id, iter)`, qui écarterait la session
vivante d'un **autre** Run du même pipeline et **dépasserait** le cap. Et `kill_node` réveille les nœuds
`waiting`, ce que rien ne faisait depuis la surface commandes.

### Ce qui ne change pas

- **`GET /sessions` et la gauge de la barre de statut** rapportent toujours le compte **vrai** : seule la
  porte d'admission applique l'exclusion.
- **ADR-0032 / la veille de vivacité** : indemne par construction — elle lit des événements, jamais un
  statut HTTP. Elle cesse simplement d'inventer un `session_died` là où un `worktree add` avait échoué.
- **ADR-0036 / la règle d'adoption du merge-back** : inchangée. Seule la façon dont un re-spawn obtient
  son `base_sha` change — une réutilisation **reporte** la base d'origine au lieu d'en dériver une, sans
  quoi l'échappatoire serait silencieusement morte (ou pire, armée à faux) pour tout nœud redémarré.
- **Le contrat de `node_retry`** et celui des quatre commandes de boucle (ADR-0025).
- **Les corps `text/plain`** de « run not found » et « cannot read/parse pipeline » : mêmes corps, même
  content-type, seule leur **position** change (ils passent avant le kill). Les normaliser est #491.
- **Le kill reste nu**, et non `reap_node_session` : le snapshot de pane ne serait jamais servi sur une
  itération non terminale (famille #492).

### Migration

- **Appelants directs de `restart_node`** (`curl`, Pipeline Manager, scripts) : remplacer tout test de la
  forme `if resp.ok` par une lecture de `body.error` puis `body.recoverable` et `body.session_killed`. Un
  `409`/`400`/`500` n'est plus une anomalie de transport, c'est un **verdict**.
- **Traiter `waiting:true` comme un succès différé**, pas comme un échec : le nœud est réservé, il
  spawnera quand un slot se libère — ne pas ré-émettre la commande.
- **Lire `reused_sub_worktree`** avant de dire à l'agent frais de repartir de zéro : quand il vaut `true`,
  le répertoire contient déjà le travail non commité de la session précédente.
- **Rien à faire côté données** : aucun payload d'événement ne change de forme. Le `base_sha` d'un
  `node_started` de re-spawn vaut désormais celui de la coupe d'origine au lieu d'être absent.

### Trou connu, énoncé pour qu'il ne soit pas re-fiché en régression

Cette vérité filaire n'est **observable par aucun humain via l'UI**. `api.ts` appelle `restartNode` en
`responseMode:"void"` avec un `catch {}` — le corps *et* l'erreur sont jetés — et le seul bouton câblé
vit dans une bannière gatée `node.status === "stale"`, que plus rien ne produit depuis #469. Le
consommateur vivant de cette route est le Pipeline Manager en `curl`, dont le préambule est mis à jour
ici. Le volet UI appartient à **#492**. Zéro travail frontend dans ce lot.

## 1.7.0

Rien de cassant. Une note, parce qu'elle ne se déduit pas du titre du commit et qu'elle change ce
qu'un opérateur peut supposer de ses branches.

### Le runtime peut désormais déplacer la ref d'une branche pipeline (#503, ADR-0036)

Jusqu'ici le runtime ne touchait jamais une ref existante : il créait des branches et des worktrees,
n'en supprimait aucun, n'en réécrivait aucun (ADR-0012(a)). Le merge-back d'un sous-worktree peut
maintenant **déplacer** le tip de la branche pipeline pour adopter l'arbre du nœud, lorsque la
divergence est l'histoire du Run que ce nœud a lui-même réécrite en se rebasant. Le déplacement passe
par un `commit-tree` à deux parents qui garde **l'ancien tip en premier parent** : aucun commit ne
devient inatteignable, `git log` de l'ancien tip reste intact, et la règle d'ADR-0012(a) sur la
non-suppression est inchangée. Le garde est structurel — le tip pipeline doit encore être exactement
le `base_sha` depuis lequel ce sous-worktree a été coupé — donc il ne peut pas adopter par-dessus le
travail d'un autre nœud.

Deux effets de bord du même défaut sont corrigés au passage : `merge_conflict_detected.payload.detail`
est enfin rempli (il lisait `stderr`, que `git merge` laisse vide en cas de conflit — le rapport est
sur stdout), et un merge-back qui échoue appende `NodeFailed` puis reape la session du nœud au lieu de
la laisser vivante sous une projection `running`.

## 1.6.0

Un changement cassant livré sous un bump **mineur**, dans la ligne des précédents posés en 1.2.0 et
1.3.0 : la surface quotidienne est identique (le bouton *Mark complete* et `pdo complete` sont au même
endroit et prennent les mêmes arguments), et le comportement retiré était un **mensonge** — huit sorties
qui décrivaient un refus répondaient `200`, dont quatre après avoir déjà tué le Run. Aucune
configuration vivante ne peut en dépendre sans être déjà cassée : un client qui lisait « succès » sur
ces réponses se trompait par construction. **Si le mainteneur préfère la lettre du préambule ci-dessus
(« la casse se signale ici et par un bump majeur »), c'est `2.0.0` : un mot à changer dans
`Cargo.toml`.**

### Cassant — un refus de complétion n'est plus jamais un `200` (#490)

Le chemin par lequel **tout** node se termine — `pdo complete` (`POST …/nodes/<id>/done`) et *Mark
complete* (`POST …/commands` `kind=mark_node_done`) — répondait `200` sur huit corps qui décrivaient un
refus. Un agent lisait `Node <id> marked complete.`, sortait en `0`, et croyait avoir livré un Run que
le daemon venait de tuer. Voir **ADR-0035**.

Les sorties qui passent de `200` à **`409`**, avec leur nouveau slug :

| Slug (`error`) | `recoverable` | Événements déjà appendés |
|---|---|---|
| `frontmatter_retry_pending` | `true` | `FrontmatterRetryPending` |
| `frontmatter_retry_exhausted` | `false` | `NodeFailed` + `RunFailed` |
| `script_validation_failed` | `false` | `NodeFailed` + `RunFailed` |
| `doc_violated_code_immutability` | `false` | `NodeFailed` + `RunFailed` |
| `merge_conflict` | `false` | `MergeConflictDetected` + `RunFailed` |
| `merge_resolution_failed` | `false` | `MergeResolverFailed` + `RunFailed` |
| `merge_resolver_spawned` / `merge_resolver_failed` | `false` | branches mortes depuis ADR-0006 |

Deux refus **changent de corps sans changer de statut** : `completion_rejected` (le garde de transition,
déjà en `409`) voit sa prose passer de `error` à **`message`**, `error` portant désormais le slug ; et
`missing_outputs` gagne simplement `recoverable: true`.

- **Les clients discriminent sur `error`, jamais sur le statut.** Un statut n'a pas assez de bits pour
  neuf causes, et le client de l'UI en était la démonstration : il relisait *tout* `409` de ce chemin
  comme `missing_outputs` avec une liste vide, gatée sur `length > 0`, donc le refus le plus fréquent de
  tous — tout clic sur un node d'un Run déjà en échec — n'affichait **rien**. Un slug inconnu se rend
  tel quel (ADR-0001).
- **`recoverable` répond à une seule question** : *est-ce encore ton tour ?* `true` ⇒ le node est
  toujours `running`, rien de terminal n'est enregistré. `false` ⇒ le daemon a **déjà** enregistré
  l'issue terminale ; ne jamais enchaîner sur `pdo fail`.
- **Le détail est verbatim celui d'avant** (`missing`, `violations`, `detail`, `reason`) : ce lot déplace
  un statut et ajoute deux clés, il ne renomme aucun champ de détail. Le `detail` du fail-fast d'un node
  `script` reste **imbriqué** — l'aplatir rendrait sa trace d'audit indistinguable d'un échec après
  retry, et c'est le **projecteur** qui apprend à lire les deux formes.

### Cassant — `pdo complete` a un contrat de codes de sortie

| Code | Sens | Geste attendu |
|---|---|---|
| `0` | accordée, ou **doublon légal** (`noop`) | rien |
| `3` | refusée, **encore ton tour** | corriger, rappeler `pdo complete`. **Pas** `pdo fail` |
| `4` | refusée, **le runtime a déjà tranché** | s'arrêter et rapporter. **Pas** `pdo fail` |
| `1` | panne, transport, corps illisible, `5xx` | ici **seulement**, `pdo fail` est le bon conseil |

**Pourquoi un `4` distinct du `1`.** Le tail bash d'un node `script` fait `pdo complete || pdo fail`.
Ce `||` était du **code mort** tant que son déclencheur répondait `200` ; le passage au `409` le
réveille. Sans discrimination du `4`, chaque refus terminal d'un node `script` produirait **deux**
`NodeFailed` et **deux** `RunFailed`, le second avec une raison fausse (`NodeFailed` est absorbé par le
garde, `RunFailed` ne l'est pas). Le tail teste donc le `4`, et un test de couche 3 en vrai tmux + vrai
bash compte exactement un `run_failed`.

**Le `0` sur un doublon légal est non négociable** : un agent perplexe qui rappelle `pdo complete` ne
doit pas lire « refusé » puis enchaîner `pdo fail` — il tuerait un Run qui vient de réussir, et sur un
node `script` le tail le ferait sans demander.

### Ce qui ne change pas

- **La sémantique du `2xx`** (ADR-0023) : « ton événement terminal est durablement enregistré et
  l'avance est planifiée ». Elle gagne seulement un corollaire — « et ta complétion n'a pas été
  refusée ».
- **La queue d'avance détachée** (#304) et le **fail-fast** d'un node `script` (ADR-0017) : intacts.
- **`410`, `404`, `500`** : « jamais `2xx` » n'est **pas** « toujours `409` ». Le tombstone d'un Run
  oublié garde son `410` (ADR-0024), une cible inconnue son `404`, une panne son `500`. Sur la route
  `POST …/nodes/<id>/done`, ces trois-là portent désormais le corps JSON du contrat
  (`{error, recoverable, message}`) au lieu d'un texte brut ; leur statut est inchangé.
- **Le corps de succès de `POST …/done` reste le texte brut `ok`**, asymétrique avec le
  `{"ok":true}` de `POST …/commands`. Symétriser appartient à **#491**.
- **La veille de vivacité** (#469 / ADR-0032) : indemne par construction — elle lit la variante du
  résultat, jamais le statut HTTP.

### Migration

- **Appelants directs** (`curl`, scripts, harnais de test) : remplacer tout test de la forme
  `if resp.ok` ou `if body.status == "..."` sur ce chemin par une lecture de `body.error` +
  `body.recoverable`. Un `409` n'est plus une anomalie de transport, c'est un **verdict**.
- **Bash d'un node `script` écrit à la main** : si le corps enchaîne `pdo complete || pdo fail`, ajouter
  la garde sur le code `4` — sinon chaque refus terminal produit un second `RunFailed` à raison fausse.
  Le tail généré par PDO le fait déjà.
- **Rien à faire côté données** : aucun payload d'événement ne change, `NodeState` gagne un
  `missing_outputs` omis quand vide, et les Runs archivés se projettent à l'identique à l'octet — à une
  correction près, un bug préexistant : un node redevenu vert ne traîne plus les violations de la
  tentative dont il s'est remis.

## 1.3.0

Un changement cassant livré sous un bump **mineur**, dans la ligne du précédent posé en 1.2.0 : la
surface UI est identique (le champ « Target repository » de New Run était déjà requis côté client, et
son gate `repoValid` est renforcé ici), et aucune configuration vivante ne dépend du comportement
retiré — les 9 Triggers de production nomment tous leur dépôt. **Si le mainteneur préfère la lettre du
préambule ci-dessus (« la casse se signale ici et par un bump majeur »), c'est `2.0.0` : un mot à
changer dans `Cargo.toml`.**

### Cassant — le cwd du daemon n'est plus une cible de Run implicite (#470)

`target_repo` devient un **champ requis** aux quatre frontières d'écriture. Un appel sans lui, avec une
chaîne vide ou avec des blancs répond **400 en nommant le champ**, au lieu de créer silencieusement son
worktree dans le dépôt d'où le daemon a été lancé (`~/.pdo/app` en production). Le 2026-07-29, deux Runs
y avaient écrit du code, récupérable seulement par un `git fetch ~/.pdo/app`. Voir ADR-0033.

- `POST /runs` (JSON **et** multipart) : 400, avant tout effet — aucun `run_id`, aucun `run_started`,
  aucun worktree, aucune session ;
- `POST /triggers` : 400, aucune ligne persistée ;
- `PATCH /triggers/:id` : reste un **merge partiel**. Un `target_repo` **absent** laisse la valeur
  stockée intacte (c'est ce qui garde `{"enabled": true}` du toggle de liste légal) ; un `null`
  explicite ou une chaîne blanche répond 400. Effet de bord voulu : avant, un `null` explicite était un
  **no-op silencieux** côté serde, donc vider le champ dépôt dans l'UI paraissait marcher et ne faisait
  rien ;
- `POST /triggers/guard/test` : 400. Le dry-run exécute un `sh -c` arbitraire ; son invariant « zéro
  effet de bord » portait sur le fait de ne pas créer de Run, pas sur ce que la commande fait ;
- un **Trigger dont le dépôt cible est nul devient dormant** au lieu de tirer : le refus remonte avant
  le guard, donc le guard n'est **jamais** lancé (5 des 9 Triggers vivants font `git pull` /
  `gh issue list`), `next_fire_at` passe à `NULL`, et « Run now » répond **409** avec la raison au lieu
  d'un `200 {fired:false}` ;
- `retry_all` recopie désormais le dépôt **résolu** et non le champ brut. Sans ça, retenter un Run
  d'avant ce changement 400erait **après** l'archivage de l'original : archivé, sans remplaçant.

**Ce qui ne change pas, et ne changera jamais** : le repli **de lecture**. Un Run archivé dont le
payload porte `target_repo: null` (≈ 46 des 101 runs de dev) reste lisible, chiffrable et rangé sous le
dépôt racine du daemon — pas de bucket « Unassigned ». L'asymétrie est la conception : obligatoire là où
il y a un appelant à qui répondre 400, résolu là où il n'y a qu'un enregistrement passé à interpréter.

**Migration.** Aucune action requise sur une instance existante : les Triggers vivants nomment tous leur
dépôt, et l'UI envoyait déjà le champ. Les **appelants directs** de l'API — scripts, agents, `curl`,
harnais de test — doivent nommer `target_repo` ; le message d'erreur dit quoi passer. Les Runs archivés
restent lisibles sans intervention.

## 1.2.0

Deux changements cassants livrés sous un bump **mineur**, par décision explicite du mainteneur : la
v2.0.0 qu'avaient choisie les deux slices n'a jamais été taggée ni publiée (la dernière release est la
v1.0.0), et aucune des deux ruptures ne change la surface qu'un utilisateur pilote au quotidien.

### Cassant — `session_died` est le seul verdict de liveness, l'état `Stale` disparaît (#469)

Un nœud n'est plus déclaré mort sur une absence d'activité : seule la mort de sa session compte, et la
complétion sur fin de tour constatée est opt-in (ADR-0032, ADR-0012). L'état de nœud `Stale` est
supprimé — un client qui l'attendait ne le verra plus. Nouveau réglage `autocomplete_turn_end`
(`PDO_AUTOCOMPLETE_TURN_END`), off par défaut.

### Cassant — `GET`/`PUT /settings` perdent `image_source` et `dockerfile_path` (#471)

La source de l'image d'un Run sandboxé n'est plus un réglage d'instance. Elle appartient au
**profil de staging**, comme le contenu de home et l'env (ADR-0031 §9) : un axe par écran, l'écran
de réglages ne répondant plus qu'à « quel profil un Run prend par défaut ».

- `GET /settings` ne contient plus `image_source`, `dockerfile_path`, ni la disclosure
  `sandbox_image` (le tag que le Dockerfile résolu produisait) ;
- `PUT /settings` portant l'un de ces champs répond **400 en le nommant**, au lieu de 200 en
  l'ignorant silencieusement. Un champ simplement inconnu reste ignoré, comme avant ;
- les colonnes `instance_config.image_source` / `.dockerfile_path` deviennent **inertes**. Elles ne
  sont pas droppées, et une valeur qui y traîne déclenche **un** `warn!` au boot qui la nomme et
  renvoie vers le profil à éditer. Aucun Run n'est affecté ;
- `PDO_SANDBOX_IMAGE_SOURCE` et `PDO_SANDBOX_DOCKERFILE` sont **conservées** et repointées sur le
  nouveau défaut de profil : une instance headless n'a que des profils virtuels et pas d'UI, donc
  l'env reste son seul moyen de changer d'image sans POSTer un profil.

**Migration.** Aucune action requise si les deux réglages n'ont jamais été touchés (leur valeur par
défaut est désormais la constante `DEFAULT_PROFILE_IMAGE` : registre hash-dérivé sur le Dockerfile
seedé, donc un profil qui ne pose pas d'image produit exactement le même ref qu'avant). Si l'un des
deux portait une valeur, le boot le dit : reporter le choix sur le profil concerné via
**Settings → Manage staging profiles…**, ou sur la variable d'environnement du daemon.
