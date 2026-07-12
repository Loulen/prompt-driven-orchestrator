# ADR-0025 — Réponses véridiques des commandes de boucle (extend_cycle / bump_region / end_region / resume_run)

Date : 2026-07-11 · Statut : accepté · Issue : #327

## Contexte

`extend_cycle` répondait `{ok:true}` inconditionnellement : node_id inconnu, membre d'une
région bornée (mauvais mécanisme), ou itération encore vivante — dans tous les cas le
handler appendait le `CommandIssued`, levait le `Halted`, relançait `re_evaluate_after_command`
(qui retourne `()`) et affirmait le succès. `bump_region`/`end_region` avaient le même défaut
pour un `region_id` inconnu. Résultat : boucles bornées non pilotables, Pipeline Manager
trompé par ses propres commandes.

L'issue proposait aussi de **déléguer** : résoudre tout membre de région vers la région et
appliquer un `bump_region` implicite. Refusé après investigation : les deux commandes bumpent
des cibles différentes, enregistrées comme événements différents, lus par des projections
différentes (`collect_cycle_extensions` clé-nœud vs `collect_region_routes` clé-région), et un
nœud à double rôle (membre de région portant sa propre arête `$var`) rend l'intention ambiguë.

## Décision

1. **Rejeter, pas déléguer.** `extend_cycle` sur un membre d'une région bornée → `409` nommant
   la région (message actionnable : « use bump_region with region_id '<region>' »). Le
   prédicat d'appartenance est le même que celui du scheduler (`loops` bornées, `members`
   contient le nœud), extrait en helper partagé. La tête/entrée de région est un membre comme
   un autre → `409` aussi. Les pipelines legacy (`loops:` vide) ne changent pas.
2. **Valider avant d'écrire.** Cible inconnue → `400` avant l'append du `CommandIssued` et
   avant la levée du `Halted`. Source de vérité = snapshot pipeline du Run
   (`resolve_run_pipeline_path`), pas la bibliothèque. Sans risque de replay : les collecteurs
   tolèrent déjà les clés inconnues, les vieux logs projettent à l'identique.
3. **Dire l'effet.** `spawn_node` retourne un `SpawnOutcome`
   (Spawned/Throttled/Refused/Failed), `re_evaluate_after_command` agrège un `ReEvalSummary`.
   Les handlers répondent `{"ok":true,"spawned":[...]}` si effet, ou
   `{"ok":true,"noop":true,"reason":...}` sinon (convention `mark_node_done`). Décision
   synchrone : le détachement ADR-0023 ne couvre que la queue de `node_done`, pas ce chemin.
4. **Documenter le pilotage de région.** Le préambule du manager gagne `bump_region`/`end_region`
   en section 1 (recette de découverte du region_id : clés de `loop_states` dans
   `GET /runs/{id}`, `loop_node_id` des `LoopIterStarted` ; une région au lap 1 n'a pas encore
   d'entrée `loop_states`) ; `extend_cycle` est rétrogradé legacy avec sa sémantique de cible
   explicite (nœud de condition de sortie, jamais la tête).

## Conséquences

- Nouveau statut `400`/`409` visible des clients ; le frontend ne parse pas ces corps
  aujourd'hui (throw générique sur non-2xx) — pas de casse, enrichissement possible ensuite.
- Un nœud à double rôle est poussé vers `bump_region` : le compteur de région est la borne
  autoritaire pour tout ce qui est dans la région (évite le double-bump d'un même lap).
- `resume_run` n'a pas d'identifiant cible : il n'a que le volet « dire l'effet » (noop/spawned).
