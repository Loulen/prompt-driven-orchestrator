# ADR-0053 — Les catalogues de modèles et d'efforts sont déduits du binaire installé

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Modèle et effort ». **Amende ADR-0046** : le modèle et l'effort
> restent conditionnés par le harnais, mais ce qui est *offert* pour un harnais cesse d'être écrit
> en dur. Referme #560.

## Contexte

Le modèle et l'effort sont du texte libre pass-through, sans enum fermé, précisément pour ne pas
périmer à chaque sortie de modèle (ADR-0001). Cette posture concerne la **valeur transmise**. Elle
ne dit rien de ce que l'interface **propose**, et c'est là que la dette s'est accumulée.

Aujourd'hui les listes proposées sont écrites en dur côté client : les alias d'Anthropic pour les
modèles, les cinq niveaux d'effort de Claude Code, et une carte à deux clés qui dit quel harnais a
un axe d'effort. Trois conséquences mesurées :

- Un nœud dont le harnais résolu n'est pas `claude` se voit **proposer le vocabulaire de `claude`**
  (#560).
- Cette carte a `true` pour défaut, donc un harnais qu'elle ne connaît pas affiche un picker
  d'effort **actif mais inerte** : le daemon dérive la vérité de la forme du descripteur, mais ne la
  publie pas, donc le client ne peut pas la refléter.
- Les défauts de modèle par harnais sont deux champs en dur, envoyés en bloc : un troisième harnais
  n'a pas de champ, et sa valeur stockée est **effacée** par n'importe quelle sauvegarde de réglages.

La mesure qui tranche vient du harnais qu'on ajoute. En six mois, sa liste de modèles est passée de
18 identifiants à 28, avec un renommage quasi total d'une de ses familles ; un modèle que sa version
récente sert couramment était **absent** de l'énumération de la version précédente. Mieux : sa
version récente a **retiré l'énumération de son aide** au profit d'un sélecteur automatique. Et son
binaire est installé par un gestionnaire de paquets qui le met à jour tout seul.

Une liste écrite en dur pour ce harnais serait donc fausse en semaines, sans que rien ne le signale.

## Ce qu'on décide

### 1. Le catalogue offert est déduit du binaire installé

Les modèles et les niveaux d'effort proposés pour un harnais sont lus **du binaire lui-même**. Ce
que PDO propose devient une propriété de ce qui est installé sur la machine, pas une constante du
produit.

### 2. Le daemon le publie ; le client l'affiche

Le catalogue, et le fait qu'un harnais ait ou non un axe d'effort, sont servis par le daemon avec la
liste des harnais. Le client cesse de connaître un catalogue : il rend ce qu'on lui donne. Aucune
liste de modèles ou d'efforts n'est plus écrite en dur, ni côté daemon, ni côté client. Corollaire :
les réglages par harnais se dérivent de la liste servie, ce qui supprime la classe de bug « une
sauvegarde efface la valeur d'un harnais qui n'a pas de champ ».

### 3. Sondé au démarrage, invalidé sur changement de version

Le sondage a lieu au démarrage du daemon, et le résultat est conservé avec la version sondée. Un
changement de version du binaire invalide le catalogue et déclenche un nouveau sondage.

### 4. On déduit ce qu'on offre, on ne valide pas ce qu'on reçoit

La valeur reste du texte libre transmis verbatim. Un modèle absent du catalogue déduit est envoyé
quand même — le mode d'échec appartient au harnais, pas à PDO (ADR-0045). Le catalogue est une
**offre**, jamais une garde.

## Les alternatives écartées

**Garder des listes en dur, enrichies par harnais.** Écartée par la mesure ci-dessus : 18 → 28
identifiants en six mois sur un seul harnais, un modèle courant absent de l'énumération précédente,
et l'énumération elle-même retirée de l'aide. Une liste en dur est une promesse que le produit ne
peut pas tenir, et le mode de défaillance est un designer qui choisit un modèle que son binaire ne
connaît pas.

**Ne rien proposer, laisser le champ en texte libre pour les harnais autres que `claude`.** C'est le
comportement actuel moins le bug #560. Honnête, et gratuit. Écartée parce qu'elle transforme un
picker en champ à recopier : le designer doit aller chercher l'orthographe exacte d'un identifiant
dans l'aide d'un binaire, alors que le daemon peut la lire pour lui.

**Sonder à chaque ouverture du picker.** Toujours juste, y compris après une auto-mise à jour.
Écartée pour son coût : un sous-process par affichage, sur un chemin interactif.

**Ne résoudre le catalogue qu'au spawn, gelé dans l'événement de démarrage.** Correct pour le nœud,
mais l'interface n'a rien à proposer *avant* le spawn, qui est le seul moment où le catalogue sert.

## Limites acceptées

- **La sortie sondée n'est pas un contrat.** L'aide d'un binaire et son script de complétion sont de
  la prose et du bash générés, pas une API. Le parseur est best-effort et peut devenir aveugle à une
  release, sans que le harnais soit cassé pour autant. Conséquence assumée : le catalogue est une
  commodité, l'échappatoire texte libre reste le chemin qui ne peut pas casser.
- **Le catalogue n'est pas l'entitlement.** Le binaire déclare ce qu'il sait adresser, pas ce que le
  compte est servi. Mesuré : un modèle présent dans la liste peut être refusé par le serveur. PDO
  offre le catalogue du binaire, pas une garantie de disponibilité.
- **Deux exécutions de sous-process par harnais installé au démarrage du daemon**, et un
  re-sondage à chaque changement de version détecté.
- **Un harnais dont le binaire n'expose aucun catalogue** retombe sur le texte libre, sans que ce
  soit un défaut : c'est une absence déclarée, comme l'axe d'effort.

## Antériorité

ADR-0046 (le modèle et l'effort n'ont de sens que dans un harnais, d'où la carte par harnais),
ADR-0045 (l'absence d'un trou est un signal exploitable par l'UI ; le mode d'échec d'un modèle
appartient au harnais), ADR-0001 (sharp tool, pas d'enum fermé sur la valeur), ADR-0034 (même
posture de tiers pour la table de prix : rien n'est seedé, la lecture reste locale), #560 (le picker
propose les alias de `claude` sur un nœud `opencode`), #347 (défaut de modèle par harnais).
