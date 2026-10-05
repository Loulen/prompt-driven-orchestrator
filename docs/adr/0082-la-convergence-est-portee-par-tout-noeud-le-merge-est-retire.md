# La convergence est portée par tout nœud ; le `Merge` est retiré

> Statut : accepted (grilling du 2026-09-29, story Notion PDO Dojo n° 2 « [cleanup] Noeud merge
> désuet »). **Supersede ADR-0006.** Amende ADR-0011 (plus de `Merge` comme cible de convergence)
> et ADR-0060 (plus de type à isolation imposée). Vocabulaire : CONTEXT.md « Convergence ».
> Porté de PDO core (ADR-0079 de core, `Loulen/prompt-driven-orchestrator` #941) lors du
> rattrapage core 1.110 → 1.115 ; renuméroté 0082, l'ADR-0079 de PDO-v2 étant le monolithe modulaire.

## Contexte

ADR-0006 avait fait du `Merge` un nœud first-class : il posait la barrière de synchro, lançait un
`git merge` de chaque branche amont, écrivait un résumé sans LLM et ne spawnait un résolveur qu'en
cas de conflit. Trois décisions ultérieures l'ont vidé de son contenu :

- **Merge-back par nœud** (ADR-0036, ADR-0060) : chaque nœud isolé fusionne lui-même son
  sous-worktree dans la branche du Run quand il se termine. Un `Merge` coupé depuis cette branche
  a donc déjà tout le travail amont, et son `git merge` n'a plus rien à faire.
- **Barrière edge-centred généralisée** (ADR-0011, #394) : la règle de convergence s'applique déjà à
  tout nœud, `End` compris.
- **Inputs émergents** : les arêtes de même nom se regroupent dans une seule entrée-liste, donc un
  `agent` ordinaire peut recevoir N branches.

Constat du 2026-09-29 : le chemin « `git merge` puis résumé trivial » et le résolveur automatique
ne sont plus appelés en production. Au runtime, un `Merge` était déjà un `agent` isolé avec une
forme de ports figée (`branches` / `merged`) et sans réglage de prompt. Aucune pipeline connue,
ni du dépôt ni de l'instance, n'en contient.

## Décision

1. **Le type `merge` est retiré** du modèle, de l'éditeur et du runtime, avec le sous-système
   résolveur resté en place. Un nœud qui a plusieurs arêtes entrantes suffit à exprimer une
   convergence. L'utilisateur la dessine toujours explicitement (ADR-0001 : *sharp tool*), mais
   sur un nœud qui porte son propre prompt.
2. **La règle de convergence est celle de tout nœud** (reprise d'ADR-0006, désormais générale) :
   - Une arête entrante est **résolue** quand elle a **firé** ou qu'elle est **morte** (`when:`
     faux, `else` battue par une arête sœur, ou producteur lui-même mort).
   - Un nœud est **prêt** quand toutes ses arêtes entrantes sont résolues et qu'au moins une a
     firé. Il ne consomme que les branches firées.
   - Un nœud dont **toutes** les arêtes entrantes sont mortes est lui-même mort et sauté. La mort
     se propage vers l'aval.
   - **Jamais de stall silencieux** : si la cascade rend `End` inatteignable, le Run fait un halt
     explicite (« unrouted »).
   - `End` est une convergence : le Run est `completed` quand toutes les entrantes de `End` sont
     résolues.

   Le seul écart de comportement concerne un nœud de convergence dont toutes les branches sont
   mortes. L'ancien `Merge` était exclu de l'auto-skip ; un `agent` est sauté. Ce comportement
   correspond à ce que décrivait déjà le glossaire.
3. **Un `type: merge` encore présent dans un YAML** suit le chemin générique des types inconnus :
   il est converti en `agent`, avec le warning ordinaire. Il n'y a ni alias, ni migrateur, ni
   diagnostic dédié, comme pour `doc-only` et `code-mutating` (ADR-0060). Le comportement est
   conservé, puisqu'un `agent` est isolé par défaut et que ses ports restent libres.
4. **Les données historiques restent lisibles.** Les variantes d'événements du résolveur restent
   dans l'event log, sans émetteur, pour que les anciens Runs se rejouent. Un Run archivé dont la
   pipeline contient un `merge` se rouvre avec ce nœud affiché comme `agent`, ce qu'il exécutait
   réellement, et son coût reste calculé.

## Options écartées

- **Retirer seulement le bouton de la barre d'outils** : on garderait un type qu'on ne peut plus
  créer mais qu'on doit continuer à maintenir.
- **Refus au chargement avec une règle `pdo migrate`** (précédent ForEach, ADR-0011) : ce refus
  dur se justifiait parce que convertir un fan-out en nœud de travail aurait exécuté zéro fois le
  travail de chaque item. Ici, la conversion en `agent` préserve le comportement, donc un refus
  n'apporterait que de la friction.
- **Garder un alias silencieux `merge` vers `agent`** : ce serait une branche de compatibilité
  sous un autre nom, que ADR-0060 refuse déjà.
