# ADR-0037 — Un spawn demandé qui n'a pas eu lieu n'est jamais un `2xx`, et le sous-worktree se réutilise au lieu de se recouper

> Statut : accepted (grilling du 2026-07-31, issue #489). Vocabulaire : CONTEXT.md § « Contrat de
> réponse des commandes ». **Amende ADR-0025** sur deux points : §2 (« valider avant d'écrire »)
> s'étend au **kill**, pas seulement à l'append ; et §3 est corrigé sur le mot `noop` pour le
> throttle d'un spawn par nœud. **Amende ADR-0036** : une réutilisation de sous-worktree ne coupe
> rien, donc elle **reporte** le `base_sha` d'origine au lieu d'en dériver un nouveau. **Corrige
> ADR-0035** dans ses « Alternatives écartées », qui décrivait #489 comme « séparé et additif » —
> il ne l'était ni l'un ni l'autre. Ne touche **ni** au tombstone d'ADR-0024 (qui garde son `410`
> et sa préséance), **ni** à la veille de vivacité d'ADR-0032 (qui lit des événements, jamais des
> statuts HTTP), **ni** au fail-fast d'ADR-0017. Suit ADR-0004 : aucun critère fermé sans test de
> couche ≥ 3.

## Contexte

`restart_node` est le dernier levier de récupération sur un nœud coincé. Le bras faisait, dans cet
ordre : garde de transition, **kill de la session tmux**, append d'un événement d'audit, puis
seulement projection du Run, résolution du pipeline, recherche du nœud, et enfin le spawn. Le
retour du spawn — cinq issues possibles, précisément conçues par ADR-0025 pour que l'appelant
puisse dire la vérité — était **jeté**. La réponse était `200 {"ok":true}`, inconditionnellement.

Trois mensonges, et le deuxième est celui qui coûte.

**Le mensonge par omission.** Un `node_id` absent du pipeline répondait `200`, après avoir tué une
session et écrit un événement d'audit pour un travail qui n'a jamais eu lieu. Les cinq issues du
spawn répondaient `200`, y compris l'échec, dont la plupart des producteurs appendent `RunFailed`.

**Le mensonge systématique.** Le sous-worktree d'un nœud est nommé purement à partir de
`(run, node, iter)`, et `restart_node` re-spawne sur le **même** `iter`. Le re-spawn rejouait donc
la création d'une branche déjà existante — que git refuse. Sur les nœuds `code-mutating` et
`merge` — les deux seuls types qui possèdent un sous-worktree, et la classe qui compte — **le
levier échouait à 100 % en affirmant avoir réussi**, sans rien appender. Résultat net, mesuré :
session morte, zéro événement, nœud toujours projeté `Running` — puis, dans les trente secondes, la
veille de vivacité réécrit le nœud en `Failed` avec une cause **fausse** (`session_died`).
L'opérateur part sur la piste tmux pour un bug git.

**Le gel sous saturation.** Le bras tue la session mais n'appende aucun événement de cycle de vie
avant le spawn, donc le nœud projette encore `Running` quand le comptage d'admission passe. À
`live == cap`, **le restart perd son propre slot au profit de lui-même**, throttlé déterministe —
et rien ne le sauve : le retry des nœuds en attente n'a aucun timer, et aucun autre chemin ne
reprend un nœud dans cet état.

## Ce qu'on décide

### §1 — L'invariant

**Un spawn demandé qui n'a pas eu lieu n'est jamais un `2xx`.**

Posé comme propriété d'un type, pas comme liste de bras à relire : le verdict de restart ne porte
**aucun** statut, sa projection HTTP en est la seule propriétaire, et le match de projection est
**exhaustif, sans joker** — ajouter une variante ne compile plus tant qu'on n'a pas décidé de son
statut.

Différence structurelle avec le type de refus de complétion (#490), et c'est celle qu'un relecteur
ratera : celui-là est un type *tout-refus*, sur lequel « jamais `2xx` » est un prédicat du type
entier. Le verdict de restart mélange succès, sursis et panne. L'invariant clonable n'est donc
**pas** « jamais `2xx` » mais **une projection totale, variante par variante** : `Spawned`,
`Waiting` et `NoOp` **doivent** être `2xx`, tout le reste ne doit jamais l'être. Un test « jamais
2xx » naïf échouerait sur `Spawned`.

Le type est **cloné du patron** de #490, pas réutilisé : ajouter une variante `2xx` au type de
refus de complétion falsifierait son propre invariant — à juste titre.

### §2 — La frontière contre-intuitive : le throttle reste `2xx`, et ce n'est pas un `noop`

Un spawn throttlé **a** appendé une réservation qui flippe le statut du nœud à `Waiting`, et le
balayage d'admission reprend réellement ce nœud-là. Ce n'est donc ni un échec ni une absence
d'effet : c'est une réservation. Elle répond `200 {"ok":true,"waiting":true,"reason":…}`.

**Écart assumé, et c'est ici qu'on corrige ADR-0025 §3.** La convention `noop` de 0025 couvre le
throttle d'admission pour les quatre commandes de boucle, qui la gardent. Elle **ne couvre pas** le
throttle d'un spawn **par nœud** : appeler « no-op » une réservation qui a changé le statut du nœud
est un petit mensonge, et #489 est une issue sur les petits mensonges. Pour cette classe de
commandes, le vocabulaire est `waiting`.

Corollaire à ne pas rater : le résumé agrégé des commandes de boucle n'est **pas** le chemin de
cette route — il aplatit refus, sursis et échec dans un même seau rendu `noop`, c'est-à-dire
exactement le mensonge de #490 avec le mot `noop` faux en prime.

### §3 — L'ordre : toute cause connaissable se teste avant l'effet destructeur

ADR-0025 §2 dit « valider avant d'écrire ». **Cette ADR l'étend du seul append au kill.** Un `4xx`
qui arrive après la destruction d'une session n'est pas une validation, c'est un constat.

Sur cette route, six sondes passent donc avant le premier effet de bord : garde de transition, Run
présent, pipeline du Run lisible et parsable, nœud présent dans **ce** pipeline (son snapshot, pas
la bibliothèque — ADR-0025 §2), précondition sandbox (ADR-0030), et classification du sous-worktree
(§6). La dernière est un appel git en lecture ; le coût est assumé au nom de la règle.

### §4 — `recoverable` est uniformément `true` sur les refus, et le bit utile est neuf

La définition de `recoverable` (ADR-0035) est « le daemon a-t-il **déjà** enregistré l'issue
terminale ? ». Sur cette route, aucun refus n'enregistre quoi que ce soit : le champ ne porte donc
aucun bit avant le kill. On le ship quand même — ADR-0035 §3 déclare la forme transversale, et un
client qui lit `body.recoverable` ne doit pas trouver `undefined` selon la route. Il redevient
informatif sur le `500`.

**Le bit qui compte ici est `session_killed`.** Il répare une contradiction : « un `4xx` signifie
que rien n'a été touché » est faux pour les deux courses post-kill (un refus re-évalué dans le
spawn contre une projection plus fraîche). La règle correcte est :

> `session_killed:false` — rien n'a été touché.
> `session_killed:true` — la session est morte et **rien ne l'a remplacée** : le nœud a besoin d'un
> autre levier, pas d'un retry de celui-ci.

On discrimine dans le **corps**, jamais en tordant un statut (ADR-0035 §3) : on ajoute un champ.

### §5 — Portée : les surfaces de spawn par nœud

La règle vaut pour `restart_node` (#489) et pour `node_retry` (#487), qui porte la même faute sur
une autre route. Écrite une fois pour les deux, sinon ce serait deux ADR pour la même règle. Elle
ne s'étend **pas** aux quatre commandes de boucle, qui gardent le vocabulaire d'ADR-0025.

### §6 — La réutilisation du sous-worktree

La coupe inconditionnelle est remplacée, aux deux sites de production, par une classification du
sous-worktree en **quatre** états — trois ne suffisent pas :

| État | Ce que c'est | Ce qu'on fait |
|---|---|---|
| `Absent` | ni répertoire (ou vide), ni ref de branche, ni enregistrement | on coupe |
| `Reusable` | enregistré sur la bonne branche, non prunable | **on réutilise en place, aucun appel git mutant** |
| `Recyclable` | prunable, HEAD détaché, ou branche orpheline sans worktree | on reape, puis on coupe |
| `Occupied` | branche checkoutée dans un autre worktree vivant, ou répertoire non-worktree non vide | on refuse et on **nomme** ce qui le tient |

Le quatrième état est la décision. Un découpage à trois (« bloqué, donc reap ») rend `restart_node`
destructeur : un verrou git périmé sur un arbre **sale** n'est pas « déjà inutilisable », c'est
précisément le travail que #489 existe pour sauver. Il faut séparer *recyclable* (rien à perdre)
d'*occupé* (tenu par un tiers).

Le prédicat de réutilisation est « worktree **enregistré** sur la bonne branche », jamais « la
branche pointe sur la base attendue » : le second est satisfait par un agent coincé qui n'a rien
commité mais a un arbre sale, et le « fix » le détruirait.

La sonde autoritaire est l'inventaire des worktrees depuis la racine du repo. Mesuré : demander sa
branche à un simple répertoire *à l'intérieur* du repo remonte au worktree principal et répond
`main` — une sonde par répertoire mentirait en silence. Le match se fait sur le **chemin absolu**,
jamais sur le basename : l'enregistrement git est nommé par le basename, donc tous les nœuds
collisionnent sur `iter-1` et git désambiguïse par des suffixes.

**Le nettoyage d'avortement n'est armé que sur les branches qui ont créé quelque chose.** Sur une
réutilisation, n'importe quelle panique du spawn atteindrait sinon le reap d'orphelin — qui réussit
sur un arbre sale. Gaté, le chemin d'avortement appende `RunFailed` **sans rien détruire** : le Run
part terminal avec le travail intact, `resume_run` le ré-ouvre, et la classification suivante rend
`Reusable`. Le chemin devient idempotent et auto-réparant, là où le résidu condamnait le nœud à
vie. L'invariant #279 (« un spawn avorté ne laisse pas d'orphelin ») n'est pas affaibli : il ne
portait que sur ce que le spawn a **créé**.

**Le `base_sha` se reporte** (c'est l'amendement à ADR-0036, et le point le plus subtil du lot).
Une réutilisation ne coupe rien, donc elle rend le `base_sha` du `NodeStarted` précédent de la
**même** itération. Les deux autres réponses évidentes sont pires que le bug :

1. relire le HEAD du worktree réutilisé rend le commit **du nœud**, jamais le tip pipeline →
   l'échappatoire d'adoption d'ADR-0036 serait **silencieusement désactivée pour tout nœud
   redémarré, à vie** ;
2. le tip de la branche pipeline **au moment de la réutilisation** **arme l'adoption à faux** : le
   garde passe, et l'adoption peut écraser le travail d'un nœud voisin mergé depuis la coupe
   d'origine.

La classification ne peut donc pas calculer la base elle-même : il faut la lui passer.

### §7 — Le slot qu'un spawn reprend ne se compte pas contre lui

Le comptage d'admission exclut le slot `(run_id, node_id, iter)` que le spawn courant est en train
de reprendre — c'est ce qui ferme le gel sous saturation du contexte. La clé est le **triplet
complet** : le comptage est global à tous les Runs alors que les ids de nœuds sont locaux au
pipeline, donc deux Runs concurrents du même pipeline ont tous deux un nœud homonyme à la même
itération — une clé aveugle au Run écarterait la session vivante de l'**autre** Run et
**dépasserait le cap**, exactement l'effondrement que l'admission existe pour empêcher.
L'exclusion ne retranche que si ce triplet détient réellement une session (le garde de transition
n'autorise le re-spawn d'une itération vivante que parce que ce spawn la remplace), et elle est
calculée **sous le verrou d'admission** — la recalculer sur une projection pré-verrou rouvrirait la
course check-and-reserve que le verrou ferme.

