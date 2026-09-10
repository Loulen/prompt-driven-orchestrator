# ADR-0068 — La libération de la complétion est une garde du daemon, pas un message dans le pane

> Statut : accepted (grilling #763). Vocabulaire : CONTEXT.md § « Nœuds interactifs — signal de complétion ». Amende ADR-0035 (nouvelle variante de refus) ; ferme un trou de CONTEXT.md § « Cycle de vie process » (l'auto-complétion n'épargnait pas les nœuds interactifs).

## Contexte

Un nœud `interactive: true` n'a qu'une sortie : l'utilisateur force la complétion depuis l'UI
(« Mark complete », artefacts pris tels quels). Il manque le geste intermédiaire — « j'ai fini
d'interagir, l'agent finit son travail et complète quand il est prêt ». Or l'interdiction de
`pdo complete` sur un nœud interactif n'existait **que dans le préambule** : le daemon acceptait
un `pdo complete` d'un nœud interactif, y compris `--auto` (hook Stop, balayage de fin de tour)
quand l'auto-complétion d'instance est cochée — en contradiction avec « n'auto-complète jamais ».

## Décisions

**1. Libérer = lever une garde du daemon.** « Mark ready for completion » enregistre un événement
de libération sur `(node, iter)` ; tant qu'il est absent, **toute** complétion d'un nœud interactif
est refusée (409 nommé, *recoverable* : l'agent garde la main, exit 3 du contrat #490), quelle que
soit la source (`explicit`, `stop_hook`, `turn_ended`). Seul `mark_node_done` — le forçage — passe
outre. Le préambule interactif cesse d'interdire `pdo complete` et décrit ce contrat.

Écartée : **taper un message dans le pane de l'agent** (`send-keys`/`paste_text`) pour lui dire de
compléter. ADR-0051 l'a déjà mesuré : le seul précédent d'injection (message correctif de
frontmatter) ne vaut que pour un agent qu'on sait au repos ; un nœud interactif est précisément une
REPL qu'un humain pilote, et un `Enter` injecté s'insère dans sa phrase. Écartée aussi : une
**capacité par harnais** « injecter un message » — la bonne forme si un jour un harnais l'exige,
pas avant qu'un besoin la justifie.

**2. Le nœud repasse `running`.** Après libération, personne n'attend plus l'humain : le Run quitte
`AwaitingUser` (sauf autre cause). Un retry (itération suivante) ou un `restart_node` (agent frais,
même iter) **réarme** la garde : l'agent neuf ne connaît pas la conversation qui a libéré.

**3. L'agent apprend la libération par l'humain, pas par le runtime.** Un nœud interactif suppose
un humain attaché : il clique, puis dit à l'agent qu'il a fini. Un `pdo complete` prématuré revient
en exit 3 avec la consigne de demander le clic. Écarté : un `pdo complete --wait` bloquant qui
attendrait la libération — il gèlerait la conversation, qui est la raison d'être du nœud. Reste
ouvert si un usage « libérer depuis le dashboard sans réattacher » apparaît.

**4. Idempotent, exposé au manager.** Re-cliquer re-libère sans effet ; la commande
`release_node_completion` porte le même geste depuis le Pipeline Manager (refus nommé sur un nœud
non interactif ou sans session vivante).

## Conséquences

- Le contrat de refus (ADR-0035) gagne une variante *recoverable*. L'auto-complétion (ADR-0032 §2)
  devient réellement inopérante sur un nœud interactif non libéré — c'était l'intention écrite.
- « Mark complete » reste l'échappatoire inconditionnelle, y compris après libération.
