# ADR-0038 — Le balayage d'orphelins inventorie tmux avant de lire le log, et aucune session n'existe avant sa réservation

> Statut : accepted (issue #485, reproduite 3/3 en pile isolée le 2026-07-31, 9 occurrences en
> production sur 8 jours). Vocabulaire : CONTEXT.md § « Balayage d'orphelins ». **Amende ADR-0032** :
> la mort de session reste le seul verdict terminal de liveness, mais l'exactitude « par
> construction » qu'elle revendique valait pour le *détecteur* et était fausse pour le *reaper*, qui
> **fabriquait** la mort que le détecteur observait fidèlement. **Amende ADR-0009** : la précondition
> posée ici est la première que `start_node` ne pouvait pas rater sans conséquence pour un autre
> sous-système — l'écart « legacy à résorber » devient porteur, avec un argument d'exactitude et non
> plus seulement d'hygiène. **Ne touche pas à ADR-0012(a)** : le balayage ne supprime toujours ni
> worktree ni branche, et ne change **rien** à *qui* est un orphelin.

## Contexte

Le reaper rend un verdict de **non-existence** : « aucune réservation dans l'event log ⟹ orphelin ⟹
`kill-session` ». Ce verdict n'est jamais vrai en soi. Il est vrai *relativement à deux
observations* — l'inventaire des sessions tmux vivantes, et la lecture du log — et le code les
prenait dans le mauvais ordre.

`run_orphan_sweep` construisait **d'abord** un instantané de **tous** les Runs (`load_all_run_ids`,
puis une `load_events` + `project` par Run), puis appelait un balayage qui n'énumérait les sessions
qu'à sa première ligne de corps. Une session née entre les deux était donc :

- **présente** dans l'inventaire — elle existait à l'instant de l'énumération ;
- **absente** de l'instantané — son `NodeStarted` n'était pas encore committé quand le Run a été lu.

Bras « absent », `kill`. Sans re-lecture, sans garde d'âge, sans seconde chance.

Occurrence du 2026-07-30, Run `20260730-020012-e5cfea0`, nœud `pqWxfLa1` :

```
02:13:10.621  Spawned tmux session: pdo-20260730-020012-e5cfea0-pqWxfLa1-iter-1
02:13:10.771  Orphan sweep: killing session for absent run 20260730-020012-e5cfea0/pqWxfLa1   ← +150 ms
02:13:39.985  Stale detector: node pqWxfLa1 — session died (tmux_server_alive=Some(true), correlated_deaths=0)
02:13:40.845  WARN Run 20260730-020012-e5cfea0 reconciled to Failed — run_stalled: blocked behind: pqWxfLa1
```

Deux Runs perdus cette nuit-là, sous un verdict qui accuse tmux, la RAM et l'API — tout sauf le
coupable. Et le seul témoin du kill était `journalctl` : ni l'UI, ni l'API, ni l'event log n'en
portaient trace.

Trois faits gouvernent la décision ci-dessous.

1. **La largeur de la fenêtre est le coût de l'instantané, donc elle croît avec l'usage.** 21 s
   mesurées le 2026-07-23 sur 437 Runs, à cadence de balayage de 60 s : ~35 % du temps. Un outil qui
   accumule des Runs par construction porte un bug dont la probabilité monte linéairement.
2. **Le balayage de boot ne pouvait pas voir le défaut** : il tourne avant la construction du
   routeur, donc aucun spawn ne lui est concurrent. Le doc-comment disait *« at daemon boot »* : la
   seule occurrence dangereuse était documentée comme inexistante. C'est ce qui a rendu le défaut
   invisible à l'inspection pendant toute la vie du reaper.
3. **Le chemin du scheduler était déjà conforme côté spawn** (`NodeStarted` appendé *avant* le
   `tmux new-session`), mais **les boutons Start et Retry ne l'étaient pas** : la primitive
   `start_node` spawnait la session puis rendait l'événement à son appelant, qui l'appendait ensuite
   — et un échec d'append n'était qu'un `error!`, la session survivant sans réservation.

## Ce qu'on décide

### 1. L'inventaire tmux est pris **avant** toute lecture du log, et c'est une preuve

Ordre imposé : `list_pdo_sessions` → `now` → lectures ciblées → décision → kills.