Et `kill_node` réveille désormais les nœuds `Waiting` : c'est le geste le plus probable pour
libérer le slot qu'un restart throttlé attend, et le retry des nœuds en attente n'a aucun timer.

## Alternatives écartées

- **Réutiliser le type de refus de complétion** au lieu de cloner son patron. Il est adossé au
  chemin de complétion (sa notion de récupérabilité est définie sur des variantes de complétion),
  et y ajouter une variante `2xx` falsifierait son invariant.
- **Trois slugs discriminants pour le refus du garde de transition.** Mesuré : le refus du garde
  n'a **aucun** discriminant interne, et le distinguer coûterait un refactor de la taille de la
  tranche, introduit en fraude dans une tranche de véracité. #490 a déjà tranché ce cas exact sur
  cette forme exacte (un slug, la prose dans `message`) ; diverger donnerait deux discriminations
  filaires différentes au **même** garde. Bonus : ça élimine un slug qui aurait été **faux** (il
  encodait un fait que le garde ne teste pas). Le discriminant reste une issue à part, co-possédée
  avec #487.
- **`202 Accepted` pour le throttle.** Défendable, mais ce serait le premier `202` du dépôt pour un
  bras de commande, et ADR-0025 a déjà légiféré ce cas en `200` : la discrimination appartient au
  corps.
