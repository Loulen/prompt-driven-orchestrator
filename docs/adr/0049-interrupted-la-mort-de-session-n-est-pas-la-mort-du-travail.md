# ADR-0049 — `Interrupted` : la mort de la session n'est pas la mort du travail, et se récupère à la main

> Statut : accepted (grilling du 2026-08-24, spec résilience des runs). Vocabulaire : CONTEXT.md
> § « Cycle de vie d'un run — résilience ». **Amende ADR-0032** : un incident infra ne produit
> plus `NodeFailed`/`RunFailed` mais un état `Interrupted` non terminal. **S'appuie sur ADR-0050**
> (un spawn avorté est un événement — prérequis pour qu'un `Interrupted` soit visible) et
> **ADR-0045** (la capacité à reprendre une session est un critère de harnais).

## Contexte

Sur 599 runs analysés (~2 mois), **85 % des échecs de node sont d'origine infra** — session
tmux disparue (17), node lâché au redémarrage du daemon (29), spawn-abort scheduler — et **non
métier**. Chacun devient aujourd'hui `NodeFailed → RunFailed` terminal, récupéré à 100 % à la
main (`resume_run` ×19, `restart_node` ×6…). Le travail de l'agent est pourtant le plus souvent
intact sur le disque. Le runtime déclare mort un travail qui ne l'est pas.

## Ce qu'on décide

### 1. Un incident infra met le node en `Interrupted`, pas `Failed`

« La session est morte, pas le travail. » `Interrupted` est distinct de `Failed` et **ne
terminalise pas le run** : celui-ci passe `AwaitingUser`, avec une **raison** portée dans
l'event, distincte de l'attente d'un node `interactive`. Un même statut de run (`AwaitingUser`)
pour deux causes, désambiguïsées par la raison — pas de quatrième statut.

### 2. Pas d'auto-retry — atteindre `Interrupted` réclame un humain

Le runtime ne retente jamais de lui-même. C'est l'application directe d'ADR-0012 : l'autonomie
est une propriété du *pipeline*, jamais une initiative du runtime. Un retry automatique
masquerait le problème de fond ; pire, rejouer sur un sous-worktree qui porte un travail partiel
(un rebase à moitié résolu) le **détruirait** — PDO ne peut pas prouver que l'écrivain est mort
(#516). L'alternative « auto-retry borné, cap N » a été instruite puis **rejetée** pour ces deux
raisons.

### 3. Deux mécanismes de récupération, déclenchés par l'humain

- **Resume de la session dans le worktree** (optimal) : réanimer l'agent dans le sous-worktree
  existant du node sans relancer le run. **Harness-spécifique** — conditionné à une capacité
  déclarée (ADR-0045) ; tous les harnais ne savent pas reprendre une session (`claude --continue`
  pour Claude Code).
- **Restart avec les artefacts partiels fournis en input** (défaut, harness-agnostique) : un
  agent frais reçoit le travail partiel **en contexte**, jamais réécrit par-dessus. C'est le
  comportement par défaut quand le resume n'est pas possible.
- **Abandon humain** → `Failed`.

## Ce qu'on ne fait pas (tranché)

- **Aucune détection « vivant mais bloqué »** (#477) : distinguer un agent qui travaille d'un
  agent wedgé demande un oracle de progrès qu'ADR-0038 refuse (pas de sonde d'idle). Hors
  périmètre, assumé.
- **Aucune gestion de la limite d'usage** : hors périmètre.

## Antériorité

#279/#498 (spawn-abort silencieux → ADR-0050), #516 (verrou git : l'écrivain mort est
indémontrable), #592 (double-spawn), ADR-0012 (autonomie méritée), ADR-0032 (la mort de session
comme verdict de liveness), ADR-0045 (le harnais comme axe de capacités).
