# L'attente utilisateur est déclarée par l'agent, et le suivi des enfants est un pull bloquant

> Statut : accepted (grilling #588). Vocabulaire : CONTEXT.md § « Nœuds interactifs — attente déclarée », § « Orchestration récursive › Attente d'enfants », § « Préambule de base ». Amende ADR-0068 (second déclencheur du retour à `running`) et ADR-0064 (forme canonique du suivi des enfants ; deux skills seedés).

## Contexte

Un nœud `interactive: true` était marqué `awaiting_user` **au spawn** : le run passait orange avant
que l'agent ait fait quoi que ce soit, et le restait pendant tout son travail. Le signal ne voulait
plus rien dire (#588). Côté orchestration, le skill `pdo-orchestrate` demandait à l'agent de
« sonder périodiquement » ses enfants : un minuteur dans un prompt, la pire des attentes.

## Décisions

**1. L'attente est déclarée, jamais inférée.** Un nœud à session vivante passe `awaiting_user`
quand son agent le dit (`pdo wait-user`, message optionnel porté en bannière) ou quand le daemon
refuse son `pdo complete` faute de libération (l'agent attend alors bel et bien le clic). Elle est
levée par la **touche Entrée d'un humain traversant le pont PTY** de l'UI, ou par la libération de
la complétion (ADR-0068). Accepté sur tout nœud, interactif ou non : un nœud coincé sur une question
devient visible au lieu de rester silencieusement `running` (#290).

Écartées : **inférer du spawn** (le comportement corrigé) ; **hooks de harnais** (`Stop` /
`UserPromptSubmit`) comme substrat primaire — capacité `claude` seulement, alors qu'une commande CLI
vaut pour tous les harnais ; **une commande de reprise** que l'agent lancerait après la réponse —
charge inutile là où la frappe humaine est déjà observable. Limite acceptée : un `tmux attach`
direct contourne le pont ; le clic reste le second chemin.

**2. Le préambule est le même pour tous les nœuds.** Il n'explique ni l'interactivité ni
l'orchestration : les toggles ajoutent un skill seedé (`pdo-interactive`, `pdo-orchestrate`) et une
ligne d'invocation. Tout nœud tente `pdo complete` ; le refus nommé lui apprend ce qu'il est.
Mesuré : les blocs conditionnels du préambule redisaient le contrat déjà porté par les refus, et
l'amendement Orchestration redisait le skill.

**3. Le suivi des enfants est un pull bloquant, jamais un push.** `pdo run wait` bloque sur un
long-poll du daemon jusqu'à ce qu'un enfant de ce nœud soit **terminal** ; l'agent ne rend pas la
main, donc aucun conflit avec la liaison d'ADR-0064 ni avec le hook `Stop` d'auto-complétion.
Écarté : coller un message dans le pane du parent — ADR-0051 et ADR-0068 l'ont déjà mesuré comme
dangereux pour une REPL, et ici il entrerait en course avec le watcher de liaison qui complète le
nœud dès que les enfants sont terminaux.

**4. L'attente d'un enfant se propage au parent par dérivation, pas par événement.** Un enfant
`awaiting_user` ne réveille pas `pdo run wait` (ce n'est pas terminal) mais rend le nœud parent, et
son run, `awaiting_user` à la lecture — comme `stalled`. Écarté : écrire un `NodeAwaitingUser` sur
le parent, qui obligerait à compter les enfants parkés et à dé-propager à chaque reprise.

## Conséquences

- Un nœud interactif fraîchement spawné a la couleur d'un nœud qui travaille. Le « viens
  t'attacher » n'existe que quand il est vrai.
- Le mécanisme de seed (ADR-0064) porte deux skills ; le toggle `interactive` sème
  `pdo-interactive` comme `orchestrator` sème `pdo-orchestrate`.
- Sous un harnais qui plafonne la durée d'un appel d'outil, l'agent boucle sur
  `pdo run wait --timeout` ; le code de sortie « délai écoulé » n'est pas un échec.
