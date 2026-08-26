# ADR-0056 — Le catalogue se lit d'abord dans la source *générée* du binaire, pas dans sa prose

> Statut : accepted (2026-08-26, sous-ticket #629 de la spec « copilot, deuxième harnais first-party »).
> **Amende ADR-0053 §1** : le catalogue reste déduit du binaire installé ; ce qu'on décide ici, c'est
> *où* on le lit dans ce binaire, et dans quel ordre.

## Contexte

ADR-0053 pose que le catalogue offert est lu du binaire. #616 a implémenté cette lecture sur **une**
source : `<binaire> --help`. Le parseur y cherche les énumérations qu'un CLI imprime à côté de
`--model` / `--effort`.

Mesuré sur `GitHub Copilot CLI 1.0.80` :

- `copilot --help` **énumère les sept niveaux d'effort**, mais décrit `--model` en prose
  (« Set the AI model to use (use 'auto' to let Copilot pick automatically) ») — **aucun identifiant**.
- Le lecteur de #616 n'a donc rien vu sur l'axe modèle, et copilot a été servi comme un harnais
  « sans catalogue » — le champ texte libre — alors qu'un catalogue existe. C'est #629.
- `copilot help config` imprime, sans réseau ni auth, **27 identifiants de modèle** en liste à puces
  sous la clé de réglage `model`.
- `copilot completion bash` imprime un `case` bash contenant `compgen -W '<les mêmes 27 + auto>'`
  pour `--model`, et la même chose pour `--effort`.
- Il n'existe **aucune commande dédiée** : `copilot models`, `copilot model list`,
  `copilot --list-models` échouent toutes ; `copilot version --json` n'existe pas. Il n'y a pas de
  sortie structurée (JSON/YAML) du catalogue.

La leçon est générale, pas propre à copilot : **un binaire n'énumère pas forcément là où on regarde**,
et l'endroit change d'une release à l'autre. Une seule source, c'est un point de cécité.

## Ce qu'on décide

### 1. Trois sources, dans un ordre de préférence

Le catalogue peut venir de trois endroits, par préférence décroissante :

1. **`<binaire> completion bash`** — le script de complétion **généré**.
2. **`<binaire> help config`** — le sujet d'aide des réglages, où un CLI documente chaque réglage et
   met ses valeurs autorisées en liste à puces.
3. **`<binaire> --help`** — le lecteur de #616.

### 1 bis. On n'exécute qu'une sous-commande que le binaire **déclare**

`--help` est lancé **en premier, toujours** : il est universel, il est l'une des trois sources, et —
la raison qui le fait passer devant — c'est là qu'un CLI **déclare ses sous-commandes**. Seules les
sources qu'il annonce sont ensuite exécutées.

Ce garde-fou n'est pas de la politesse, c'est la correction de toute l'échelle. Mesuré : `claude`
n'a ni sous-commande `completion` ni sous-commande `help`, et lit l'une ou l'autre comme un
**prompt** — `claude completion bash` ouvre une session qui reste là jusqu'à ce que le timeout du
sondage la tue. Parcourir l'échelle à l'aveugle dépenserait deux timeouts de cinq secondes à chaque
re-sondage de claude, dont un **dans une réponse `/settings`**.

L'ordre d'exécution est donc indépendant de l'ordre de préférence : `--help` tourne en premier, et sa
réponse à lui se plie en dernier.

Coût mesuré sur les trois harnais first-party : `claude` **un** sous-process (comme avant #629),
`copilot` **deux**, `opencode` **deux**. Jamais un blocage, jamais une erreur.

### 2. La source générée est préférée à la prose

`completion bash` passe en premier parce que c'est la seule des trois qui **existe pour être lue par
un programme** : elle est engendrée à partir des choix que le CLI déclare lui-même, pas rédigée pour
un humain. C'est la réponse à « préférer une sortie machine si le binaire en expose une » — sur
copilot 1.0.80, il n'y a pas de sortie structurée du catalogue, mais il y a cette source générée, et
elle vaut mieux qu'un parsing de prose. Bénéfice mesuré au passage : elle porte `auto`, le sélecteur
automatique de copilot, que le sujet d'aide ne liste pas.

> Correctif à une affirmation de #616 reprise dans #629 : « la complétion bash de `--model`
> n'énumère pas les noms » est **fausse** sur 1.0.80 — elle les énumère tous.

### 3. Chaque axe appartient à la première source qui répond pour lui

Le pliage se fait **par axe**, pas par catalogue entier : les sources ne couvrent pas les mêmes
choses. Sur copilot 1.0.80 le script de complétion porte les deux axes ; sur une release qui ne
générerait plus de choix, les modèles viendraient de `help config` et les efforts de `--help`. Un axe
déjà rempli n'est jamais écrasé par une source suivante, et le parcours s'arrête dès que les deux
axes sont remplis — le cas courant ne dépense donc qu'un sous-process.

### 4. Rien d'autre ne change

Le catalogue reste une **offre, jamais une garde** (ADR-0001, ADR-0053 §4) : un identifiant
hors catalogue part verbatim. La fraîcheur reste la version du binaire (ADR-0053 §3). Un binaire qui
n'énumère nulle part retombe sur le texte libre — une **absence déclarée**, pas un défaut.

## Les alternatives écartées

**Garder `--help` seul et écrire la liste de copilot en dur.** C'est exactement ce qu'ADR-0053 refuse,
et la mesure qui l'a motivé (18 → 28 identifiants en six mois) vient de ce harnais-là.

**Une source par harnais, choisie par nom.** Le lecteur redeviendrait une carte à maintenir, et un
harnais déclaré sur disque n'aurait pas d'entrée. L'échelle est harnais-agnostique par construction :
le binaire dit lui-même, dans son `--help`, quelles sources il a.

**Essayer les trois sources sur tout binaire, sans garde-fou.** C'est la première version de ce
ticket, et elle est fausse : mesurée sur `claude`, elle ajoute dix secondes de timeout par
re-sondage, parce qu'un CLI sans sous-commande `completion` ne la *refuse* pas forcément — il la lit
comme un prompt.

**Exécuter le script de complétion pour l'interroger.** On lit du bash généré ; on ne l'exécute pas.
Le sonder comme du texte est sans risque, l'exécuter ne l'est pas.

**Faire de `help config` la source préférée** (c'est celle que #629 nomme). Écartée : la prose de
l'aide est réécrite plus souvent qu'un générateur de complétion, et elle ne porte pas `auto`.

## Limites acceptées

- **Aucune des trois n'est un contrat.** ADR-0053 §Limites tient mot pour mot : prose et bash générés,
  parseurs best-effort, cécité possible à une release — et l'échappatoire texte libre reste le chemin
  qui ne peut pas casser.
- **Jusqu'à trois sous-process** au premier sondage d'un harnais qui déclare les deux sous-commandes
  et n'énumère nulle part (au lieu d'un). Bornés par le même timeout, hors du chemin résident.
- **La déclaration d'une sous-commande se lit dans de la prose, elle aussi.** Le test est volontaire-
  ment lâche (« le premier ou le deuxième mot de la ligne est le nom ») : un faux positif coûte un
  sondage borné qui ne trouve rien, un faux négatif coûte le catalogue. Les deux formes mesurées sont
  couvertes — `  completion <shell>` (copilot) et `  opencode completion` (opencode).

## Antériorité

ADR-0053 (le catalogue est déduit du binaire installé — amendé ici sur le *où*), ADR-0001 (sharp
tool, pas d'enum fermé sur la valeur), ADR-0045 (le mode d'échec d'un modèle appartient au harnais),
#616 (la lecture `--help`, et la supposition « copilot n'a plus de catalogue » que #629 corrige),
#629 (ce ticket).