- **Un mapper unique spawn → réponse** partagé par toute la surface de commandes. L'addendum #236
  d'ADR-0009 le prouve *lossy*.
- **Garder le `200` en enrichissant le corps** — ce que #489 demandait à l'origine, et ce
  qu'ADR-0035 supposait. Écarté : le statut *est* le mensonge, cf. l'errata posé sur 0035.
- **Rafraîchir la base d'un sous-worktree réutilisé.** Mesuré dangereux : merger la base dans un
  sous-worktree sale échoue, et commiter d'abord livre à un agent frais un arbre truffé de
  marqueurs de conflit. On **dit** `base_moved` et on laisse l'opérateur choisir `node_retry`, qui
  *est* l'outil « base fraîche ».

## Limites acceptées

- **Un verrou git périmé est signalé, pas supprimé.** Il remonte dans le corps (`stale_git_lock`)
  et en warn, et le worktree reste réutilisable. Refuser supprimerait le dernier levier de
  récupération sur un état que le restart peut améliorer (un agent frais peut retirer le verrou
  lui-même) ; le supprimer nous-mêmes est l'opération contre laquelle git met en garde, et PDO ne
  peut pas prouver que l'écrivain est mort — #485 est le précédent qui coûte cher.
- **La base n'est pas rafraîchie**, cf. ci-dessus. `base_moved` est calculé et remonté ; rien de
  plus.
