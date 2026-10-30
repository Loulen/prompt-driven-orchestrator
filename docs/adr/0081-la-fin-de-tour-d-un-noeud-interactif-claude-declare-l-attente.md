# La fin de tour d'un nœud interactif sous `claude` déclare l'attente ; l'utilisateur répond depuis le panneau

> Statut : accepted (retours du test de « Reply », 2026-10-14). **Amende ADR-0069 §1** : un hook de
> harnais devient un substrat **complémentaire** de l'attente déclarée, pour `claude` seulement ;
> `pdo wait-user` reste le substrat primaire et le seul pour les autres harnais. Vocabulaire :
> CONTEXT.md « Attente déclarée », « Réponse ».

## Contexte

L'attente déclarée (ADR-0069) repose sur la mémoire de l'agent : il doit lancer `pdo wait-user`
quand il rend la main. Au premier test du panneau « Reply » sur un pipeline de planification, un
agent `claude` a posé trois questions dans la conversation puis a fini son tour **sans** déclarer
l'attente : le Run est resté `running`, sans bannière, alors que la main était à l'humain. Le
préambule le demandait ; rien ne le garantissait. Les utilisateurs visés par « Reply » (PO, design,
archi, managers) ne lisent pas le terminal : pour eux, une attente non signalée est invisible.

## Décisions

**1. Sur un nœud `interactive: true` lancé sous `claude`, chaque fin de tour déclare l'attente.**
Le `Stop` hook injecté (le même mécanisme que l'auto-complétion, ADR-0043) lance
`pdo wait-user --turn-end`, cause **`turn_ended`**. Dans un nœud interactif, un agent qui rend la
main attend son humain par construction : ce n'est pas une inférence d'un silence (ce
qu'ADR-0069 écarte), c'est l'événement de fin de tour du harnais. Le hook est armé quel que soit
le réglage `autocomplete_turn_end` ; quand ce réglage est actif aussi, une seule commande enchaîne
`pdo complete --auto || pdo wait-user --turn-end` (Claude exécute les hooks d'un même événement en
parallèle : l'ordre vit dans le shell).

**2. `turn_ended` ne remplace jamais une attente déjà déclarée.** La question de l'agent
(`declared`) ou la complétion refusée (`completion_not_released`) disent plus que « son tour est
fini » : la déclaration `turn_ended` sur un nœud déjà en attente est un `noop`. Elle se lève comme
les autres (Entrée au pont PTY, « Reply », libération de la complétion) et compte comme attente
dans le mode de durée Waiting.

**3. `pdo wait-user` peut proposer les réponses.** `--option "<choix>"` (répétable, 8 au plus) et
`--multi` portent des choix que le panneau affiche en boutons ; un clic envoie le libellé comme
« Reply ». Le substrat reste la CLI, valable pour tous les harnais.

**4. L'outil de question de `claude` (`AskUserQuestion`) se répond depuis le panneau.** Au second
test, l'agent n'a ni écrit ses questions ni utilisé `--option` : il a pris son propre outil, un menu
au clavier dans le terminal, illisible pour qui ne pilote pas un TUI. Sur un nœud interactif, un
hook `PreToolUse` sur cet outil lance `pdo ask-user` : il déclare l'attente (la première question
en bannière), garde la question ouverte par long-poll et rend au tool les réponses données dans la
vue Conversation (`updatedInput.answers`) ; l'outil s'exécute alors sans son menu. Bloquer le tour
ne coûte rien : l'agent attend déjà la réponse. Délai du hook : 24 h ; à son expiration, ou à tout
échec du hook (pas d'identité de nœud, entrée illisible), le menu de Claude s'affiche dans le
terminal comme avant. Les réponses en attente vivent en mémoire du daemon ; après un redémarrage le
hook continue de sonder et la réponse suivante l'atteint.

Écartés : **le hook comme substrat unique de l'attente** (capacité `claude` seulement, ADR-0069
l'avait déjà écarté) ; **retirer `AskUserQuestion` aux nœuds interactifs** (robuste, mais perd la
carte de questions que l'agent produit de lui-même) ; **piloter le menu au clavier** (`send-keys`,
fragile à chaque version du TUI) ; **réécrire `turn_ended` sur une attente existante** (perdrait la
question de l'agent).

## Conséquences

- Un nœud interactif `claude` passe orange dès qu'il rend la main, même si l'agent oublie
  `pdo wait-user` ; les autres harnais gardent le comportement d'ADR-0069.
- Un nœud interactif `claude` porte toujours un fichier de settings à côté de son prompt (le hook
  de fin de tour et celui de l'outil de question).
- Un développeur sur l'onglet Terminal ne voit pas le menu de Claude tant que la question attend le
  panneau : il y répond dans la vue Conversation.
- Le skill seedé `pdo-interactive` passe en version 3 (les options) et se réécrit sur chaque
  instance.
