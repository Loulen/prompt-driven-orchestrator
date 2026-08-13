# ADR-0043 — Le substrat primaire de la complétion sur fin de tour est un hook `Stop` côté agent ; le balayage daemon en devient le repli

> Statut : accepted (grilling du 2026-08-13, issue #433). Vocabulaire : CONTEXT.md § « Cycle de vie
> process — résilience ». **Amende ADR-0032 §2** : la complétion sur fin de tour reste la même
> politique (opt-in, décochée par défaut, deux gardes), mais gagne un substrat de livraison
> **primaire, event-driven, côté agent** ; le balayage daemon décrit au §2 en devient le **filet de
> repli**. Application directe d'**ADR-0012** (autonomie méritée). Repose sur le contrat d'exit de
> **ADR-0035** (un output manquant est un refus récupérable, rien n'est enregistré).

## Contexte

ADR-0032 §2 a rendu la complétion sur fin de tour possible mais l'a livrée par un **seul** substrat :
le balayage de liveness du daemon, qui lit le transcript JSONL (`parse_turn_state`) et complète le
nœud. Or ce substrat est fragile pour la raison même qui motive #433 : depuis #469 il n'existe **plus
aucun** filet de staleness, donc un nœud qui a fini son travail sans jamais appeler `pdo complete`
reste dans le REPL, **vivant et immobile**, et suspend son Run **indéfiniment** — seule la veille
humaine le rattrape (mesuré : trois occurrences en 27 h, sur trois pipelines et trois rôles
distincts, dont une bloquée *sur une question*, les trois ayant écrit leur sortie avant de se taire).

Le balayage seul ne suffit pas à rendre le filet *fiable donc activable* : sa détection dépend de la
résolution du bon fichier transcript (fragile — #473), d'une heuristique de parse du JSONL (format
non contractuel de CC) et d'une latence de balayage (~30 s). Tant qu'il est le seul substrat, on ne
peut pas raisonnablement cocher la case.

## Ce qu'on décide

**Ajouter un hook `Stop` de Claude Code, injecté par le runtime dans chaque session d'agent, qui
exécute `pdo complete --auto` à chaque fin de tour.** C'est le substrat **primaire** ; le balayage
d'ADR-0032 §2 reste en place comme **repli**. Les deux passent par le même chemin partagé que
`pdo complete` (merge du sous-worktree compris) et sont **idempotents** entre eux : qui arrive second
obtient un no-op.

1. **`Stop`, pas `SessionEnd` ni le mode `-p`.** PDO lance `claude --dangerously-skip-permissions
   "<prompt>"` en REPL **interactif** : à la fin d'un tour le process **ne sort pas**, il reste
   résident — c'est précisément l'état du nœud muet. `Stop` se déclenche à cette fin de tour, *pendant
   que claude est encore vivant*, dans la fenêtre exacte où un `pdo complete` a du sens. `SessionEnd`
   se déclenche à la sortie du process (donc jamais, ici) et sous un budget de temps trop court pour
   `pdo complete`. Aucun champ du payload `Stop` ne distingue « fini » de « arrêté sur une question » —
   sans importance : session non-interactive ⇒ personne ne répond, un filet inconditionnel attrape les
   deux, et la garde « outputs valides » (ci-dessous) protège le second cas.

2. **Wrapping `pdo complete --auto; exit 0`, jamais `; exit $?`.** Un hook `Stop` ne force la
   *poursuite* du tour que s'il sort en **exit 2** ou imprime `{"decision":"block"}` ; **exit 0**
   laisse le tour se terminer. Le `; exit 0` neutralise donc tout code non nul de `pdo complete` — en
   particulier l'**exit 3** d'un output encore manquant (ADR-0035 : refus récupérable, *rien
   n'est enregistré*, le nœud reste `running`). Conséquence : le hook ne peut **jamais** boucler ni
   forcer une complétion prématurée. Propager `$?` réintroduirait ces deux dangers.

3. **Réutiliser le réglage `autocomplete_turn_end`, sans second interrupteur, décoché par défaut.**
   Le hook est une **action durable initiée par le runtime** (le runtime l'injecte ; l'agent ne l'a
   pas demandé), de la même classe que le balayage daemon. ADR-0012 en fait une autonomie qui *se
   mérite* : même opt-in, même défaut OFF. Scinder une seule politique sur deux switches
   fragmenterait sa signification. Décoché ⇒ le `--settings` n'est **pas** injecté (résolu à chaud au
   spawn, comme les réglages frères d'ADR-0015 : effet au prochain nœud, sans redémarrage).

4. **Marqueur d'audit `CompletionSource::StopHook`, projeté en `NodeAutoCompleted`.** La complétion
   étant initiée par le runtime, le journal doit lire « automatique », pas « l'agent a décidé »
   (`Explicit`). On **réutilise** l'`EventKind::NodeAutoCompleted` existant (pas de nouveau variant
   d'événement : projection et UI inchangées) ; seul le libellé de log du daemon distingue le hook
   (`auto:stop_hook`) du balayage (`auto:turn_ended`). `pdo complete --auto` porte cette source de
   bout en bout.

5. **Injection par `claude --settings <fichier éphémère par nœud>`.** Les hooks passés par
   `--settings` **fusionnent** additivement avec la hiérarchie (`~/.claude`, `.claude` projet) : le
   hook injecté ne remplace jamais ceux de l'utilisateur. Le fichier vit **sous l'arbre du dépôt**
   (à côté du prompt éphémère déjà écrit par nœud), donc il se résout à un chemin **identique** sur
   l'hôte et dans le conteneur (mount identité `{repo}:{repo}`, tous profils), sans jamais toucher
   `~/.claude`. Nœuds `script` immunisés par construction (bash, pas de claude — ADR-0017).

## Alternatives écartées

- **Un second interrupteur dédié au hook.** Rejeté : le hook et le balayage servent *une* politique
  (compléter un nœud vivant qui a fini son tour). Deux switches pour un comportement, c'est deux
  états à expliquer et à tenir cohérents, pour aucun gain.
- **Le hook appelle `pdo complete` nu (source `Explicit`).** Rejeté : le journal mentirait sur
  l'origine de la complétion (agent vs runtime), effaçant la distinction qu'ADR-0032 a introduite.
- **Un nouveau `EventKind` `NodeHookCompleted`.** Rejeté : forcerait des bras de projection,
  transition-guard et tests, pour une information (hook vs balayage) que le libellé de log suffit à
  porter — les deux sont « auto » pour la projection.
- **`SessionEnd` / mode `-p`.** Écartés : ne couvrent pas le nœud muet (voir §1).

## Portée et limites

- **Basculer le défaut OFF→ON n'est pas dans ce lot.** Ce serait renverser le cœur d'ADR-0012 (le
  runtime ne complète pas un nœud vivant de sa propre initiative par défaut) : décision humaine
  séparée. La valeur livrée ici est de rendre le filet existant *fiable donc activable*.
- **Le hook doit être ré-injecté à la reprise.** Une session ressuscitée (`resume`, recovery au boot)
  relance un `claude --continue` par un chemin distinct du spawn initial ; sans réinjection du
  `--settings`, la session reprise perd le hook (le balayage-repli la couvre encore, mais le substrat
  primaire doit survivre). C'est une contrainte de mise en œuvre, pas une exception à la décision.
- **Sessions Manager et merge-resolver hors périmètre** : elles ne passent pas par le tail d'agent,
  donc n'obtiennent pas le hook — voulu.

## Antériorité

ADR-0032 (#469, amendé ici), ADR-0012 (autonomie méritée), ADR-0015 (patron du réglage résolu à
chaud), ADR-0035 (contrat d'exit `0/3/4/1` sur lequel repose la sûreté du wrapping), ADR-0017
(immunité des nœuds `script`), ADR-0030/0031 (mount identité qui rend le fichier de settings valide
en conteneur), #469 (suppression du détecteur `Stale`), #473 (fragilité de la résolution du
transcript, qui minait le balayage-repli).