**Démonstration par contraposée.** Le log ne fait que croître. Une absence constatée *après*
l'inventaire implique une absence *à* l'inventaire. Donc une session absente du log lu à `T_lecture`
n'existait pas à `T_inventaire` — elle n'est pas dans l'inventaire, et le bras « absent » ne peut pas
la voir. Une session née après l'inventaire est simplement hors du jeu jugé ; elle sera examinée au
balayage suivant, quand sa réservation sera visible.

La monotonie ne travaille que dans ce sens. L'ordre inverse n'a **pas** de démonstration
symétrique : c'est pourquoi ce n'est pas un choix de lisibilité mais une propriété d'exactitude.

### 2. L'inventaire est une **donnée d'entrée**, pas quelque chose que le balayage va chercher

Un ordre correct qui repose sur la politesse du site d'appel n'est pas un invariant, c'est une
coïncidence. `sweep_orphans` appelait `list_pdo_sessions` depuis son propre corps : aucun appelant ne
*pouvait* garantir l'ordre. Elle est supprimée et remplacée par un couple :

- `decide_sweep(inputs, ttl, now) -> Vec<SweepDecision>` — **pur** (ADR-0009 couche 1) : zéro tmux,
  zéro DB, zéro horloge. Une décision par entrée, `Keep` compris, pour qu'un test puisse affirmer
  l'**absence** de kill aussi fort que sa présence.
- `apply_sweep(socket, decisions) -> SweepTally` — le bord : log de la raison, puis `kill-session`.

La clé de `SweepInput` est **la session**, pas le Run. C'est le point de forme qui compte : une carte
`run_id → RunState` est la structure qui *rappelle* le bug (pour la remplir, l'implémentation
naturelle est « tout projeter, puis lister tmux »), et une closure de lookup garde exactement la
propriété qui a causé #485 — le *moment* où la carte derrière elle a été construite n'est pas
exprimable dans le type. Avec une entrée par session, on ne peut pas remplir les faits sans déjà
tenir les noms.

### 3. Aucune session tmux n'existe avant que l'événement qui la réserve soit durablement enregistré

C'est la précondition dont dépend le §1, et le reaper **ne peut pas** la faire respecter : elle
appartient au spawn. Elle est donc rendue vraie sur tous les chemins, et `start_node` change de forme
pour que l'ordre inverse soit **inexprimable** : la primitive (synchrone et sans DB par contrat) rend
une *intention de spawn* que l'appelant exécute **après** l'append.

**Pourquoi le type, et pas la discipline des appelants.** Un invariant porté par deux appelants est un
invariant que le troisième cassera. Et l'ordre était déjà cassé sur deux des trois portes existantes.

**Pourquoi maintenant, et pas « plus tard, c'est théorique ».** La fenêtre `[spawn, append]` des
boutons Start/Retry était atteignable uniquement si la latence d'append dépassait la durée de
l'instantané — quelques millisecondes contre 21 secondes, donc jamais. Or l'inversion du §1 rend
naturel de réduire l'instantané aux Runs qui tiennent une session vivante, ce qui fait tomber cette
durée à quelques millisecondes : la fenêtre **se rouvre en silence**, sur des chemins que l'utilisateur
atteint en un clic. Livrer l'inversion seule, c'est livrer un correctif dont la justesse repose sur un
bug de performance connu qui ne doit jamais être corrigé. L'ordre de livraison — réservation d'abord,
inversion ensuite — fait partie de la décision.

**Le contre-échange, assumé.** Si l'append réussit et que le spawn échoue, on obtient un `NodeStarted`
sans session. C'est exactement ce que fait le chemin du scheduler depuis toujours, et depuis #469
(ADR-0032) la mort de session est un verdict **bruyant**. On échange un échec silencieux (session
orpheline, `200` menteur) contre un échec visible. C'est le bon sens du troc.

### 4. Ce que l'ordre ne prouve pas — et pourquoi c'est quand même bon

Trois cas restent hors de la preuve, consignés ici pour qu'on ne les relise pas comme des oublis.

- **Le tombstone de Run oublié (ADR-0024).** Un Run tombstoné voit son log effacé : sa session
  devient légitimement « absente » et se fait reaper. C'est voulu — l'oubli est durable.
- **La session ressuscitée par `GET …/pane`.** Le chemin de resume recrée une session pour un nœud
  dont l'itération est terminée ; le bras TTL la reprend. Ce n'est pas une réservation manquante,
  c'est une session dont la réservation est *ancienne*, ce que le TTL couvre déjà.
