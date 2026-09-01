# ADR-0043 — Le substrat primaire de la complétion sur fin de tour est un hook `Stop` côté agent ; le balayage daemon en devient le repli

Sans cet ADR, un agent s'en remettrait au seul balayage daemon (parse de transcript JSONL, latence
~30 s, résolution de fichier fragile) pour détecter une fin de tour — un filet trop peu fiable pour
qu'on ose l'activer.

> Statut : accepted (grilling du 2026-08-13, issue #433). Vocabulaire : CONTEXT.md § « Cycle de vie
> process — résilience ». **Amende ADR-0032 §2** : même politique (opt-in, décochée par défaut, deux
> gardes), mais un substrat de livraison **primaire, event-driven, côté agent** ; le balayage devient
> le **filet de repli**. Application d'**ADR-0012**. Repose sur le contrat d'exit d'**ADR-0035**.

## Contexte

Depuis #469 il n'existe **plus aucun** filet de staleness : un nœud qui a fini son travail sans
appeler `pdo complete` reste dans le REPL, **vivant et immobile**, et suspend son Run
**indéfiniment** (mesuré : trois occurrences en 27 h, sur trois pipelines et trois rôles distincts,
dont une bloquée *sur une question*, les trois ayant écrit leur sortie avant de se taire).

## Ce qu'on décide

**Ajouter un hook `Stop` de Claude Code, injecté par le runtime dans chaque session d'agent, qui
exécute `pdo complete --auto` à chaque fin de tour.** C'est le substrat **primaire** ; le balayage
d'ADR-0032 §2 reste en place comme **repli**. Les deux passent par le même chemin partagé que `pdo
complete` (merge du sous-worktree compris) et sont **idempotents** : qui arrive second obtient un
no-op.

1. **`Stop`, pas `SessionEnd` ni le mode `-p`.** PDO lance `claude` en REPL **interactif** : à la fin
   d'un tour le process **ne sort pas**, il reste résident — c'est précisément l'état du nœud muet.
   `Stop` se déclenche à cette fin de tour, *pendant que claude est encore vivant*. `SessionEnd` se
   déclenche à la sortie du process (donc jamais, ici) et sous un budget de temps trop court. Aucun
   champ du payload `Stop` ne distingue « fini » de « arrêté sur une question » — sans importance :
   session non-interactive ⇒ personne ne répond, et la garde « outputs valides » protège le second
   cas.

2. **Wrapping `pdo complete --auto; exit 0`, jamais `; exit $?`.** Un hook `Stop` ne force la
   *poursuite* du tour que s'il sort en **exit 2** ou imprime `{"decision":"block"}`. Le `; exit 0`
   neutralise donc tout code non nul — en particulier l'**exit 3** d'un output encore manquant
   (ADR-0035 : refus récupérable, rien n'est enregistré, le nœud reste `running`). Conséquence : le
   hook ne peut **jamais** boucler ni forcer une complétion prématurée. Propager `$?` réintroduirait
   ces deux dangers.

3. **Réutiliser le réglage `autocomplete_turn_end`, sans second interrupteur, décoché par défaut.**
   Le hook est une **action durable initiée par le runtime**, de la même classe que le balayage.
   ADR-0012 en fait une autonomie qui *se mérite*. Décoché ⇒ le `--settings` n'est **pas** injecté
   (résolu à chaud au spawn, comme les réglages frères d'ADR-0015).

4. **Marqueur d'audit `CompletionSource::StopHook`, projeté en `NodeAutoCompleted`.** La complétion
   étant initiée par le runtime, le journal doit lire « automatique », pas `Explicit`. On **réutilise**
   l'`EventKind` existant (projection et UI inchangées) ; seul le libellé de log distingue le hook
   (`auto:stop_hook`) du balayage (`auto:turn_ended`).

5. **Injection par `claude --settings <fichier éphémère par nœud>`.** Les hooks passés par
   `--settings` **fusionnent** additivement avec la hiérarchie : le hook injecté ne remplace jamais
   ceux de l'utilisateur. Le fichier vit **sous l'arbre du dépôt**, donc il se résout à un chemin
   **identique** sur l'hôte et dans le conteneur (mount identité), sans jamais toucher `~/.claude`.
   Nœuds `script` immunisés par construction (ADR-0017).

## Alternatives écartées

- **Un second interrupteur dédié au hook.** Le hook et le balayage servent *une* politique ; deux
  switches, c'est deux états à expliquer et à tenir cohérents.
- **Le hook appelle `pdo complete` nu (source `Explicit`).** Le journal mentirait sur l'origine
  (agent vs runtime).
- **Un nouveau `EventKind` `NodeHookCompleted`.** Forcerait des bras de projection, transition-guard
  et tests, pour une information que le libellé de log suffit à porter.
- **`SessionEnd` / mode `-p`.** Ne couvrent pas le nœud muet (voir §1).

## Portée et limites

- **Basculer le défaut OFF→ON n'est pas dans ce lot.** Ce serait renverser le cœur d'ADR-0012 :
  décision humaine séparée. La valeur livrée ici est de rendre le filet *fiable donc activable*.
- **Le hook doit être ré-injecté à la reprise.** Une session ressuscitée relance un `claude
  --continue` par un chemin distinct du spawn initial ; sans réinjection du `--settings`, elle perd
  le substrat primaire (le balayage-repli la couvre encore).
- **Sessions Manager et merge-resolver hors périmètre** : elles ne passent pas par le tail d'agent,
  donc n'obtiennent pas le hook — voulu.

## Antériorité

ADR-0032 (amendé ici), ADR-0012, ADR-0015, ADR-0035 (contrat d'exit sur lequel repose la sûreté du
wrapping), ADR-0017, ADR-0030/0031 (mount identité), #469, #473.