- **La vérité filaire livrée ici n'est observable par aucun humain via l'UI** : le client jette le
  corps et l'erreur, et le seul bouton câblé vit dans une bannière que plus rien ne produit depuis
  #469. Trou connu, propriété de **#492**, énoncé pour qu'un prochain grilling ne le fiche pas en
  régression. Zéro travail frontend dans ce lot.
- **`Spawned` est rendu même quand le spawn tmux lui-même a échoué** — l'erreur est loguée et le
  flux continue. Le `200 {spawned}` de cette ADR est donc lui-même non véridique dans ce cas. Bug
  distinct, à ficher, cité ici pour ne pas créditer la décision d'un cas qu'elle ne couvre pas.
- **`restart_node` n'invalide jamais les artefacts** : les sorties partielles de la tentative
  avortée survivent au même `iter`, et une complétion ultérieure peut valider l'**ancien** output.
  Vrai avant comme après.
- **Le kill reste nu**, pas un reap (#488) : mécaniquement le reap serait correct, mais le snapshot
  de pane ne serait jamais servi (il n'est servi que sur une itération terminale, et un restart
  laisse le nœud non terminal). Trou nommé par CONTEXT.md ; cette ADR ne le ferme pas.
- **Les corps d'erreur en texte brut** (Run absent, pipeline illisible) restent tels quels : les
  normaliser est le périmètre de **#491**.
- **Le retry des nœuds en attente n'a toujours aucun timer**, et deux autres libérateurs de slot ne
  le réveillent pas (halt/pause, boot recovery). #489 ferme `kill_node` seul ; le reste est fiché.
- **Le comptage d'admission reste par nœud, pas par itération** (#453) : pré-existant ; l'exclusion
  ne se construit pas sur une hypothèse contraire.

## Relations

- Issue **#489** (cette décision).
- **ADR-0025** (#327) — amendée deux fois : §2 s'étend au kill, §3 est corrigé sur `noop` pour les
  commandes de spawn par nœud.
- **ADR-0035** (#490) — la forme du corps (`error` = slug, `recoverable`, prose dans `message`) est
  reprise telle quelle ; ses « Alternatives écartées » portent un errata, parce qu'elles
  décrivaient #489 comme séparé et additif.
- **ADR-0036** (#503) — amendée : le `base_sha` d'une itération réutilisée est **reporté**, pas
  re-dérivé.
- **ADR-0032** (#469) — indemne : la veille de vivacité lit des événements, jamais un statut HTTP.
  Elle cesse simplement d'inventer un `session_died` là où une création de worktree avait échoué.
- **ADR-0004** — critères fermés en couche 1 (projection totale, match sans joker) et en couche 3.
- **#487** (`node_retry`) — §5 est écrit pour lui aussi ; il consomme le patron de projection posé
  ici.
- **#498** — sa slice A doit consommer la classification posée ici via le bras `Recyclable`, pas la
  réimplémenter. Son levier 3 (un spawn échoué du scheduler doit appender un événement) reste chez
  elle : le chemin du scheduler n'a aucune réponse HTTP, l'événement y est le seul canal.
- **#491** vient après (corps texte brut vs JSON). **#492** possède le trou côté UI.