- **`__merge_resolver__` reste non conforme, et on le laisse.** Le parseur classe
  `pdo-<run>-__merge_resolver__-iter-1` en `NodeRun`, mais son événement atterrit dans un champ de
  projection distinct et **jamais** dans les nœuds : le lookup ne connaît que `__manager__` et
  `__shell__`, donc cette session serait tuée à chaque passe, **ordre ou pas**. Latent seulement parce
  que le chemin est mort — le seul appelant de production passe un `false` littéral. Greffer une
  réservation pour du code qui ne peut jamais spawner, c'est entretenir ce que deux ADR (0006, 0036)
  ont retiré. La suppression du sous-système est un follow-up ; ce n'est pas cette décision.

### 5. Le balayage tue **plus tard**, jamais **moins**

La contrepartie de l'inversion, et elle est bornée : un vrai orphelin né dans la fenêtre vit un tick
de plus. Cohérent avec la doctrine du produit (« le balayage suivant rattrape », #251), et sans
commune mesure avec le coût de l'inverse — une itération de nœud jetée avec ses effets de bord déjà
produits, ou un Run entier.

Corollaire non négociable : **le correctif s'accompagne de son jumeau en test**. La session jeune
survit **et** la session réellement absente est toujours tuée. Sans le second, on « corrige » en
neutralisant le ménage, et les sessions s'accumulent vers le point d'effondrement de tmux (~30).

### 6. Les huit messages de kill sont un contrat, et l'absence passe en `warn!`

Tout le diagnostic de #485 est venu d'un `journalctl | grep "Orphan sweep: killing session for absent
run"`. Les huit chaînes sont donc épinglées **byte-identiques** par un test unitaire, y compris leurs
deux irrégularités (les bras nœud ne disent pas le mot « node » ; le bras TTL s'appuie sur le nom de
session). Un formateur composé `{kind} × {cause}` les casserait en silence : huit bras plats.

Le niveau, lui, se dédouble. *Archivé* et *TTL* restent `info!` — ménage nominal, toutes les 60 s, sur
des sessions que personne ne regarde ; les passer en `warn!` fabriquerait un flux de warnings en
régime permanent, ce qui apprend à l'opérateur à les ignorer et enterre la seule ligne qui compte.
*Absent* et *nom non reconnu* passent en `warn!` : après ce correctif, c'est un « ne peut plus
arriver » sur une session vivante, et un `journalctl -p warning` doit suffire à le trouver.

Et parce que `journalctl` seul est le motif de panne récurrent de ce produit — ADR-0034 le dit en
citant #485 nommément —, `GET /sessions` porte désormais
`reaper: { last_sweep_at, killed, killed_for_absent_run }`. Deux compteurs et non un : un compteur ne
peut pas porter de raisons, et `killed_for_absent_run` est la classe qui doit rester **plate** en
régime normal.

Les deux sont **cumulés depuis le boot**, et c'est une décision, pas un détail. Un kill est un
*événement* ; le nombre de nœuds bloqués sur le menu de limite (`blocked_on_limit`, #290) est un
*niveau*. Un niveau se recalcule à chaque passe — c'est correct et sans état à purger. Un événement se
**compte** : remis à zéro à chaque passe, le compteur répond « le *dernier* balayage a-t-il tué ? »,
question dont la réponse est ~toujours `0`, puisque la passe qui tue est suivie en quelques secondes
d'une passe à vide qui écrit `0` par-dessus. L'opérateur qui demande « le reaper a-t-il tué mon
nœud ? » lisait donc `0` quelle que soit sa rapidité — la jauge reproduisait le défaut qu'elle devait
corriger. Le cumul est aussi ce qui rend « doit rester plate » **vérifiable** : plate signifie
*jamais incrémentée*, et non *dernière passe oisive*. Corollaire assumé : les compteurs ne sont pas
persistés (un redémarrage les remet à zéro) et `journalctl` garde le détail par session.

Ce cumul ne contredit pas le « recalculé de zéro à chaque passe » invoqué plus bas : cette propriété
porte sur l'**état de décision** du balayage (aucune mémoire par session entre deux passes, donc rien
à purger), pas sur deux scalaires d'observabilité. `SweepTally` reste strictement la tally de sa passe ;
c'est l'appelant qui accumule.

## Alternatives écartées

- **Un délai de grâce sur l'âge de la session** (« ne jamais tuer une session de moins de N
  secondes »). Paraît plus simple, et est **strictement plus faible**. Ça introduit l'horloge de tmux
  comme **seconde source de vérité** à côté de l'event log, qui est la source de vérité du runtime ;
  ça réintroduit un **seuil**, donc la classe exacte de faux positifs qu'ADR-0032 §1 a supprimée, avec
  la même issue à terme (une lecture lente sur une instance âgée dépasse n'importe quel N choisi
  aujourd'hui) ; ça retarde le reap d'un vrai orphelin jeune ; et ça ne ferait **rien** pour
  `__merge_resolver__`, tué à tout âge. **Un délai de grâce achète une probabilité là où l'ordre
  achète une preuve.**
- **Re-lire le log avant de tuer.** Une lecture SQLite sur le chemin du kill pour un gain nul en
  exactitude — c'est toujours un TOCTOU, la fenêtre est juste plus courte. Et le cas TTL la défait
  franchement : un nœud force-démarré à l'itération N+1 dont l'itération précédente a complété il y a
  deux heures obtient d'une relecture la même réponse *vraie* (« complété il y a 2 h »), qui
  **confirme** le kill.
- **Élargir le bras « absent »** en « absent *et* vieux », ou « absent *et* confirmé deux balayages de
  suite ». Mémoriser un état entre balayages contredirait « recalculé de zéro à chaque passe », la
  propriété qui fait qu'il n'y a aucun état à purger.
- **Attaquer le N+1 comme un problème de performance.** Il est la *largeur* de la fenêtre, pas sa
  cause : l'accélérer sans corriger l'ordre rend le bug plus rare et plus difficile à diagnostiquer.
  Ici il disparaît **par construction** (le domaine du lookup est `{ run_id | ∃ session vivante }`),
  soit un site sur neuf ; les huit autres ont réellement besoin de tous les Runs.
- **Avaler l'erreur DB en « absent ».** Un timeout de pool — exactement ce que montre le journal de
  l'issue — lirait un Run vivant comme absent et tuerait sa session. Le `?` est **fail-closed** :
  les deux sites de production ne font que `warn!`, donc un tick avorté ne reape **rien** et réessaie
  au tick suivant.

## Limites acceptées

- **Aucun test ne reproduit la course.** Il n'existe aucun hook entre l'inventaire et la lecture.
  L'ordre est gardé par **construction** (l'inventaire est un paramètre) plus le commentaire
  d'invariant ; les tests épinglent les conséquences observables des deux côtés, pas un rouge-puis-vert
  sur la course elle-même.
