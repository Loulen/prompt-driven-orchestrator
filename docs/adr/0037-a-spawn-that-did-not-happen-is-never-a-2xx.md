# ADR-0037 — Un spawn demandé qui n'a pas eu lieu n'est jamais un `2xx`, et le sous-worktree se réutilise au lieu de se recouper

Sans cette ADR, on répondrait `200 {"ok":true}` à un `restart_node` en jetant le verdict du spawn, et on
recouperait inconditionnellement le sous-worktree du nœud — c'est-à-dire qu'on détruirait le travail que
le levier existe pour sauver, en affirmant avoir réussi. Rien dans le code ni les tests ne le dit : le
type de retour du spawn est simplement ignoré.

> Statut : accepted (#489). **Amende ADR-0025** : §2 (« valider avant d'écrire ») s'étend au **kill** ;
> §3 est corrigé sur le mot `noop` pour le throttle d'un spawn par nœud. **Amende ADR-0036** : une
> réutilisation de sous-worktree ne coupe rien, donc elle **reporte** le `base_sha` d'origine.
> **Corrige ADR-0035** dans ses « Alternatives écartées », qui décrivait #489 comme « séparé et
> additif » — il ne l'était ni l'un ni l'autre. Ne touche ni au tombstone d'ADR-0024, ni à la veille de
> vivacité d'ADR-0032 (qui lit des événements, jamais des statuts HTTP), ni au fail-fast d'ADR-0017.

## Contexte

`restart_node` faisait, dans cet ordre : garde de transition, **kill de la session tmux**, append d'un
événement d'audit, puis seulement projection, résolution du pipeline, recherche du nœud, spawn. Le
verdict du spawn — cinq issues conçues par ADR-0025 pour que l'appelant puisse dire la vérité — était
**jeté**. Trois mensonges, et le deuxième est celui qui coûte :

- **Par omission.** Un `node_id` absent du pipeline répondait `200`, après avoir tué une session et écrit
  un audit pour un travail qui n'a jamais eu lieu.
- **Systématique.** Le sous-worktree est nommé à partir de `(run, node, iter)` et `restart_node` re-spawne
  sur le **même** `iter` : le re-spawn rejouait la création d'une branche déjà existante, que git refuse.
  Sur les nœuds `code-mutating` et `merge` — les seuls qui possèdent un sous-worktree — **le levier
  échouait à 100 % en affirmant avoir réussi**. Session morte, zéro événement, nœud toujours projeté
  `Running` — puis la veille de vivacité le réécrit en `Failed` avec une cause **fausse**
  (`session_died`), et l'opérateur part sur la piste tmux pour un bug git.
- **Le gel sous saturation.** Aucun événement de cycle de vie n'est appendé avant le spawn, donc le nœud
  projette encore `Running` au comptage d'admission. À `live == cap`, **le restart perd son propre slot au
  profit de lui-même**, et rien ne le sauve.

## Ce qu'on décide

### §1 — L'invariant

**Un spawn demandé qui n'a pas eu lieu n'est jamais un `2xx`.** Posé comme propriété d'un type, pas comme
liste de bras à relire : le verdict de restart ne porte **aucun** statut, sa projection HTTP en est la
seule propriétaire, et le match est **exhaustif, sans joker** — ajouter une variante ne compile plus tant
qu'on n'a pas décidé de son statut.

Différence structurelle avec le type de refus de complétion (ADR-0035), et c'est celle qu'un relecteur
ratera : celui-là est un type *tout-refus*, sur lequel « jamais `2xx` » est un prédicat du type entier. Le
verdict de restart mélange succès, sursis et panne. L'invariant clonable n'est donc **pas** « jamais
`2xx` » mais **une projection totale, variante par variante** : `Spawned`, `Waiting` et `NoOp` **doivent**
être `2xx`, tout le reste ne doit jamais l'être. Un test « jamais 2xx » naïf échouerait sur `Spawned`. Le
type est **cloné du patron**, pas réutilisé : ajouter une variante `2xx` au type de refus de complétion
falsifierait son propre invariant.

### §2 — La frontière contre-intuitive : le throttle reste `2xx`, et ce n'est pas un `noop`

Un spawn throttlé **a** appendé une réservation qui flippe le nœud à `Waiting`, et le balayage d'admission
reprend réellement ce nœud. Ni échec ni absence d'effet : une réservation. Elle répond
`200 {"ok":true,"waiting":true,"reason":…}`.

**Écart assumé, et c'est ici qu'on corrige ADR-0025 §3.** La convention `noop` couvre le throttle
d'admission des quatre commandes de boucle, qui la gardent ; elle **ne couvre pas** le throttle d'un spawn
**par nœud** — appeler « no-op » une réservation qui a changé le statut du nœud est un petit mensonge.
Corollaire : le résumé agrégé des commandes de boucle n'est **pas** le chemin de cette route — il aplatit
refus, sursis et échec dans un même seau rendu `noop`.

### §3 — L'ordre : toute cause connaissable se teste avant l'effet destructeur

ADR-0025 §2 dit « valider avant d'écrire » ; **cette ADR l'étend du seul append au kill**. Un `4xx` qui
arrive après la destruction d'une session n'est pas une validation, c'est un constat. Six sondes passent
donc avant le premier effet de bord : garde de transition, Run présent, pipeline lisible et parsable, nœud
présent dans **ce** pipeline (son snapshot, pas la bibliothèque), précondition sandbox (ADR-0030), et
classification du sous-worktree (§6). La dernière est un appel git en lecture ; le coût est assumé.

### §4 — `recoverable` est uniformément `true` sur les refus, et le bit utile est neuf

Aucun refus de cette route n'enregistre quoi que ce soit, donc `recoverable` (ADR-0035) n'y porte aucun bit
avant le kill. On le ship quand même : un client qui lit `body.recoverable` ne doit pas trouver `undefined`
selon la route. **Le bit qui compte ici est `session_killed`**, qui répare la contradiction « un `4xx`
signifie que rien n'a été touché » — faux pour les deux courses post-kill :

> `session_killed:false` — rien n'a été touché.
> `session_killed:true` — la session est morte et **rien ne l'a remplacée** : le nœud a besoin d'un autre
> levier, pas d'un retry de celui-ci.

On discrimine dans le **corps**, jamais en tordant un statut.

### §5 — Portée : les surfaces de spawn par nœud

La règle vaut pour `restart_node` et pour `node_retry` (#487), qui porte la même faute sur une autre route.
Elle ne s'étend **pas** aux quatre commandes de boucle, qui gardent le vocabulaire d'ADR-0025.

### §6 — La réutilisation du sous-worktree

La coupe inconditionnelle est remplacée par une classification en **quatre** états — trois ne suffisent pas :

| État | Ce que c'est | Ce qu'on fait |
|---|---|---|
| `Absent` | ni répertoire (ou vide), ni ref de branche, ni enregistrement | on coupe |
| `Reusable` | enregistré sur la bonne branche, non prunable | **on réutilise en place, aucun appel git mutant** |
| `Recyclable` | prunable, HEAD détaché, ou branche orpheline sans worktree | on reape, puis on coupe |
| `Occupied` | branche checkoutée dans un autre worktree vivant, ou répertoire non-worktree non vide | on refuse et on **nomme** ce qui le tient |

Le quatrième état est la décision. Un découpage à trois (`Absent` / `Reusable` / « bloqué, donc reap »)
rend `restart_node` destructeur : une opération git interrompue sur un arbre **sale** n'est pas « déjà
inutilisable », c'est précisément le travail que #489 existe pour sauver.

Le prédicat de réutilisation est « worktree **enregistré** sur la bonne branche », jamais « la branche
pointe sur la base attendue » : le second est satisfait par un agent coincé qui n'a rien commité mais a un
arbre sale, et le « fix » le détruirait.

La sonde autoritaire est l'inventaire des worktrees depuis la racine du repo : demander sa branche à un
simple répertoire *à l'intérieur* du repo remonte au worktree principal et répond `main` — une sonde par
répertoire mentirait en silence. Le match se fait sur le **chemin absolu**, jamais sur le basename :
l'enregistrement git est nommé par basename, donc tous les nœuds collisionnent sur `iter-1`.

**Le nettoyage d'avortement n'est armé que sur les branches qui ont créé quelque chose.** Sur une
réutilisation, n'importe quelle panique du spawn atteindrait sinon le reap d'orphelin — qui réussit sur un
arbre sale. Gaté, le chemin d'avortement appende `RunFailed` **sans rien détruire** : le Run part terminal
avec le travail intact, `resume_run` le ré-ouvre, et la classification suivante rend `Reusable`.
L'invariant « un spawn avorté ne laisse pas d'orphelin » n'est pas affaibli : il ne portait que sur ce que
le spawn a **créé**.

**Le `base_sha` se reporte** (amendement à ADR-0036, et le point le plus subtil du lot). Une réutilisation
ne coupe rien, donc elle rend le `base_sha` du `NodeStarted` précédent de la **même** itération. Les deux
autres réponses évidentes sont pires que le bug : relire le HEAD du worktree réutilisé rend le commit **du
nœud**, jamais le tip pipeline → l'échappatoire d'adoption d'ADR-0036 serait **silencieusement désactivée
pour tout nœud redémarré, à vie** ; et le tip de la branche pipeline au moment de la réutilisation **arme
l'adoption à faux** — le garde passe, et l'adoption peut écraser le travail d'un nœud voisin mergé depuis.
La classification ne peut donc pas calculer la base elle-même : il faut la lui passer.

### §7 — Le slot qu'un spawn reprend ne se compte pas contre lui

Le comptage d'admission exclut le slot `(run_id, node_id, iter)` que le spawn courant reprend. La clé est
le **triplet complet** : le comptage est global à tous les Runs alors que les ids de nœuds sont locaux au
pipeline, donc deux Runs concurrents du même pipeline ont un nœud homonyme à la même itération — une clé
aveugle au Run écarterait la session vivante de l'**autre** Run et **dépasserait le cap**. L'exclusion ne
retranche que si ce triplet détient réellement une session, et elle est calculée **sous le verrou
d'admission** : la recalculer sur une projection pré-verrou rouvrirait la course check-and-reserve.

Et `kill_node` réveille désormais les nœuds `Waiting` : c'est le geste le plus probable pour libérer le
slot qu'un restart throttlé attend.

## Alternatives écartées

- **Réutiliser le type de refus de complétion** au lieu de cloner son patron : il est adossé au chemin de
  complétion, et y ajouter une variante `2xx` falsifierait son invariant.
- **Trois slugs discriminants pour le refus du garde de transition.** Mesuré : le refus du garde n'a
  **aucun** discriminant interne, et le distinguer coûterait un refactor de la taille de la tranche. Un des
  trois slugs aurait été **faux** (il encodait un fait que le garde ne teste pas). #515 a depuis livré une
  cause typée, mais cette route l'aplatit toujours sur `restart_refused` ; la discrimination filaire reste
  une issue à part.
- **`202 Accepted` pour le throttle.** Défendable, mais ce serait le premier `202` du dépôt pour un bras de
  commande, et ADR-0025 a légiféré ce cas en `200` : la discrimination appartient au corps.
- **Un mapper unique spawn → réponse** partagé par toute la surface de commandes : l'addendum #236
  d'ADR-0009 le prouve *lossy*.
- **Garder le `200` en enrichissant le corps** — ce que #489 demandait à l'origine. Le statut *est* le
  mensonge.
- **Rafraîchir la base d'un sous-worktree réutilisé.** Mesuré dangereux : merger la base dans un
  sous-worktree sale échoue, et commiter d'abord livre à un agent frais un arbre truffé de marqueurs de
  conflit. On **dit** `base_moved` et on laisse l'opérateur choisir `node_retry`, qui *est* l'outil « base
  fraîche ».

## Limites acceptées

- **Une opération git interrompue est inventoriée et routée, jamais supprimée (#516).** Tous les marqueurs
  présents remontent — `interrupted_git_ops` (tableau, `[]` si rien) — et la consigne différenciée arrive
  **dans le préambule du nœud re-spawné**, pas seulement dans le corps. Le worktree reste réutilisable. Un
  scanner qui ne remontait que le premier marqueur laissait un `index.lock` masquer un `MERGE_HEAD`, et le
  merge-back prenait un commit à 2 parents silencieux. PDO ne peut pas prouver l'écrivain mort ; l'agent
  frais résout.
- **La base n'est pas rafraîchie** : `base_moved` est calculé et remonté, rien de plus.
- **La vérité filaire livrée ici n'est observable par aucun humain via l'UI** : le client jette le corps et
  l'erreur, et le seul bouton câblé vit dans une bannière que plus rien ne produit depuis #469. Trou connu,
  propriété de **#492**.
- **`restart_node` n'invalide jamais les artefacts** : les sorties partielles de la tentative avortée
  survivent au même `iter`, et une complétion ultérieure peut valider l'**ancien** output.
- **Le kill reste nu**, pas un reap : mécaniquement le reap serait correct, mais le snapshot de pane ne
  serait jamais servi (il ne l'est que sur une itération terminale, et un restart laisse le nœud non
  terminal).
- **Les corps d'erreur en texte brut** (Run absent, pipeline illisible) restent tels quels ; les normaliser
  est le périmètre de **#491**.
- **Le comptage d'admission reste par nœud, pas par itération** (#453) : pré-existant ; l'exclusion ne se
  construit pas sur une hypothèse contraire.

Deux limites d'origine sont **fermées** : un `Err` de spawn tmux appende désormais `NodeFailed` + reap gaté
+ `RunFailed`, donc la panne tmux retombe sous §1 et §3 au lieu d'y échapper (#508) ; et le retry des nœuds
en attente est re-drivé par `re_evaluate_after_command` et `boot_recovery` — fix événementiel, toujours pas
de timer (#509).
