# Changelog

Ce fichier ne consigne que les changements **cassants** et les notes de version qui ne se
déduisent pas d'un titre de commit. L'historique complet vit dans le git log et dans les
[Releases GitHub](https://github.com/Loulen/prompt-driven-orchestrator/releases), générées
depuis les commits.

Le projet suit le versionnement sémantique. Il n'a **aucun objectif de compatibilité
ascendante** : la casse se signale ici et par un bump majeur, jamais en gardant des champs
morts. Seule contrainte non négociable — les **données historiques restent lisibles** : un Run
archivé s'ouvre et se chiffre quelle que soit la version qui a écrit son payload.

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
