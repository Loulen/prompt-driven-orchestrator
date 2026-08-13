# ADR-0036 — Le merge-back se résout en faveur du nœud quand la divergence est l'histoire du Run réécrite par lui

> Statut : accepted (issue #503, reproduite le 2026-07-31). Vocabulaire : CONTEXT.md § « Merge-back
> d'un sous-worktree ». **Ne touche pas à ADR-0006** : le résolveur automatique de conflit reste
> retiré, et cette décision ne *résout* rien — elle refuse d'inventer un merge là où il n'y a rien
> à merger. **Ne touche pas à ADR-0035** : le refus de complétion reste un `409` nommé ; on réduit
> le nombre de cas qui doivent en produire un. **Amende ADR-0012(a)** sur un point : le runtime ne
> supprime toujours aucune branche ni aucun worktree, mais il peut désormais **déplacer** la ref de
> la branche pipeline — sous un commit de merge qui garde l'ancien tip en premier parent, donc sans
> jamais rendre un commit inatteignable.
>
> **Amendé par ADR-0037 (#489)** : une réutilisation de sous-worktree ne coupe rien, donc elle
> **reporte** le `base_sha` d'origine de l'itération au lieu d'en dériver un nouveau.

## Contexte

Le sous-worktree d'un nœud est coupé depuis la **branche pipeline**, donc le merge-back repose sur
un invariant implicite : *le tip de la branche pipeline reste un ancêtre de la branche du nœud*.
Tant qu'il tient, le merge est un fast-forward et ne peut pas échouer.

PDO livre lui-même les deux moitiés d'une configuration qui le contredit. Le nœud `Ship It` de la
bibliothèque a pour prompt de publier contre la branche d'intégration. Quand cette branche a bougé
pendant le Run — ce qui arrive dès qu'un Run concurrent livre — l'agent se rebase pour produire une
PR propre. Le rebase **ré-écrit** les commits du Run : les deux branches portent alors chacune sa
propre copie du même travail, construites indépendamment.

Occurrence du 2026-07-30, Run `20260730-150012-9a79d52` (#490) :

```
branche du nœud   7420eee  fix(#490)! … — 1.6.0
                  f9f971e  feat(#427) … — 1.5.0 (#500)   ← atterri sur main pendant le Run
                  f6f4630  refactor(#236) … — 1.4.1      ← base du Run

branche pipeline  2563fa0  fix(#490)! … — 1.5.0          ← le MÊME correctif, sa propre copie
                  f6f4630

merge-base(pipeline, nœud) = f6f4630 → 20 fichiers en conflit
```

Le merge-back a donc échoué, le Run est passé `failed` — alors que le diff entre le tip du nœud et
le squash de sa PR est **vide** : la livraison avait réussi. PDO tenait toutes les preuves du succès
au moment où il déclarait l'échec (port de sortie rempli et validé, le merge-back tournant
strictement après). Le verdict du Run n'était pas dérivé de l'issue du travail, mais de la
comptabilité interne de PDO.

Deux défauts collatéraux sur la même occurrence : l'événement de conflit était vide depuis toujours
(le rapport de git était lu au mauvais endroit), et le nœud en conflit restait projeté `running`,
session vivante ~24 h après le `run_failed`, invisible des deux filets de liveness. Fréquence :
**1 occurrence en 445 Runs terminaux**. C'est un défaut de **confiance**, pas une panne : le coût
réel est que les 38 autres `run_failed` deviennent suspects, et qu'aucune surface ne disait
pourquoi.

## Ce qu'on décide

### 1. Un conflit de merge-back se résout en faveur du nœud quand la divergence est celle du Run

Règle unique, **structurelle**, lisible dans le code sous un nom dédié :

> **Le tip de la branche pipeline est encore le commit depuis lequel le sous-worktree de ce nœud a
> été coupé.**

La branche pipeline est créée depuis la base du Run et ne reçoit **que** le travail des nœuds de ce
Run. Donc, si son tip n'a pas bougé depuis le spawn du nœud, tout commit que le tip possède et que
la branche du nœud n'a pas est un commit dont le nœud **est parti** et qu'il a réécrit. Adopter son
arbre supersède l'histoire du Run par elle-même, et rien d'autre.

Aucun événement ne portait de SHA avant cette ADR. La coupe du sous-worktree **retourne** désormais
la base depuis laquelle elle a coupé, et les **deux** chemins de spawn (runtime et primitives
manuelles) l'écrivent dans le payload `NodeStarted` sous **`base_sha`** — sans quoi une itération
relancée à la main naîtrait sans base, donc sans recours. La comparaison relit la base ancrée sur
le **dernier** `NodeStarted` de l'itération, parce qu'une itération peut être re-coupée
(invalidation) ou **réutilisée** (`restart_node`, ADR-0037 — auquel cas le `base_sha` d'origine est
reporté tel quel). La lecture est pure (event log), la comparaison est dans la couche d'effet.

**Base inconnue ⇒ pas de résolution.** Un Run enregistré par un daemon pré-#503 n'a pas de
`base_sha`, et une base inconnue n'est pas un permis de réécrire une branche.

### 2. Le mécanisme est un commit de merge, jamais un `reset --hard <branche du nœud>`

L'issue recommandait `git reset --hard <sub_branch>`. Refusé : cela rendrait les commits supersédés
**inatteignables depuis la branche** — PDO supprimerait de l'histoire pour réparer sa propre
comptabilité — et **propagerait #503** : un nœud frère coupé du même tip verrait son propre
merge-back partir en conflit, et le contrôle d'ancestralité d'une sous-branche antérieure rendrait
« NOT MERGED », cassant l'invariant de la boot recovery (#213 AC3). À la place :

```
git commit-tree <arbre du nœud> -p <ancien tip pipeline> -p <tip du nœud> -m …
git reset --hard <ce commit>          # dans le worktree pipeline
```

Deux parents, l'ancien tip pipeline en **premier**, et un arbre qui *est* celui du nœud. Le commit
dit exactement ce qui s'est passé, le diff au premier parent montre l'écart, la branche pipeline
atteint toujours les deux côtés, et rien n'est détruit — seulement mis en minorité sur l'arbre. Le
nouveau tip est aussi un ancêtre pour le merge-back suivant.

Le déplacement de ref doit être un `reset --hard` **dans le worktree pipeline** : `git branch -f`
échoue sur une branche checkoutée par un worktree, et `git update-ref` sort en `0` mais laisse
l'index du worktree désynchronisé — ce qui ferait échouer le prochain nœud `doc-only` sur une
fausse violation d'immutabilité.

### 3. Aucun garde de **contenu** ne marche — les trois sont mesurés faux

C'est la partie utile de cette ADR. Trois formulations ont été instruites sur les vraies branches
de l'occurrence, et **les trois refusent** le cas qu'elles devaient sauver :

| Prédicat | Verdict sur #503 | Cause |
|---|---|---|
| Égalité de blobs par chemin | **REFUSE** | le bump `1.5.0 → 1.6.0` (`Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `CONTEXT.md`) |
| Containment de **chemins** | **REFUSE** | l'agent a **renuméroté** son ADR `0034 → 0035` pour éviter le numéro que #427 venait de prendre. 33 fichiers sur 34 présents ; le manquant existe **encore** sur la branche pipeline. **Un agent qui rebase renomme aussi.** |
| **Tree-sémantique** (« merger le tip pipeline dans le tip du nœud ne change pas son arbre ») | **REFUSE**, et est même **inatteignable** | `git merge-tree` sur les deux tips → exit 1, 20 chemins en conflit. Un conflit est **symétrique** : si le merge a conflicté dans le worktree pipeline, merge-tree conflicte aussi. Le test ne peut jamais passer sur ce chemin. |

Les 20 chemins ne sont pas du bookkeeping de version : le cœur du daemon et le frontend conflictent
parce que les deux branches ont construit **le même correctif indépendamment**. Aucune heuristique
de contenu ne distingue « même travail, ré-écrit » de « travail différent ». D'où le garde
structurel du §1.

Restent deux préconditions sur le **mécanisme**, pas sur l'argument :

- **Worktree pipeline sale sur des fichiers suivis** ⇒ refus. `git merge` échoue bruyamment là où
  `reset --hard` détruit en silence. Il peut légitimement être sale : un nœud `doc-only`/`script`
  en vol, les restes d'une violation d'immutabilité (jamais annulés), le shell de Run, le manager
  résident.
- **Histoires sans ancêtre commun** ⇒ refus. `git merge` échoue aussi là, et adopter remplacerait
  tout l'arbre du Run.

### 4. Un vrai conflit reste un `run_failed`, et la résolution n'est jamais silencieuse

Base périmée ⇒ conflit. Deux nœuds frères qui écrivent la même ligne incompatiblement restent un
échec : le merge du premier a **déplacé le tip**, donc la base du second ne correspond plus, et son
arbre ne contient pas le travail du premier — le prendre le perdrait (cf. #394 sur les branches
parallèles). Tests de non-régression explicites sur les deux topologies : le conflit sémantique
authentique, et la topologie #503 plus un commit postérieur au spawn — qui reste un échec.

Quand l'adoption a lieu, un événement `merge_resolved_in_node_favour` est appendé — informationnel
dans la projection, avec la règle appliquée, les deux tips, le commit de résolution et la liste des
fichiers qui auraient conflicté. PDO a réécrit une branche : cela se lit.

## Non-objectifs

- Ne pas ressusciter le résolveur automatique retiré par ADR-0006. La suppression du sous-système
  vestigial reste une autre issue, déjà appelée par ADR-0035.
- Ne pas apprendre à `Ship It` à merger plutôt que rebaser. Ce serait une mitigation bienvenue, pas
  le correctif : son prompt est **éditable par l'utilisateur**, il ne dit jamais « rebase » (le
  rebase est émergent), et le fichier n'est pas suivi par git — aucun changement de dépôt ne peut
  l'atteindre. Le seul levier prompt-side non contournable serait le préambule runtime, et un
  invariant du daemon ne peut pas être porté par un prompt.

## Conséquences

- Un Run dont le nœud terminal s'est rebasé sur une branche d'intégration qui a bougé complète au
  lieu d'être classé `failed` avec son travail déjà livré.
- La branche pipeline d'un tel Run n'est plus une histoire strictement croissante : elle peut
  porter un commit de merge dont l'arbre vient du nœud. Le nœud aval suivant, coupé de ce tip, voit
  ce qui a été livré — la propriété qu'on veut.
- Un nœud peut, sur un Run à branche unique, faire superséder l'histoire du Run par la sienne (par
  exemple en revenant délibérément en arrière). C'est cohérent avec le modèle de PDO — le
  sous-worktree du nœud est partout ailleurs la source de vérité de son résultat — et c'est
  réversible : l'ancien tip est le premier parent.
- Livrés avec la décision, en réparation des défauts collatéraux du contexte : l'événement de
  conflit porte désormais le rapport complet de git (il était vide sur 100 % des conflits depuis
  toujours — le code lisait stderr alors que `git merge` rapporte sur stdout), les deux SHA et les
  chemins non mergés ; le nœud en conflit est marqué failed et sa session reapée (il restait
  projeté `running`, invisible des filets de liveness, et l'UI le ressuscitait à chaque clic) ; le
  chemin d'**erreur** de merge, lui, ne reape pas — il laisse le Run vivant et l'agent libre de
  recommencer ; et le Run projette `failure_reason`, pour que le point rouge — qui constituait la
  totalité du signal d'échec — porte enfin un titre.
- Le sharp tool d'ADR-0001 tient : PDO ne devine pas, il applique une règle énonçable en une phrase
  et écrit dans le log laquelle il a appliquée.
