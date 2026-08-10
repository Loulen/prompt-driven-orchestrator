# Changelog

Ce fichier ne consigne que les changements **cassants** et les notes de version qui ne se
déduisent pas d'un titre de commit. L'historique complet vit dans le git log et dans les
[Releases GitHub](https://github.com/Loulen/prompt-driven-orchestrator/releases), générées
depuis les commits.

Le projet suit le versionnement sémantique. Il n'a **aucun objectif de compatibilité
ascendante** : la casse se signale ici et par un bump majeur, jamais en gardant des champs
morts. Seule contrainte non négociable — les **données historiques restent lisibles** : un Run
archivé s'ouvre et se chiffre quelle que soit la version qui a écrit son payload.

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
