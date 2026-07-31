# ADR-0036 — Le merge-back se résout en faveur du nœud quand la divergence est l'histoire du Run réécrite par lui

> Statut : accepted (issue #503, reproduite le 2026-07-31). Vocabulaire : CONTEXT.md § « Merge-back
> d'un sous-worktree ». **Ne touche pas à ADR-0006** : le résolveur automatique de conflit reste
> retiré, et cette décision ne *résout* rien — elle refuse d'inventer un merge là où il n'y a rien à
> merger. **Ne touche pas à ADR-0035** : le refus de complétion reste un `409` nommé ; on réduit le
> nombre de cas qui doivent en produire un. **Amende ADR-0012(a)** sur un point : le runtime ne
> supprime toujours aucune branche ni aucun worktree, mais il peut désormais **déplacer** la ref de la
> branche pipeline — sous un commit de merge qui garde l'ancien tip en premier parent, donc sans jamais
> rendre un commit inatteignable.
>
> **Amendé par ADR-0037 (#489) sur le `base_sha`.** Depuis #489, `restart_node` **réutilise** le
> sous-worktree d'une itération au lieu de le recouper. Une réutilisation ne coupe rien : elle
> **reporte** le `base_sha` du `NodeStarted` précédent de la même itération, telle quelle. Les deux
> autres réponses sont pires que le bug — relire `HEAD` dans le worktree réutilisé rend le commit *du
> nœud* et désactive donc silencieusement l'échappatoire ci-dessous pour tout nœud redémarré ; prendre
> le tip pipeline au moment de la réutilisation l'**arme à faux** et peut écraser le travail d'un nœud
> voisin mergé depuis la coupe d'origine. La règle d'adoption elle-même est inchangée.

## Contexte

Le sous-worktree d'un nœud est coupé depuis la **branche pipeline** (`create_sub_worktree`), donc le
merge-back de `commit_and_merge_sub_worktree_inner` repose sur un invariant implicite : *le tip de la
branche pipeline reste un ancêtre de la branche du nœud*. Tant qu'il tient, `git merge` est un
fast-forward et ne peut pas échouer.

PDO livre lui-même les deux moitiés d'une configuration qui le contredit. Le nœud `Ship It` de la
bibliothèque a pour prompt de publier contre la branche d'intégration (`~/.pdo/library/ship-it.yaml` :
*« create a pr with auto merge to the source branch »*). Quand cette branche a bougé pendant le Run —
ce qui arrive dès qu'un Run concurrent livre — l'agent se rebase pour produire une PR propre. Le
rebase **ré-écrit** les commits du Run : les deux branches portent alors chacune sa propre copie du
même travail, construites indépendamment.

Occurrence du 2026-07-30, Run `20260730-150012-9a79d52` (#490) :

```
branche du nœud   7420eee  fix(#490)! … — 1.6.0
                  f9f971e  feat(#427) … — 1.5.0 (#500)   ← atterri sur main pendant le Run
                  f6f4630  refactor(#236) … — 1.4.1      ← base du Run

branche pipeline  2563fa0  fix(#490)! … — 1.5.0          ← le MÊME correctif, sa propre copie
                  f6f4630

merge-base(pipeline, nœud) = f6f4630 → 20 fichiers en conflit
```

Le merge-back a donc échoué, le Run est passé `failed` — alors que `git diff 7420eee 4ee5f18` (le
squash de sa PR) est **vide** : la livraison avait réussi. PDO tenait toutes les preuves du succès au
moment où il déclarait l'échec : le port de sortie du nœud était rempli et validé (`Verdict: Pass`),
le merge-back tournant strictement après. Le verdict du Run n'était pas dérivé de l'issue du travail,
mais de la comptabilité interne de PDO.

Deux défauts collatéraux, mesurés sur la même occurrence :

- `merge_conflict_detected.payload.detail` était **vide sur 100 % des conflits depuis toujours** :
  `worktree_ops.rs` lisait `output.stderr`, alors que `git merge` écrit tout son rapport de conflit sur
  **stdout** et laisse stderr vide. Un événement pour 20 fichiers en conflit disait `detail: ""`.
- Le nœud restait projeté `running`, sa session tmux vivante ~24 h après le `run_failed` avec un `claude`
  vivant dedans, occupant un slot sur 20, l'UI proposant `Stop` / `Retry` / `Mark complete` dessus. Pas
  « à vie » au sens strict — `boot_recovery` réconcilie cette forme au **redémarrage suivant** du daemon
  — mais à vie du *processus*, et le nœud était invisible aux deux filets qui devraient l'attraper
  (`run_stale_detection` saute tout Run non `Running` ; le balayage TTL clé sur `completed_at`, `None`
  pour un nœud `Running`).

Fréquence : **1 occurrence en 445 Runs terminaux** (1 `merge_conflict_detected` pour 406
`run_completed` + 39 `run_failed`). C'est un défaut de **confiance**, pas une panne : le coût réel est
que les 38 autres `run_failed` deviennent suspects, et qu'aucune surface ne disait pourquoi.

## Ce qu'on décide

### 1. Un conflit de merge-back se résout en faveur du nœud quand la divergence est celle du Run

Règle unique, **structurelle**, lisible dans le code sous `worktree_ops::ADOPTION_RULE` :

> **Le tip de la branche pipeline est encore le commit depuis lequel le sous-worktree de ce nœud a été
> coupé.**

La branche pipeline est créée depuis la base du Run et ne reçoit **que** le travail des nœuds de ce
Run. Donc, si son tip n'a pas bougé depuis le spawn du nœud, tout commit que le tip possède et que la
branche du nœud n'a pas est un commit dont le nœud **est parti** et qu'il a réécrit. Adopter son arbre
supersède l'histoire du Run par elle-même, et rien d'autre.

Aucun événement ne portait de SHA avant cette ADR. `create_sub_worktree` **retourne** désormais la base
depuis laquelle il a coupé, et les **deux** chemins de spawn l'écrivent dans le payload `NodeStarted`
sous `base_sha` : `node_spawn` (le chemin runtime) **et** `node_primitives` (`start_node`,
`restart_node`), sans quoi une itération relancée à la main naîtrait sans base — donc sans recours.
`merge_action::spawn_base_sha` la relit, ancrée sur le **dernier** `NodeStarted` de l'itération, parce
que `restart_node` et `invalidate_nodes` re-coupent le sous-worktree ; `worktree_ops` la compare au tip
réel. La lecture est pure (event log), la comparaison est dans la couche d'effet.

**Base inconnue ⇒ pas de résolution.** Un Run enregistré par un daemon pré-#503 n'a pas de `base_sha`,
et une base inconnue n'est pas un permis de réécrire une branche.

### 2. Le mécanisme est un commit de merge, jamais un `reset --hard <branche du nœud>`

L'issue recommandait `git reset --hard <sub_branch>`. Refusé : cela rendrait les commits supersédés
**inatteignables depuis la branche** — PDO supprimerait de l'histoire pour réparer sa propre
comptabilité — et **propagerait #503** : un nœud frère coupé du même tip verrait son propre merge-back
partir en conflit, et le `--is-ancestor` d'une sous-branche antérieure rendrait « NOT MERGED », cassant
l'invariant #213 AC3 (`boot_recovery`). À la place :

```
git commit-tree <arbre du nœud> -p <ancien tip pipeline> -p <tip du nœud> -m …
git reset --hard <ce commit>          # dans le worktree pipeline
```

Deux parents, l'ancien tip pipeline en **premier**, et un arbre qui *est* celui du nœud. Le commit dit
exactement ce qui s'est passé, `git diff C^1 C` montre l'écart, `git log` sur la branche pipeline
atteint toujours les deux côtés, et rien n'est détruit — seulement mis en minorité sur l'arbre. Le
nouveau tip est aussi un ancêtre pour le merge-back suivant.

Le déplacement de ref doit être un `reset --hard` **dans le worktree pipeline** : `git branch -f`
échoue (`cannot force update the branch … used by worktree at …`) et `git update-ref` sort en `0` mais
laisse l'index du worktree désynchronisé, ce qui ferait échouer le prochain nœud `doc-only` en
`doc_violated_code_immutability`.

### 3. Aucun garde de **contenu** ne marche — les trois sont mesurés faux

C'est la partie utile de cette ADR. Trois formulations ont été instruites sur les vraies branches de
l'occurrence, et **les trois refusent** le cas qu'elles devaient sauver :

| Prédicat | Verdict sur #503 | Cause |
|---|---|---|
| Égalité de blobs par chemin | **REFUSE** | le bump `1.5.0 → 1.6.0` (`Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `CONTEXT.md`) |
| Containment de **chemins** | **REFUSE** | l'agent a **renuméroté** son ADR `0034 → 0035` pour éviter le numéro que #427 venait de prendre. 33 fichiers sur 34 présents ; le manquant, `docs/adr/0034-completion-refusal-is-never-2xx.md`, existe **encore** sur la branche pipeline. **Un agent qui rebase renomme aussi.** |
| **Tree-sémantique** (« merger le tip pipeline dans le tip du nœud ne change pas son arbre ») | **REFUSE**, et est même **inatteignable** | `git merge-tree --write-tree 2563fa0 7420eee` → exit 1, 20 chemins en conflit. Un conflit est **symétrique** : si `git merge <nœud>` a conflicté dans le worktree pipeline, `merge-tree` conflicte aussi. Le test ne peut jamais passer sur ce chemin. |

Les 20 chemins ne sont pas du bookkeeping de version : `lib.rs`, `event_log.rs`,
`completion_refusal.rs`, `NodeDetailPanel.tsx` conflictent parce que les deux branches ont construit
**le même correctif indépendamment**. Aucune heuristique de contenu ne distingue « même travail,
ré-écrit » de « travail différent ». D'où le garde structurel du §1.

Restent deux préconditions sur le **mécanisme**, pas sur l'argument :

- **Worktree pipeline sale sur des fichiers suivis** ⇒ refus. `git merge` échoue bruyamment là où
  `reset --hard` détruit en silence. Il peut légitimement être sale : un nœud `doc-only`/`script` en
  vol, les restes d'un `doc_violated_code_immutability` (jamais annulés), le shell de Run, l'agent
  `__manager__` résident.
- **Histoires sans ancêtre commun** ⇒ refus. `git merge` échoue aussi là, et adopter remplacerait tout
  l'arbre du Run.

### 4. Un vrai conflit reste un `run_failed`, et la résolution n'est jamais silencieuse

Base périmée ⇒ conflit. Deux nœuds frères qui écrivent la même ligne incompatiblement restent un
échec : le merge du premier a **déplacé le tip**, donc la base du second ne correspond plus, et son
arbre ne contient pas le travail du premier — le prendre le perdrait (cf. #394 sur les branches
parallèles). Tests de non-régression explicites, `genuine_semantic_conflict_still_fails` et
`a_rebased_node_whose_base_went_stale_still_conflicts` : ce second cas est la même topologie que #503,
plus un `doc-only` qui a committé après le spawn — et il reste un échec.

Quand l'adoption a lieu, un événement `merge_resolved_in_node_favour` est appendé — informationnel dans
la projection, avec la règle appliquée, les deux tips, le commit de résolution et la liste des fichiers
qui auraient conflicté. PDO a réécrit une branche : cela se lit.

### 5. Un conflit dit *quoi*, et le nœud qui l'a subi est mort

- `MergeResult::Conflict` porte désormais une structure : le rapport de git **stdout inclus**, les deux
  SHA complets, et les chemins non mergés — lus avant que `merge --abort` ne nettoie l'index. Ces
  quatre champs vont dans le payload de `merge_conflict_detected`.
- Le chemin de conflit appende `NodeFailed` (il ne le faisait pas) **et** reape la session tmux du nœud,
  via la queue détachée d'ADR-0023 : le refus doit atteindre la CLI (`409`, codes de sortie d'ADR-0035)
  avant que la session dans laquelle `pdo complete` tourne ne soit tuée. Même traitement pour la
  violation d'immuabilité d'un nœud `doc-only`, qui fuyait sa session de la même façon.
  Trois effets de bord voulus de ce `NodeFailed` : il pose `completed_at`, donc il **arme le filet TTL**
  du reaper (qui clé dessus et ne voyait donc jamais ce nœud, comme `run_stale_detection` qui `continue`
  sur tout Run non `Running`) ; il rend le nœud terminal, donc `GET …/pane` cesse de le **ressusciter**
  avec `claude --continue` à chaque clic dans l'UI ; et son payload ne porte **que** `reason` — pas de
  clé `detail`, que la projection lirait comme preuve de validation d'outputs.
- Le chemin d'**erreur** (`MergeFailed`, un `git commit` cassé par exemple) ne reape pas : il laisse le
  Run vivant et l'agent libre de recommencer, donc sa session a encore quelque chose à faire.

### 6. Un Run non vert dit pourquoi

`RunState.failure_reason` projette le `reason` de `run_failed` / `run_skipped` (et le `message` de
`run_halted`), champ que ces événements portaient depuis toujours sans qu'aucun consommateur ne le
lise. Effacé par `RunResumed`, comme `NodeState::failure_reason` l'est par `NodeStarted`. Exposé sur
`GET /runs/:id` **et** sur l'entrée de liste, de sorte que le point rouge — qui constituait la
totalité du signal d'échec — porte enfin un titre.

## Non-objectifs

- Ne pas ressusciter le résolveur automatique retiré par ADR-0006. La suppression du sous-système
  vestigial (`MergeResolverStarted/Failed/Completed`, `RunState.merge_resolver`, `MergeResolverInfo`,
  `prompts/builtin/merge-resolver.md`, route `__merge_resolver__`, et le bras
  `MergeResult::ConflictPendingResolution` que seul `keep_conflict == true` construit) reste une autre
  issue, déjà appelée par ADR-0035 §6.
- Ne pas apprendre à `Ship It` à merger plutôt que rebaser. Ce serait une mitigation bienvenue, pas le
  correctif : son prompt est **éditable par l'utilisateur**, il ne dit jamais « rebase » (le rebase est
  émergent — le mot n'apparaît nulle part dans le dépôt), et le fichier est **untracked** (`.gitignore`
  ne whiteliste que trois `.pdo/pipelines/*.yaml`), donc aucun changement de dépôt ne peut l'atteindre.
  Le seul levier prompt-side non contournable serait le préambule runtime (`prompt_augmenter`), et un
  invariant du daemon ne peut pas être porté par un prompt.

## Conséquences

- Un Run dont le nœud terminal s'est rebasé sur une branche d'intégration qui a bougé complète au lieu
  d'être classé `failed` avec son travail déjà livré.
- La branche pipeline d'un tel Run n'est plus une histoire strictement croissante : elle peut porter un
  commit de merge dont l'arbre vient du nœud. Le nœud aval suivant, coupé de ce tip, voit ce qui a été
  livré — ce qui est la propriété qu'on veut.
- Un nœud peut, sur un Run à branche unique, faire superséder l'histoire du Run par la sienne (par
  exemple en revenant délibérément en arrière). C'est cohérent avec le modèle de PDO — le sous-worktree
  du nœud est partout ailleurs la source de vérité de son résultat — et c'est réversible : l'ancien tip
  est le premier parent.
- Le sharp tool d'ADR-0001 tient : PDO ne devine pas, il applique une règle énonçable en une phrase et
  écrit dans le log laquelle il a appliquée.
