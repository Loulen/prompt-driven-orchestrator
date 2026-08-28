# ADR-0050 — Un échec de spawn est toujours un événement ; le spawn est idempotent

> Statut : accepted (grilling du 2026-08-24, spec résilience des runs). Vocabulaire : CONTEXT.md
> § « Cycle de vie d'un run — résilience ». **Étend ADR-0037** : « un abort n'est jamais un 2xx » ne
> valait que sur la réponse HTTP ; il vaut désormais sur la couche **scheduler/event**, où il n'y a
> pas de réponse à dire vrai. **Fondation d'ADR-0049** : un `Interrupted` visible suppose un spawn
> qui appende son propre échec.

## Contexte

Sans cette ADR, un échec de spawn côté scheduler ne fait qu'un `error!` et laisse le run figé
`running`, seul `journalctl` connaissant la cause. Deux gaps ouverts (1.31.1) :

- **#498** — un échec de spawn sur le chemin scheduler (`git worktree add -b` sur une branche
  survivante) est **fondu dans le seau `skipped`** avec les temporisations bénignes. Aucun événement
  appendé → les **trois filets ratent** : la liveness ne voit pas de session morte, le stall detector
  ne voit pas de node `Running`, `run_stall_reason` voit un node « schedulable ».
- **#592** — `resume_run` et le balayage d'admission spawnent le **même `(node, iter)`** ; le perdant
  renvoie `duplicate session`, traité comme `node_failed`, qui **écrase le gagnant vivant** →
  `RunFailed` irrécupérable en boucle fermée. Récidive de l'invariant « une seule itération vivante »
  (#196/#201/#212) sur le chemin `resume`, non couvert par la garde.

## Ce qu'on décide

### 1. Un échec de spawn appende un événement, toujours

Tout échec ou abort de spawn sur le chemin scheduler appende un **événement terminal**
nommant le node et la cause. Le run se réconcilie visiblement (→ `Interrupted`/`AwaitingUser`,
ADR-0049), jamais figé. L'événement est le **seul canal** de vérité côté scheduler — il n'y a pas de
réponse HTTP à laquelle mentir.

### 2. Le spawn est idempotent

- **Reap avant recréation** : un worktree ou une branche survivant est ramassé avant de recréer, ce
  qui clôt la collision de #498.
- **Garde « une seule itération vivante » partagée** entre le handler de `resume` et le balayage
  d'admission — l'invariant #212 cesse d'avoir un chemin qui le contourne.
- **`duplicate session` est un perdant bénin** : un `no-op`, jamais un `node_failed`.

### 3. La classification de sous-worktree tolère l'environnement de l'agent

PDO **n'impose pas** à l'agent son git (ADR-0001) ; il encaisse ce que l'agent fait. Un worktree à
**notre propre chemin** mais sur une branche nommée ≠ `pdo/sub-*` — l'agent a fait `git checkout -b
feature/…` en suivant un git-flow projet — est classé **`Reusable`** (c'est le travail du node), pas
`Occupied`. Le `Occupied` légitime reste « la branche est checkoutée dans un **autre** worktree
vivant ».

## Ordre de livraison

Cette ADR est la **fondation** : sans (2), toute reprise (ADR-0049) ou ré-ouverture (amendement
ADR-0032) rouvre la course de #592 ou re-heurte la branche de #498. À livrer en premier.

## Antériorité

#498, #592, #196/#201/#212, #279 (la classe « spawn abort silencieux »), ADR-0037, ADR-0009
(la primitive de spawn de référence porte les gardes), ADR-0038 (réservation avant session).
