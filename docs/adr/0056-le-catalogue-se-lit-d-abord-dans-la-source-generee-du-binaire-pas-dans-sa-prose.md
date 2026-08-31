# ADR-0056 — Le catalogue se lit d'abord dans la source *générée* du binaire, pas dans sa prose

Sans cet ADR, un agent lirait le catalogue d'un harnais dans la seule sortie `--help`, et conclurait
qu'un binaire qui y décrit `--model` en prose « n'a pas de catalogue » alors qu'il en publie un
ailleurs.

> Statut : accepted (2026-08-26, sous-ticket #629 de la spec « copilot, deuxième harnais
> first-party »). **Amende ADR-0053 §1** : le catalogue reste déduit du binaire installé ; ce qu'on
> décide ici, c'est *où* on le lit dans ce binaire, et dans quel ordre.

## Contexte

Mesuré sur `GitHub Copilot CLI 1.0.80` : `--help` énumère les sept niveaux d'effort mais décrit
`--model` en prose (aucun identifiant) ; `copilot help config` imprime, sans réseau ni auth, 27
identifiants de modèle ; `copilot completion bash` imprime un `compgen -W` contenant les mêmes 27 +
`auto`, et l'équivalent pour `--effort`. Aucune commande dédiée n'existe (`copilot models`,
`--list-models`, `version --json` échouent), et il n'y a aucune sortie structurée du catalogue.

La leçon est générale : **un binaire n'énumère pas forcément là où on regarde**, et l'endroit change
d'une release à l'autre. Une seule source, c'est un point de cécité.

## Ce qu'on décide

### 1. Trois sources, par préférence décroissante

1. **`<binaire> completion bash`** — le script de complétion **généré**.
2. **`<binaire> help config`** — le sujet d'aide des réglages, valeurs autorisées en liste à puces.
3. **`<binaire> --help`** — le lecteur de #616.

### 1 bis. On n'exécute qu'une sous-commande que le binaire **déclare**

`--help` est lancé **en premier, toujours** : c'est là qu'un CLI déclare ses sous-commandes. Seules
les sources qu'il annonce sont ensuite exécutées.

Ce garde-fou est load-bearing. Mesuré : `claude` n'a ni sous-commande `completion` ni `help`, et lit
l'une ou l'autre comme un **prompt** — `claude completion bash` ouvre une session qui reste là
jusqu'au timeout du sondage. Parcourir l'échelle à l'aveugle dépenserait deux timeouts de cinq
secondes à chaque re-sondage de claude, dont un **dans une réponse `/settings`**.

L'ordre d'exécution est donc indépendant de l'ordre de préférence : `--help` tourne en premier, et sa
réponse se plie en dernier. Coût mesuré : `claude` un sous-process, `copilot` deux, `opencode` deux.

### 2. La source générée est préférée à la prose

`completion bash` passe en premier parce que c'est la seule des trois qui **existe pour être lue par
un programme** : engendrée depuis les choix que le CLI déclare, pas rédigée pour un humain. Bénéfice
mesuré : elle porte `auto`, le sélecteur automatique de copilot, que le sujet d'aide ne liste pas.

> Correctif à #616/#629 : « la complétion bash de `--model` n'énumère pas les noms » est **fausse**
> sur 1.0.80 — elle les énumère tous.

### 3. Chaque axe appartient à la première source qui répond pour lui

Le pliage se fait **par axe**, pas par catalogue entier : les sources ne couvrent pas les mêmes
choses. Un axe déjà rempli n'est jamais écrasé par une source suivante, et le parcours s'arrête dès
que les deux axes sont remplis — le cas courant ne dépense donc qu'un sous-process.

### 4. Rien d'autre ne change

Le catalogue reste une **offre, jamais une garde** (ADR-0001, ADR-0053 §4). La fraîcheur reste la
version du binaire (ADR-0053 §3). Un binaire qui n'énumère nulle part retombe sur le texte libre —
une **absence déclarée**, pas un défaut.

## Les alternatives écartées

**Garder `--help` seul et écrire la liste de copilot en dur.** Ce qu'ADR-0053 refuse, et la mesure
qui l'a motivé (18 → 28 identifiants en six mois) vient de ce harnais-là.

**Une source par harnais, choisie par nom.** Le lecteur redeviendrait une carte à maintenir, et un
harnais déclaré sur disque n'aurait pas d'entrée.

**Essayer les trois sources sur tout binaire, sans garde-fou.** Première version du ticket, fausse :
sur `claude` elle ajoute dix secondes de timeout par re-sondage.

**Exécuter le script de complétion pour l'interroger.** On lit du bash généré ; on ne l'exécute pas.

**Faire de `help config` la source préférée.** Écartée : la prose de l'aide est réécrite plus souvent
qu'un générateur de complétion, et elle ne porte pas `auto`.

## Limites acceptées

- **Aucune des trois n'est un contrat.** ADR-0053 §Limites tient mot pour mot ; l'échappatoire texte
  libre reste le chemin qui ne peut pas casser.
- **Jusqu'à trois sous-process** au premier sondage d'un harnais qui déclare les deux sous-commandes
  et n'énumère nulle part. Bornés par le même timeout, hors du chemin résident.
- **La déclaration d'une sous-commande se lit dans de la prose, elle aussi.** Le test est
  volontairement lâche : un faux positif coûte un sondage borné, un faux négatif coûte le catalogue.
  Les deux formes mesurées sont couvertes — `  completion <shell>` et `  opencode completion`.

## Antériorité

ADR-0053 (amendé ici sur le *où*), ADR-0001 (pas d'enum fermé sur la valeur), ADR-0045 (le mode
d'échec d'un modèle appartient au harnais), #616, #629.
