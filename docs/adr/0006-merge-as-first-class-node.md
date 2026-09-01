# Le Merge devient un nœud first-class, plus auto-spawné

**Le runtime ne spawne jamais un résolveur de merge de lui-même : le `Merge` est un nœud du DAG que le designer place explicitement.** L'auto-spawn (et son toggle `auto_merge_resolver`) introduisait de l'orchestration ambiante — le designer ne *voyait* pas la barrière de synchro dans son graphe, ce qui contredit *Sharp tool* (ADR-0001) — et ne couvrait que le cas conflit, pas la barrière elle-même. Écarté aussi : *deux nœuds distincts `Merge` (code) et `Join` (doc-only)*, alors que « rassembler N branches » est un geste unique côté designer (le runtime choisit selon le type des upstream) ; et *un spawn LLM systématique pour la traçabilité*, dont le coût écrase le bénéfice — un summary trivial sans LLM suffit à l'audit.

Forme : 1 input port `branches: repeated`, 1 output `merged` (frontmatter `conflict_count`, `branches: [...]`, corps narratif). Sémantique : barrière edge-centrée (ci-dessous) ; fork d'un sous-worktree depuis la branche du Run ; `git merge` de chaque upstream ayant une branche dédiée ; **si conflit** → spawn d'un agent qui lit les artefacts du Blackboard pour reconstituer les intentions, résout, commit ; **sinon** → summary trivial + commit, sans LLM.

## La barrière est edge-centrée (amendée avec ADR-0011)

Une barrière **node-centrée** (« tous mes producteurs sont `Completed` ») **stalle silencieusement** dès qu'ADR-0011 route une branche ailleurs : son producteur ne devient jamais `Completed` et le Merge attend éternellement. La barrière est donc un cas particulier d'une règle de convergence générale, pas un traitement spécial du `merge` :

- Une edge entrante est **résolue** si elle a **firé** (producteur `Completed` + garde satisfaite, ou edge inconditionnelle) ou est **morte** (`when:` faux, `else` battue par une sœur, ou producteur lui-même mort).
- Un nœud est **mort** s'il a des edges entrantes et qu'elles sont **toutes** mortes. La mort se **propage** vers l'aval, `End` inclus.
- **Prêt** = toutes les entrantes résolues **et au moins une firée** ; le nœud spawne en consommant **uniquement les branches firées**.
- Un `Merge` à zéro branche firée est mort et **sauté**, comme tout nœud tout-mort — tant que `End` reste atteignable.
- **Jamais de stall silencieux** : si la cascade rend `End` inatteignable, le Run **halt explicitement** (`run_halted`, « unrouted »), diagnosticable et routable par le Pipeline Manager. Laisser le Run figé en comptant sur un timeout a été écarté pour cette raison.
- **`End` est une convergence, pas un premier-arrivé (#394)** : le Run ne passe `completed` que lorsque **toutes** les entrantes de `End` sont résolues. Sur un fan-out plat (`start→A→end`, `start→B→end`), la **dernière** branche vivante complète ; les arrivées antérieures sont des no-op. Avant #394, la branche rapide abandonnait sa sœur `running` pour toujours. Le `Halt`-sur-`End` (edge avec `reason:`) reste, lui, un premier-arrivé : c'est une sortie *bloquée*, pas une complétion.

Corollaire : `branches: repeated` accumule les branches **réellement firées** (compte dynamique, pas la liste statique des upstream déclarés). Une branche morte ne produit ni artefact ni branche git, donc elle est naturellement hors du `git merge` et du frontmatter.

**Périmètre.** Cette barrière couvre la convergence **hors boucle**. La convergence **par-lap** dans une boucle est **différée** (#148/#151) ; l'état de résolution est porté par-Run, compatible avec une future clé par-itération.

**Migration.** Les pipelines qui s'appuyaient sur l'auto-spawn cassent : il faut insérer un Merge downstream de chaque fan-out `code-mutating`, sinon plus rien ne résout les conflits. Diagnostic info-only dans l'éditeur si un tel fan-out n'a pas de Merge (ADR-0001 autorise le lint info-only).