- **La jauge ne répond pas « qui a tué *mon* nœud ».** C'est une question scopée au Run ; deux
  compteurs à l'échelle de l'instance ne peuvent pas y répondre, même cumulés. La surface qui pourrait
  est l'event log (un événement informatif, motif de projection no-op déjà établi), mais sa valeur est
  conditionnée à une UI d'historique d'events qui n'existe pas — le client d'API correspondant n'a
  aucun appelant dans le frontend. Follow-up, pas périmètre.
- **Le lookup reste aveugle à `iter`.** C'est une mauvaise *clé de résolution*, pas un mauvais *ordre
  d'observation* ; le corriger changerait la politique de kill (faux positifs sur les régions
  `collection`, filet TTL désactivé pour les nœuds multi-itérations) et demande ses propres tests.
  Préservé tel quel, follow-up séparé.
- **#498 n'est pas fermée.** La session tuée est perdue, mais son sous-worktree et sa branche
  survivent, et la collision de branche qui s'ensuit condamne le nœud à vie. Ce correctif supprime le
  **producteur principal** de cette condition ; il en reste d'autres (#487, #489).

## Relations

- **ADR-0009** (primitives à trois couches) — amendée : `decide_sweep` est de la couche 1 pure, et la
  précondition de réservation devient un contrat de couche 2 opposable, plus un simple écart d'hygiène.
- **ADR-0032** (liveness) — amendée : le détecteur ne mentait pas, il rapportait fidèlement une mort
  que nous avions causée. Aucun seuil, aucun verdict de liveness n'est touché.
- **ADR-0034** (prix hors bande) — la raison invoquée pour mettre l'observabilité *dans* le périmètre :
  elle nomme `journalctl` seul comme le motif de panne récurrent de ce produit, en citant #485.
- **ADR-0015** (précédence `stored → env → default`) — inchangée : le TTL reste lu frais à chaque tick.
- **ADR-0012(a)** — inchangée : le balayage tue des sessions, jamais un worktree ni une branche.
- **ADR-0024** (tombstone) — cf. §4 : un Run oublié voit sa session reapée, et c'est voulu.
