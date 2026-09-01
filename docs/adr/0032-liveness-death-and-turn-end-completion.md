# ADR-0032 — Liveness : la mort de session est le seul verdict terminal, la complétion se déclenche sur fin de tour

Sans cette ADR, on garderait un seuil d'inactivité (mtime du transcript) pour déclarer un nœud mort, et
on tuerait des nœuds sains : le seuil n'est pas mal calibré, il est structurellement incapable. Rien
dans le code ne dit qu'un nœud sain reste silencieux plusieurs minutes d'affilée.

> Statut : accepted (#469). Amende **ADR-0012** (le réglage §2 est l'application de l'autonomie
> méritée) et **#214** (*surfacer* et *tuer* redeviennent distincts pour un nœud vivant). Clôt
> **#373 Unit B** en won't-do (§3). Remplace la conception mtime de **#123**.
>
> **Amendé par ADR-0038.** « Exact par construction » vaut pour le *détecteur* et était faux pour le
> *reaper*, qui **fabriquait** la mort que le détecteur observait ensuite fidèlement. Le verdict
> terminal reste le bon ; ce qu'il faut lire avec lui, c'est **qui** a tué la session.
>
> **Amendé par ADR-0045.** « La mort de session est le seul verdict terminal » cesse d'être un constat
> sur Claude Code pour devenir un **critère d'éligibilité** d'un harnais : un harnais qui sort en fin
> de travail rendrait ce verdict indiscernable d'un succès, donc il est refusé.
>
> **Amendé par ADR-0043.** Le §2 gagne un substrat de livraison **primaire, event-driven, côté
> agent** (hook `Stop` exécutant `pdo complete --auto`) ; le balayage daemon en devient le **repli**.
> Politique, gardes et chemin partagé inchangés.
>
> **Amendé par ADR-0049 et ADR-0050.** Le §1 tient pour la *détection*, mais sa **conséquence
> change** : un incident infra ne produit plus `NodeFailed → RunFailed` mais un `Interrupted` non
> terminal, et un spawn avorté appende un événement au lieu de figer le run `running`.

Trois décisions durables, arbitrées sur deux réponses du owner : *on veut détecter Mort, pas un seuil
qui ne serait pas robuste* ; *un faux positif ne doit pas coûter un Run*.

## Le fait de code qui commande le reste

**Pour un nœud agent, la mort de l'agent EST la mort de la session tmux, par construction.**
`tmux_session_manager::wrap_with_env` émet `exec bash -c '<exports> && <tail>'` et `build_agent_tail`
émet `exec claude …` : double `exec`, donc le processus `claude` **est** le leader du pane, seul window
de la session. Il sort → le pane meurt → la session meurt → `session_alive == false`. Idem en sandbox :
le pane porte le client `docker exec`, qui rend la main dès que claude sort dans le conteneur.
`remain-on-exit` n'est jamais armé.

Conséquence : **une sonde de fraîcheur (mtime du transcript) n'apporte rien à la détection de mort.**
Ce qu'elle attrape en plus, c'est exclusivement l'agent *vivant mais silencieux*.

## Ce qu'on décide

### 1. La mort de session est le seul verdict terminal de liveness

`Detection::Stale` et `Detection::AutoComplete` sont supprimés, avec `STALE_THRESHOLD` (120 s) et la
sonde d'idle qui les alimentait. `stale_detector::decide` se réduit à `!session_alive → SessionDied`,
sinon `Ok`. **Aucun seuil, donc aucun faux positif possible.**

Ce n'était pas un seuil mal calibré, c'était un seuil structurellement incapable. Mesuré sur le
transcript réel du nœud tué (`XBG5Cxkn`, 679 records) : **cinq trous au-dessus du seuil dans un seul
nœud sain** — 155 s, 185 s, 214 s, 270 s, 291 s — venant de `docker build` et de
`cargo test --workspace`. Le nœud a été tué à 43 minutes de travail par un `docker build` qui portait
un `timeout 1800`.

**Coût assumé** : un agent vivant mais wedgé pour toujours (prompt interactif, menu #290, retries API
épuisés) garde son slot indéfiniment, sans état terminal et sans que personne le tue. C'est le prix de
« un faux positif ne doit pas coûter un Run », et c'est un coût **borné et visible** — sans commune
mesure avec la perte d'un Run de 45 minutes.

**Ce qui est conservé, non négociable** : `EventKind::NodeStale`, sa projection, `NodeStatus::Stale`,
`validate_stale`, les bras `Stale` de `validate_fail` / `run_stall_reason`. Le log est **append-only** :
retirer le variant ferait échouer la désérialisation de tout Run historique qui en porte un, donc
`project()` renverrait `None` et **ces Runs disparaîtraient de l'UI**. Ce sont des lecteurs sans
producteur, et ils doivent le rester.

