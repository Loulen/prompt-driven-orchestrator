# ADR-0025 — Réponses véridiques des commandes de boucle (extend_cycle / bump_region / end_region / resume_run)

Date : 2026-07-11 · Statut : accepté · Issue : #327

> La convention noop de §3 (« dire l'effet », jamais « un `200` suffit à le dire ») tient pour les
> quatre commandes de boucle mais ne s'arrête pas là : **noop ≠ refus**, et un refus n'est jamais un
> `2xx` — y compris quand le refus suit un effet de bord irréversible (une commande qui détruit une
> session tmux doit valider *avant* de détruire, pas rendre un `4xx` après coup comme s'il s'agissait
> d'une validation). Le throttle d'un spawn par nœud (`restart_node`, `node_retry`) qui répond
> `{"ok":true,"waiting":true,…}` n'est pas non plus un noop : un `NodeWaiting` a été appendé et a
> changé le statut du nœud. Voir ADR-0035 et ADR-0037.

## Contexte

Sans cette ADR, une commande de boucle répond `{ok:true}` inconditionnellement — node_id inconnu,
membre d'une région bornée (mauvais mécanisme), ou itération encore vivante : le handler appendait le
`CommandIssued`, levait le `Halted`, relançait une réévaluation qui ne retourne rien, et affirmait le
succès. Boucles bornées non pilotables, Pipeline Manager trompé par ses propres commandes.

L'issue proposait aussi de **déléguer** : résoudre tout membre de région vers la région et appliquer
un `bump_region` implicite. Refusé après investigation : les deux commandes bumpent des cibles
différentes, enregistrées comme événements différents, lus par des projections différentes
(clé-nœud vs clé-région), et un nœud à double rôle (membre de région portant sa propre arête `$var`)
rend l'intention ambiguë.

## Décision

1. **Rejeter, pas déléguer.** `extend_cycle` sur un membre d'une région bornée → `409` nommant la
   région, avec un message actionnable pointant `bump_region`. Le prédicat d'appartenance est le même
   que celui du scheduler, extrait en helper partagé. La tête/entrée de région est un membre comme un
   autre. Les pipelines legacy (`loops:` vide) ne changent pas.
2. **Valider avant d'écrire.** Cible inconnue → `400` avant l'append du `CommandIssued` et avant la
   levée du `Halted`. Source de vérité = **snapshot pipeline du Run**, pas la bibliothèque. Sans
   risque de replay : les collecteurs tolèrent déjà les clés inconnues.
3. **Dire l'effet.** `spawn_node` retourne un `SpawnOutcome` (Spawned/Throttled/Refused/Failed),
   agrégé par la réévaluation. Les handlers répondent `{"ok":true,"spawned":[...]}` si effet, ou
   `{"ok":true,"noop":true,"reason":…}` sinon. Décision **synchrone** : le détachement ADR-0023 ne
   couvre que la queue de `node_done`, pas ce chemin.
4. **Documenter le pilotage de région.** Le préambule du manager gagne `bump_region`/`end_region`
   avec la recette de découverte du region_id (une région au lap 1 n'a pas encore d'entrée
   `loop_states`) ; `extend_cycle` est rétrogradé legacy avec sa sémantique de cible explicite (nœud
   de condition de sortie, jamais la tête).

## Conséquences

- Nouveaux statuts `400`/`409` visibles des clients ; le frontend throw générique sur non-2xx — pas
  de casse, enrichissement possible ensuite.
- Un nœud à double rôle est poussé vers `bump_region` : le compteur de région est la borne
  autoritaire pour tout ce qui est dans la région (évite le double-bump d'un même lap).
- `resume_run` n'a pas d'identifiant cible : il n'a que le volet « dire l'effet ».
