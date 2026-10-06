# Édition du graphe pendant qu'un Run tourne

> Amendé par ADR-0080 : le pilotage d'un Run est en lecture seule, l'édition à chaud est un geste explicite (« Éditer pour ce Run ») et n'écrit que le snapshot du Run par défaut.

**Un Run en cours reste éditable à chaud, sans modèle draft/published : en mode « Éditer pour ce Run » (ADR-0080), le graphe du Run se modifie pendant qu'il tourne et le scheduler se réajuste au tick suivant.** *Deliberate over autonomous* suppose exactement le geste de rerouter à chaud ce qu'un output vient de révéler, et l'audit est déjà tenu par l'event log SQLite append-only. C'est sciemment hors-marché : aucun outil mainstream ne fait du hot-rerouting d'un graphe en cours d'exécution.

**Portée de l'édition.** Pendant un Run, l'édition modifie le snapshot run-scope, et lui seul. L'écriture vers le Pipeline partagé est un geste distinct et averti (ADR-0080). **Étanchéité** : chaque run a son propre snapshot, donc aucune édition n'impacte un autre run en cours.

**Les seuls garde-fous sont des invariants de cohérence runtime, jamais de la validation prescriptive** (*Sharp tool*, ADR-0001). Ils existent parce qu'un rejet tardif serait un stall ou une session orpheline, pas parce que le design serait « mauvais » :

- (a) Un node à **session vive** (`running`, `awaiting_user`) est immuable — suppression comme **changement de type** : la session vive a été lancée sur l'ancienne définition alors que `pdo complete` rejoue le snapshot courant du run ; un swap mid-session désynchronise les deux. Sur un node non spawné ou terminé, tout est libre.
- (b) Le `max_iter` d'une boucle live est éditable (ce qui rend la commande `extend_cycle` du Pipeline Manager redondante).
- (c) Ajout de node + edge libre ; le scheduler pickup au tick suivant. Les nodes completed/running ne re-tournent pas.
- (d) **Retirer** un membre d'une région de boucle en vol (compteur de lap actif) est rejeté — les nodes déjà itérés ne seraient plus attendus et la barrière de lap se désynchroniserait. **Agrandir** reste libre (le nouveau membre rejoint au lap suivant).
- (e) Une edge **dangling** (node ou port inexistant ; inputs émergents exemptés) est un simple warning à l'édition mais **refuse le lancement d'un run** : lancer dessus garantit un stall silencieux mid-run.

La réconciliation complète des éditions (événement `PipelineEdited`, application différée) reste hors scope.

*Note de terminologie : « topologie figée » dans CONTEXT.md parle du **déterminisme d'orchestration** (pas de LLM-router probabiliste), pas de l'immutabilité du graphe pendant l'exécution.*