**Conséquences en cascade**, obtenues sans les coder : plus de nœud `Stale` ⇒ le nœud reste `Running`
⇒ `can_progress()` vrai ⇒ `reconcile_run_level_stall` ne voit jamais « aucun nœud vivant », et
`validate_completion` accepte le `node_done` tardif. Les quatre verrous de #469 tombent ensemble.

### 2. La complétion automatique se déclenche sur une fin de tour constatée, jamais sur une durée

> **Livraison (ADR-0043).** Cette politique a désormais **deux substrats**, gatés sur le même
> réglage : un hook `Stop` côté agent (`pdo complete --auto`) — **primaire**, event-driven, sans
> lecture de transcript — et le balayage daemon décrit ci-dessous — **repli**. Les deux passent par
> le chemin partagé et sont idempotents entre eux (qui arrive second obtient un no-op). Ce qui suit
> décrit le substrat de repli et la politique commune (gardes, états, réglage) ; les deux gardes
> valent identiquement pour le hook.

Le danger n°1 (« l'agent a fini sans appeler `pdo complete` ») n'est **pas** un cas de mort :
`claude --dangerously-skip-permissions "<prompt>"` ne sort pas à la fin d'un tour, il reste dans le
REPL. Cet agent est donc **vivant et immobile**, et il a une signature *positive*.

`stale_detector::parse_turn_state` lit les N derniers Ko (256 Ko) du `.jsonl` et répond quatre états,
dont **un seul est actionnable** :

| état | signification | actionnable |
| --- | --- | --- |
| `InToolCall` | un `tool_use` sans `tool_result` : l'agent est *dans* un appel d'outil, quelle que soit la durée du silence | non |
| `AwaitingAssistant` | dernier record substantiel = `user`/`tool_result` : l'assistant doit encore répondre — **c'est le cas des retries API épuisés (#251)** | non |
| `TurnEnded` | dernier substantiel = message `assistant`, aucun `tool_use` pendant | **oui** |
| `Unknown` | pas de transcript, rien de parsable, ou un seul record dépassant la fenêtre | non (« au travail ») |

Un **record substantiel** porte un objet `message` de `role` `assistant` ou `user`. Cette définition
est porteuse : un « regarde la dernière ligne » naïf lit un des records de métadonnées non horodatés
que CC écrit en queue (`last-prompt`, `ai-title`, `mode`, `permission-mode`) et ne conclut rien.

**Deux gardes indépendantes**, toutes deux obligatoires : `TurnEnded` **et** outputs valides
(`outputs_validator::validate`). La seconde couvre l'agent qui termine son tour pour *poser une
question* : tour fini, travail pas fini. Un **anti-rebond** de 60 s depuis la dernière écriture précède
les deux : la mtime survit dans **ce seul rôle**, et ce n'est plus un oracle.

**`Unknown` se comporte comme « au travail »** : à signal absent, on ne touche à rien. Fail-safe par
construction.

**Limites assumées.** Le format JSONL de CC n'est pas un contrat documenté — même précaution que les
ancres de pane de #290 — même si les blocs `tool_use` / `tool_result` avec leurs `id` en sont la partie
la plus stable, très loin devant le wording d'un menu. Un agent bloqué sur un prompt interactif garde
un `tool_use` pendant : il ne sera **jamais** complété, cohérent avec « on ne détecte que Mort ou Fini,
jamais Bloqué ». Un nœud coincé sur le menu de limite d'usage (#290) l'est de même *par construction* :
la limite tombe en *demandant* le message assistant suivant, donc son dernier record substantiel est un
`user`/`tool_result` → `AwaitingAssistant`. Aucune troisième garde n'a été ajoutée pour lui.

**Le réglage** est global instance, **décoché par défaut** (patron ADR-0015 : colonne
`autocomplete_turn_end` 0/1, couture env `PDO_AUTOCOMPLETE_TURN_END`, précédence
`stored → env → default(false)`), libellé sur ce qui est *mesuré* et jamais sur une durée. C'est
l'application directe d'ADR-0012 : une action durable initiée par le runtime se mérite. Décoché,
**aucune lecture de transcript** : le chemin par défaut devient un `session_exists` par nœud, donc
*moins cher* qu'avant #469 (qui payait un `read_dir` plus une validation d'outputs par nœud et par
tick). Le réglage est résolu **une fois par balayage**, pas au boot (précédent du TTL reaper, #129) :
un basculement prend effet en moins de 30 s sans redémarrage.

**L'action passe par le même chemin que `node_done`**, pas par un append. Le corps du handler est levé
dans `complete_node_iteration` (le handler ne garde que l'adaptation HTTP) et le balayage l'appelle
avec `CompletionSource::TurnEnded`. Sans cela, sur un nœud `code-mutating`/`merge`, un
`NodeAutoCompleted` appendé directement produirait un `Completed` avec le commit resté sur
`pdo/sub-…` et l'aval qui ne reçoit rien : `commit_and_merge_sub_worktree_inner` vit **au-dessus** de
`run_advance::complete_node`, dans le handler. L'événement émis est `EventKind::NodeAutoCompleted` (déjà
projeté comme une complétion, déjà couvert par la même garde) — pour que le log dise que la complétion
est automatique.

**Résolution du transcript : par identité de session, jamais par cwd (#473).** Résoudre « *son*
transcript » par cwd est faux : un nœud ni `code-mutating` ni `merge` partage le worktree du Run avec la
session manager — un dossier, plusieurs `.jsonl`, et le plus récent est **souvent celui du manager**. La
même racine frappait `claude --continue` au resume, actif même case décochée. Correctif : **PDO épingle
un `sessionId` au spawn** (`--session-id <uuid>`, enregistré sur `NodeStarted`), la sonde résout par nom
exact et le resume cible ce transcript-là. Les infra sessions restent sans id : elles n'ont pas de
`NodeStarted`, ne sont jamais sondées ni reprises. Une ligne d'avant #473 retombe sur l'ancienne
résolution mtime — aucune migration. **L'invariant byte-identité du tail** ne vaut donc plus pour un nœud
agent ; il est conservé pour le cas `None`, et la parité host≡sandbox reste entière.

### 3. #373 Unit B est fermé en won't-do

Rearmer `AutoCompletePolicy::Act`, c'était compléter un nœud idle depuis 120 s dont les outputs
valident — exactement le nœud de #469, en pire, puisque le pipeline avancerait **sous un agent
vivant**. `AutoCompletePolicy` et `EventKind::NodeAutoCompleteObserved` disparaissent donc du
producteur. La décision est écrite ici pour qu'on ne « finisse pas le travail commencé » dans six mois.

## Ce qu'on ne fait pas (tranché, ne pas réintroduire)

- **Aucune observation quand la case est décochée** : ni événement non terminal, ni badge, ni gauge.
  Pas de `NodeAutoCompleteObserved` réincarné.
- **Aucun rattrapage.** `validate_completion` n'est **pas** déverrouillé pour `Stale` : les nœuds
  `Stale` historiques restent irrécupérables, et le bouton « Mark complete » rendu pour eux continue de
  cliquer dans le vide. Le Run `20260729-074716-047c2cb` et le travail de #466 sont abandonnés côté PDO
  (à l'époque `RunResumed` ne relevait que `Paused → Running` ; la reprise d'un `Failed` est
  désormais possible — ADR-0049 — mais ne réhabilite pas pour autant les nœuds `Stale` historiques).
- **Reprise d'un Run terminal : désormais possible (ADR-0049).** La spec résilience lève cet
  interdit — un état terminal n'est plus un verrou (ré-ouverture par re-projection sûre, y compris
  `Completed`/`Skipped`), et un incident infra ne fait plus `RunFailed` mais `Interrupted →
  AwaitingUser`. `Failed` ne vient plus que d'un `pdo fail` délibéré d'un agent ou d'un abandon
  humain.
- **Pas de sonde d'arbre de processus, pas de seuil ajusté, pas de seuil par nœud.**
- **Le point ambre de #180 ne s'allumera plus jamais.** `event_log::is_stalled` ne teste que
  `NodeStatus::Stale` ; sans producteur, il est constamment faux. C'est une **suppression de
  fonctionnalité assumée**, écrite ici pour que personne ne rebranche un producteur afin de « réparer »
  le point ambre. Le bandeau ambre et ses boutons Stop/Retry restent en place : les Runs historiques
  projettent toujours `Stale`. Ils deviennent du code mort pour tout nouveau Run.

## Contrepartie sur la posture fail-fast

CONTEXT.md § *Cycle de vie process — résilience* pose « jamais d'auto-réparation silencieuse, toute
divergence est rendue visible (état `Failed` avec cause lisible) ». Après #469, un agent
vivant-mais-wedgé est une divergence **ni** visible **ni** `Failed`. C'est un amendement conscient, pas
un oubli : la posture continue de valoir pour tout ce que le runtime *sait*, et refuse désormais de
conclure sur ce qu'il ne sait pas. Un nœud immobile reste `Running`, avec sa session attachable et son
pane lisible — l'humain garde Stop et Retry. La contradiction relevée par #469 ne change donc pas de
camp en silence ; elle est arbitrée, et c'est le seuil qui perd.

Les nœuds `script` (ADR-0017) sont immunisés de §2 par construction : pas de `claude`, donc pas de
transcript, donc `Unknown`.
