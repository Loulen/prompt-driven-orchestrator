# ADR-0037 — Un spawn demandé qui n'a pas eu lieu n'est jamais un `2xx`, et le sous-worktree se réutilise au lieu de se recouper

> Statut : accepted (grilling du 2026-07-31, issue #489). Vocabulaire : CONTEXT.md § « Contrat de
> réponse des commandes ». **Amende ADR-0025** sur deux points : §2 (« valider avant d'écrire »)
> s'étend au **kill**, pas seulement à l'append ; et §3 est corrigé sur le mot `noop` pour le throttle
> d'un spawn par nœud. **Amende ADR-0036** : une réutilisation de sous-worktree ne coupe rien, donc
> elle **reporte** le `base_sha` d'origine au lieu d'en dériver un nouveau. **Corrige ADR-0035** dans
> ses « Alternatives écartées », qui affirme que #489 était « séparé et additif » — il ne l'était ni
> l'un ni l'autre. Ne touche **ni** au tombstone d'ADR-0024 (qui garde son `410` et sa préséance),
> **ni** à la veille de vivacité d'ADR-0032 (qui lit des événements, jamais des statuts HTTP), **ni**
> au fail-fast d'ADR-0017. Suit ADR-0004 : aucun critère fermé sans test de couche ≥ 3.
>
> On garde une section `## Relations`, comme 0033 et 0035 — pas comme 0036, qui s'en passe. Ce n'est
> pas le gabarit majoritaire du dépôt ; c'est celui qui convient à une décision dont la moitié de la
> valeur est d'amender trois ADR voisines, et dans ce dépôt la relation s'écrit des deux côtés.

## Contexte

`restart_node` est le dernier levier de récupération sur un nœud coincé. Le bras faisait, dans cet
ordre : garde de transition, **kill de la session tmux**, append d'un `CommandIssued`, puis seulement
projection du Run, résolution du pipeline, recherche du nœud, et enfin `spawn_node`. Le retour de
`spawn_node` — un `SpawnOutcome` à cinq variantes, précisément conçu par ADR-0025 pour que l'appelant
puisse dire la vérité — était **jeté**. Pas de `let _ =`, pas de log : jeté. La réponse était
`200 {"ok":true}`, inconditionnellement.

Deux conséquences, et la seconde est celle qui coûte.

**Le mensonge par omission.** Un `node_id` absent du pipeline répondait `200`, après avoir tué une
session et écrit un événement d'audit pour un travail qui n'a jamais eu lieu. Un Run inexistant
répondait `404` — mais après les deux mêmes effets de bord. Un conteneur sandbox pas encore prêt
répondait `200`. Les cinq `SpawnOutcome` répondaient `200`, y compris `Failed`, dont trois des quatre
producteurs appendent un `RunFailed`.

**Le mensonge systématique.** Le sous-worktree d'un nœud est nommé purement à partir de
`(run, node, iter)`, et `restart_node` re-spawne sur le **même** `iter`. Le re-spawn rejouait donc
`git worktree add -b pdo/sub-<run>-<node>-iter-<N>` sur une branche déjà existante. Git refuse
(exit 255). Sur les nœuds `code-mutating` et `merge` — les deux seuls types qui possèdent un
sous-worktree, et la classe qui compte — **le levier échouait à 100 % en affirmant avoir réussi**. Le
`SpawnOutcome::Failed` qui en sortait était le seul témoin de la panne dans tout le processus : ce
bras-là est le seul des quatre producteurs de `Failed` qui n'appelle pas `fail_spawn_before_start`,
donc il n'appende rien.

Le résultat net, mesuré : session morte, zéro événement, nœud toujours projeté `Running` — puis, dans
les trente secondes, la veille de vivacité réécrit le nœud en `Failed` avec une cause **fausse**
(`session_died: tmux session … no longer exists`). L'opérateur part sur la piste tmux pour un bug git.

Et sous saturation du cap, un troisième mensonge se referme en gel : le bras tue la session mais
n'appende aucun événement de cycle de vie avant `spawn_node`, donc le nœud projette encore `Running`
quand le comptage d'admission passe. À `live == cap`, **le restart perd son propre slot au profit de
lui-même**, `Throttled` déterministe. Rien ne le sauve : `retry_waiting_nodes` n'a aucun timer,
`resume_run` considère un nœud throttlé comme possédé par le balayage, le boot recovery ne regarde que
`Running`/`AwaitingUser`, et le bouton Stop répond `409` parce que `node_stop` exige `Running`.

## Ce qu'on décide

### §1 — L'invariant

**Un spawn demandé qui n'a pas eu lieu n'est jamais un `2xx`.**

Posé comme propriété d'un type, pas comme liste de bras à relire : `RestartVerdict`
(`restart_verdict.rs`) ne porte **aucun statut**, et `restart_response` en est la seule propriétaire.
Le `match` de projection est **exhaustif, sans joker** : ajouter une variante ne compile plus tant
qu'on n'a pas décidé de son statut.

Différence structurelle avec `CompletionRefusal` (#490), et c'est celle qu'un relecteur ratera :
`CompletionRefusal` est un type *tout-refus*, sur lequel « jamais `2xx` » est un prédicat du type
entier. `RestartVerdict` mélange succès, sursis et panne. L'invariant clonable n'est donc **pas**
« jamais `2xx` » mais **une projection totale, variante par variante** : `Spawned`, `Waiting` et
`NoOp` **doivent** être `2xx`, tout le reste ne doit jamais l'être. Un test « jamais 2xx » naïf
échouerait sur `Spawned`.

Le type est **cloné du patron** de #490, pas réutilisé : ajouter une variante `2xx` à
`CompletionRefusal` rendrait `a_refusal_never_projects_to_a_2xx` rouge — à juste titre.

### §2 — La frontière contre-intuitive : `Throttled` reste `2xx`, et ce n'est pas un `noop`

`SpawnOutcome::Throttled` **a** appendé un `NodeWaiting`, qui flippe le statut du nœud à `Waiting`, et
`scheduler_dispatcher::waiting_nodes` → `retry_waiting_nodes` reprend réellement ce nœud-là. Ce n'est
donc ni un échec ni une absence d'effet : c'est une réservation. Elle répond
`200 {"ok":true,"waiting":true,"reason":…}`.

**Écart assumé, et c'est ici qu'on corrige ADR-0025 §3.** La convention `noop` de 0025
(`200 {"ok":true,"noop":true,"reason":…}`) couvre le throttle d'admission pour les quatre commandes de
boucle, et `ReEvalSummary::record_spawn` le fait encore pour elles. Elle **ne couvre pas** le throttle
d'un spawn **par nœud** : appeler « no-op » une réservation qui a changé le statut du nœud est un
petit mensonge, et #489 est une issue sur les petits mensonges. Pour cette classe de commandes, le
vocabulaire est `waiting`.

Corollaire à ne pas rater : `ReEvalSummary` n'est **pas** le chemin de cette route. Son `record_spawn`
aplatit `Refused | Deferred | Failed` dans un même seau `skipped`, que `into_response_body` rend en
`200 {"ok":true,"noop":true}` — c'est-à-dire exactement le mensonge de #490, avec le mot `noop` faux
en prime.

### §3 — L'ordre : toute cause connaissable se teste avant l'effet destructeur

ADR-0025 §2 dit « valider avant d'écrire ». **Cette ADR l'étend du seul append au kill.** Un `4xx` qui
arrive après la destruction d'une session n'est pas une validation, c'est un constat.

Sur cette route, six sondes passent donc avant le premier effet de bord : garde de transition, Run
présent, pipeline du Run lisible et parsable, nœud présent dans **ce** pipeline (son snapshot, pas la
bibliothèque — ADR-0025 §2), précondition sandbox (`sandbox_spawn_block`, pure), et classification du
sous-worktree (§6). La dernière est un appel `git` en lecture ; le coût est assumé au nom de la règle.

### §4 — `recoverable` est uniformément `true` sur les refus, et le bit utile est neuf

La définition de `recoverable` (ADR-0035) est « le daemon a-t-il **déjà** enregistré l'issue
terminale ? ». Sur cette route, aucun refus n'enregistre quoi que ce soit : le champ ne porte donc
aucun bit avant le kill. On le ship quand même — ADR-0035 §3 déclare la forme transversale, et un
client qui lit `body.recoverable` ne doit pas trouver `undefined` selon la route. Il redevient
informatif sur le `500`, où il vaut `!run_failed`.

**Le bit qui compte ici est `session_killed`.** Il répare une contradiction : « un `4xx` signifie que
rien n'a été touché » est faux pour les deux courses post-kill (`Deferred` et `Refused` re-évalués
dans `spawn_node` contre une projection plus fraîche). La règle correcte est :

> `session_killed:false` — rien n'a été touché.
> `session_killed:true` — la session est morte et **rien ne l'a remplacée** : le nœud a besoin d'un
> autre levier, pas d'un retry de celui-ci.

On discrimine dans le **corps**, jamais en tordant un statut (ADR-0035 §3) : on ajoute un champ.

### §5 — Portée : les surfaces de spawn par nœud

La règle vaut pour `restart_node` (#489) et pour `node_retry` (#487), qui porte la même faute sur une
autre route. Écrite une fois pour les deux, sinon ce serait 0037 puis 0038 pour la même règle.

Elle ne s'étend **pas** aux quatre commandes de boucle, qui gardent le vocabulaire d'ADR-0025.

### §6 — La réutilisation du sous-worktree

`ensure_sub_worktree` remplace `create_sub_worktree` aux deux sites de production. Le sous-worktree se
classe en **quatre** états — trois ne suffisent pas :

| État | Ce que c'est | Ce qu'on fait |
|---|---|---|
| `Absent` | ni répertoire (ou vide), ni ref de branche, ni enregistrement | on coupe |
| `Reusable` | enregistré sur `refs/heads/<sub_branch>`, non prunable | **on réutilise en place, aucun appel git mutant** |
| `Recyclable` | prunable, HEAD détaché, ou branche orpheline sans worktree | on reape, puis on coupe |
| `Occupied` | branche checkoutée dans un autre worktree vivant, ou répertoire non-worktree non vide | on refuse et on **nomme** ce qui le tient |

Le quatrième état est la décision. Un découpage à trois (`Absent` / `Reusable` / « bloqué, donc
reap ») rend `restart_node` destructeur : un verrou git périmé sur un arbre **sale** n'est pas
« déjà inutilisable », c'est précisément le travail que #489 existe pour sauver. Il faut séparer
*recyclable* (rien à perdre) d'*occupé* (tenu par un tiers).

Le prédicat de réutilisation est « worktree **enregistré** sur la bonne branche », jamais « la branche
pointe sur la base attendue » : le second est satisfait par un agent coincé qui n'a rien commité mais
a un arbre sale, et le « fix » le détruirait.

La sonde autoritaire est `git worktree list --porcelain` depuis `repo_root`. Mesuré :
`git -C <dir> rev-parse --abbrev-ref HEAD` sur un simple répertoire *à l'intérieur* du repo remonte au
worktree principal et répond `main` — une sonde par répertoire mentirait en silence. Le match se fait
sur le **chemin absolu**, jamais sur le basename : `.git/worktrees/` est nommé par le basename, donc
tous les nœuds collisionnent sur `iter-1` et git désambiguïse en `iter-11`, `iter-12`…

**`orphan_to_reap` n'est armé que sur les branches qui ont créé quelque chose.** Sur une réutilisation,
n'importe quelle panique dans le span isolé de `spawn_node` atteindrait `fail_spawn_before_start` →
`reap_orphan_sub_worktree` → `worktree remove --force`, qui réussit sur un arbre sale. Gaté, le chemin
d'avortement appende `RunFailed` **sans rien détruire** : le Run part terminal avec le travail intact,
`resume_run` le ré-ouvre, et la classification suivante rend `Reusable`. Le chemin devient idempotent
et auto-réparant, là où le résidu condamnait le nœud à vie. L'invariant #279 (« un spawn avorté ne
laisse pas d'orphelin ») n'est pas affaibli : il ne portait que sur ce que le spawn a **créé**.

**Le `base_sha` se reporte** (c'est l'amendement à ADR-0036, et le point le plus subtil du lot). Une
réutilisation ne coupe rien, donc elle rend le `base_sha` du `NodeStarted` précédent de la **même**
itération. Les deux autres réponses évidentes sont pires que le bug :

1. `rev_parse(dir, "HEAD")` sur un worktree réutilisé rend le commit **du nœud**, jamais le tip
   pipeline → l'échappatoire d'adoption d'ADR-0036 serait **silencieusement désactivée pour tout nœud
   redémarré, à vie** ;
2. le tip de la branche pipeline **au moment de la réutilisation** **arme l'adoption à faux** : le
   garde passe, et `resolve_in_node_favour` peut écraser le travail d'un nœud voisin mergé depuis la
   coupe d'origine.

`ensure_sub_worktree` ne peut donc pas calculer la base elle-même : il faut la lui passer. Le
commentaire d'invariant de `merge_action.rs` (« `restart_node` re-cuts the sub-worktree from wherever
the branch is then ») est corrigé en conséquence — il décrivait le comportement d'avant.

### §7 — Deux corrections que la réutilisation rend obligatoires

Elles ne sont pas des à-côtés : la tranche B promeut deux bugs latents en bugs systématiques.

- **`reap_orphan_sub_worktree` était cassé.** Le `if sub_worktree_dir.exists()` sautait le
  `worktree remove --force` quand le répertoire avait déjà disparu, et un `branch -D` sur une branche
  qu'un enregistrement épingle échoue (`cannot delete branch … used by worktree at …`, exit 1). Le
  reap laissait donc les **deux** verrous en place. Correctif mesuré : `worktree prune` **avant** le
  `branch -D`, et `remove` inconditionnel. C'est aussi le levier 2 de #498.
- **`commit_and_merge_sub_worktree_inner` jetait le statut de son `git add -A`.** Chaîne mesurée avec
  un `index.lock` résiduel : `add` échoue (128) → `diff --cached --quiet` répond 0 → aucun commit
  n'est pris → `git merge` dit « Already up to date » → `pdo complete` rend `MergeResult::Success` sur
  **100 % du travail perdu**, sans conflit, sans événement, sans trace. Exactement la perte silencieuse
  qu'ADR-0004 interdit, et rien de #503 ne se déclenche. Le statut est désormais vérifié et l'échec
  `bail!`e — il ressort en `CompletionRefusal::MergeFailed`, un `500` déjà géré.

### §8 — Le slot qu'un spawn reprend ne se compte pas contre lui

Le comptage d'admission exclut le slot `(run_id, node_id, iter)` que le spawn courant est en train de
reprendre. La clé est le **triplet complet** : le comptage est global à tous les Runs alors que les
ids de nœuds sont locaux au pipeline, donc deux Runs concurrents du même pipeline ont tous deux un
`implementer` à l'`iter 1` — une clé aveugle au Run écarterait la session vivante de l'**autre** Run
et **dépasserait le cap**, exactement l'effondrement que le module existe pour empêcher.

L'exclusion n'est inconditionnelle qu'en apparence : elle ne retranche que si ce triplet-là détient
*réellement* une session, et le garde de transition n'autorise le re-spawn d'une itération vivante que
pour une seule raison — ce spawn la remplace. Elle est calculée **sous le verrou d'admission**, depuis
la même projection tous-Runs que le comptage : réutiliser la projection pré-verrou rouvrirait la course
check-and-reserve que le verrou ferme.

Et `kill_node` réveille désormais les nœuds `Waiting` : c'est le geste le plus probable pour libérer le
slot qu'un restart throttlé attend, et `retry_waiting_nodes` n'a aucun timer.

## Alternatives écartées

- **Réutiliser `CompletionRefusal`** au lieu de cloner son patron. Il est adossé au chemin de
  complétion : `recoverable()` y est un `matches!` sur deux variantes de complétion, et y ajouter une
  variante `2xx` falsifierait `only_the_two_still_your_turn_refusals_are_recoverable`.
- **Trois slugs discriminants pour le refus du garde** (`run_not_live`, `iteration_already_completed`,
  `newer_iteration_live`). Mesuré : `Verdict::Reject` n'a **aucun** discriminant, ses dix sites de
  construction passent tous par un helper privé qui ne porte qu'une `String`, et les distinguer
  coûterait dix constructions plus neuf destructurations dans cinq fichiers — un refactor de la taille
  de la tranche B, introduit en fraude dans une tranche de véracité. #490 a déjà tranché ce cas exact
  sur cette forme exacte (`completion_rejected`, un slug, la prose dans `message`, épinglé par un
  test) ; diverger donnerait deux discriminations filaires différentes au **même** garde. Bonus : ça
  élimine un slug **faux** — le garde teste `live_iter != iter`, donc un restart de l'iter 5 pendant
  que l'iter 1 vit tomberait dans `newer_iteration_live`, qui encoderait un fait que le garde ne teste
  pas. Le discriminant `RejectKind` reste une issue à part, co-possédée avec #487.
- **`202 Accepted` pour le throttle.** Défendable, mais ce serait le premier `202` du dépôt pour un
  bras de commande, et ADR-0025 a déjà légiféré ce `SpawnOutcome` en `200` : la discrimination
  appartient au corps.
- **Un mapper unique `SpawnOutcome → Response`** partagé par les vingt-deux triplets de la surface.
  L'addendum #236 d'ADR-0009 le prouve *lossy*.
- **Garder le `200` en enrichissant le corps** — ce que #489 demandait à l'origine, et ce qu'ADR-0035
  supposait. Écarté : le statut *est* le mensonge, cf. l'errata posé sur 0035.
- **Rafraîchir la base d'un sous-worktree réutilisé.** Mesuré dangereux : `git merge <base>` dans un
  sous-worktree sale échoue (« Your local changes … would be overwritten »), et commiter d'abord livre
  à un agent frais un arbre truffé de marqueurs de conflit. On **dit** `base_moved` et on laisse
  l'opérateur choisir `node_retry`, qui *est* l'outil « base fraîche ».

## Limites acceptées

- **Un verrou git périmé est signalé, pas supprimé.** `stale_git_lock` remonte dans le corps et dans
  un `warn!`, et le worktree reste réutilisable. Refuser supprimerait le dernier levier de récupération
  sur un état que le restart peut améliorer (un agent frais peut retirer le verrou lui-même) ; le
  supprimer nous-mêmes est l'opération contre laquelle git met en garde, et PDO ne peut pas prouver que
  l'écrivain est mort — #485 est le précédent qui coûte cher. Le filet est §7 : la panne, si elle
  survient, est désormais bruyante.
- **La base n'est pas rafraîchie**, cf. ci-dessus. `base_moved` est calculé et remonté ; rien de plus.
- **La vérité filaire livrée ici n'est observable par aucun humain via l'UI.** `api.ts` appelle
  `restartNode` en `responseMode:"void"` + `catch {}` — le corps *et* l'erreur sont jetés — et le seul
  bouton câblé vit dans une bannière gatée `node.status === "stale"`, que plus rien ne produit depuis
  #469. Trou connu, propriété de #492, énoncé pour qu'un prochain grilling ne le fiche pas en
  régression. Zéro travail frontend dans ce lot.
- **`SpawnOutcome::Spawned` est rendu même quand `tmux_session_manager::spawn` a échoué** — l'erreur
  est loguée et le flux continue. Le `200 {spawned}` de cette ADR est donc lui-même non véridique dans
  ce cas. Bug distinct, à ficher, cité ici pour ne pas créditer la décision d'un cas qu'elle ne couvre
  pas.
- **`restart_node` n'appelle jamais `invalidate_nodes`** : les artefacts partiels de la tentative
  avortée survivent au même `iter`, et un `pdo complete` ultérieur peut valider l'**ancien**
  `output.md`. Vrai avant comme après.
- **Le kill reste nu**, et non `reap_node_session` (#488). Mécaniquement le reap serait correct, mais
  le snapshot de pane ne serait **jamais servi** : `GET …/pane` ne le sert que sur une itération
  terminale, et un restart laisse le nœud non terminal. CONTEXT.md § *Reap sur état terminal* nomme
  déjà ce trou ; cette ADR ne le ferme pas.
- **Les corps d'erreur `text/plain`** (Run absent, pipeline illisible/inparsable) restent tels quels.
  Les normaliser est le périmètre de **#491**, et le faire ici mélangerait une rupture et un nettoyage
  sous un seul bump.
- **`retry_waiting_nodes` n'a toujours aucun timer**, et deux autres libérateurs de slot ne le
  réveillent pas (les bras halt/pause, et `boot_recovery` qui échoue les `Running` orphelins). #489
  ferme `kill_node` seul ; le reste est fiché.
- **Le comptage d'admission reste par nœud, pas par itération** (#453) : N laps parallèles d'un nœud
  consomment **un** slot. Pré-existant ; l'exclusion ne se construit pas sur une hypothèse contraire.

## Relations

- Issue **#489** (cette décision).
- **ADR-0025** (#327) — amendée deux fois : §2 s'étend au kill, §3 est corrigé sur `noop` pour les
  commandes de spawn par nœud.
- **ADR-0035** (#490) — la forme du corps (`error` = slug, `recoverable`, prose dans `message`) est
  reprise telle quelle ; ses « Alternatives écartées » et ses « Relations » portent un errata, parce
  qu'elles décrivent #489 comme séparé et additif.
- **ADR-0036** (#503) — amendée : le `base_sha` d'une itération réutilisée est **reporté**, pas
  re-dérivé, et le commentaire d'invariant de `merge_action.rs` est corrigé.
- **ADR-0032** (#469) — indemne : la veille de vivacité lit des événements, jamais un statut HTTP. Elle
  cesse simplement d'inventer un `session_died` là où un `worktree add` avait échoué.
- **ADR-0004** — les critères sont fermés en couche 1 (projection totale, `match` sans joker) et en
  couche 3a (`tests/restart_node_truth.rs`, `tests/sub_worktree_survive.rs`, `tests/sandbox_tracer.rs`).
- **#487** (`node_retry`) — §5 est écrit pour lui aussi ; il consomme le patron de projection posé ici.
- **#498** — sa slice A doit consommer `ensure_sub_worktree` via le bras `Recyclable`, pas le
  réimplémenter. Son levier 3 (`SpawnOutcome::Failed` du scheduler doit appender un événement) reste
  chez elle : le chemin du scheduler n'a aucune réponse HTTP, l'événement y est le seul canal.
- **#491** vient après (corps `text/plain` vs JSON). **#492** possède le trou côté UI.
