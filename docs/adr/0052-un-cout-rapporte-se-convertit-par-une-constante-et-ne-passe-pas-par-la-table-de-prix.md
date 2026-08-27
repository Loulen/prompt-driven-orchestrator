# ADR-0052 — Un coût rapporté se convertit par une constante et ne passe pas par la table de prix

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Coût dérivé / coût rapporté ». **Amende ADR-0022** : le coût n'est
> plus seulement dérivé de tokens ; la forme *rapportée* devient un chemin de premier rang.
> **Amende ADR-0034** : la table de prix résolue cesse d'être le passage obligé de tout coût.

## Contexte

ADR-0022 a fait du coût une **estimation dérivée à la lecture** : tokens lus dans les transcripts
locaux, multipliés par une table de prix (ADR-0034). ADR-0045 avait déjà entrevu qu'un harnais peut
compter lui-même son coût, sans trancher la forme que prendrait ce chiffre dans PDO.

Un second harnais instrumenté force la décision, et les mesures faites pour cette spec disent que
le chemin dérivé **ne peut pas** être réutilisé tel quel.

**Les buckets de cache ne se mappent pas.** Le fold de coût de Claude Code somme quatre buckets à
multiplicateurs distincts (création 5 min à 1,25×, création 1 h à 2×, lecture à 0,1×, plus l'input
nu). `copilot` n'en expose que deux, lecture et écriture, sans multiplicateur publié par bucket.

**Et surtout, le total d'input n'a pas le même sens.** Mesuré sur une session réelle : `inputTokens`
valait 26 513, exactement la somme de l'input nu (2), du cache lu (16 559) et du cache écrit
(9 952). C'est un **total incluant le cache**, là où Claude Code rapporte un input *hors* cache.
Appliquer la formule à cinq termes à ces champs double-compte donc tout le cache, silencieusement,
et d'autant plus que la session est longue — le cas exact que la formule existe pour tarifer
correctement.

**La table de prix, elle, est structurellement Anthropic.** Elle ne lit que le fournisseur
`anthropic` de sa source, ne retient que les identifiants commençant par `claude-`, et échoue si
cette récolte est vide. Un modèle servi par un autre harnais n'y a pas de place, sauf à être saisi à
la main dans le tier manuel — une par machine, à maintenir à chaque sortie de modèle.

Enfin, l'unité de facturation a changé sous nos pieds : `copilot` compte en **crédits IA**, un
crédit valant un centime de dollar, et la *premium request* n'est plus que le tier historique. Un
chiffre lu il y a six mois dans ce produit ne veut plus dire la même chose aujourd'hui.

## Ce qu'on décide

### 1. Deux formes légitimes de coût, jamais mélangées

Un coût **dérivé** est recalculé par PDO depuis des tokens et la table de prix résolue : c'est le
chemin de `claude`, inchangé. Un coût **rapporté** est celui que le harnais a compté lui-même, dans
son unité de facturation.

### 2. Un coût rapporté se convertit par une constante publiée, sans table de prix

La conversion vers le dollar est une **constante documentée par le fournisseur**, pas une
estimation : elle ne dégrade donc pas l'honnêteté du chiffre, et elle rend les deux formes
additionnables. Un coût rapporté ne consulte ni la table embarquée, ni la fetchée, ni la manuelle —
il ne peut donc pas produire de `unpriced_models`, et il ne fait pas grossir une table Anthropic
avec des familles qui n'en relèvent pas.

### 3. Un total de Run est ventilé par harnais

Le total reste sommable en dollars, mais il se **dit** par harnais : « X via `copilot`, Y via
`claude` ». C'est ce qui rend un Run mixte lisible au lieu de le rendre incalculable, tout en
gardant visible que les deux moitiés n'ont pas la même nature ni la même précision.

**Un total indisponible n'efface pas la ventilation.** Quand un nœud tourne sur un harnais sans
source de coût (ADR-0045/#553), c'est la **somme** qui est refusée, pas la connaissance : les
tranches que PDO sait calculer se disent quand même, sous le « — » et sa raison. La première
implémentation court-circuitait avant de les calculer, ce qui rendait la ventilation invisible
précisément dans le Run qui mélange trois harnais — le seul construit pour l'observer (FP #617). Une
tranche n'est pas une fraction de total : elle vaut par elle-même, avec sa forme.

### 4. Pas de conversion de devise

Le chiffre reste en dollars. Convertir demanderait un taux de change, donc une source réseau sur un
chemin de lecture qu'ADR-0034 a délibérément gardé local et sans dépendance.

## Les alternatives écartées

**Recalculer le coût rapporté depuis ses tokens**, pour homogénéiser les deux chemins. C'est
l'option qui semblait la plus propre avant mesure, et c'est celle que la mesure a tuée : le
double-comptage du cache décrit plus haut. On aurait obtenu un chiffre plus faux que celui du
harnais, en s'appuyant sur sa propre télémétrie pour le produire.

**Ne rendre que l'unité native** (crédits, requêtes) sans conversion, en refusant tout total de Run
mixte. Défendable, et c'était la position initiale du grilling : elle refuse de fabriquer des
dollars pour un nœud sous abonnement, où le coût marginal réel est nul jusqu'à épuisement du quota.
Écartée parce que la conversion est une constante publiée et non une estimation : le prix payé —
perdre le total d'un Run mixte, déjà la conséquence assumée d'ADR-0045 — n'achetait plus rien.

**Étendre la table de prix aux modèles non-Anthropic**, pour que tout passe par le chemin dérivé.
Écartée : elle recréerait par la table le double-comptage qu'on vient d'écarter par la formule, et
elle ferait de PDO le mainteneur d'un catalogue de prix multi-fournisseurs que sa source ne sert
pas.

## Limites acceptées

- **Un coût rapporté vaut ce que vaut la télémétrie du harnais.** PDO cesse d'être l'arbitre du
  chiffre pour ce harnais ; il rapporte. Une erreur de comptage du fournisseur devient invisible.
- **L'unité de facturation d'un harnais peut changer**, et vient de le faire. La constante de
  conversion est donc une donnée à surveiller, au même titre que la table de prix — avec ceci de
  moins grave qu'elle est unique et publiée, là où la table est un catalogue.
- **La précision des deux moitiés d'un Run mixte diffère** sans que le total le dise autrement que
  par sa ventilation.

## Antériorité

ADR-0022 (le coût est une estimation dérivée à la lecture), ADR-0034 (les trois tiers de la table de
prix, et le constat que « sans réseau » n'a jamais été littéral), ADR-0045 (« deux formes légitimes
de coût », et « les buckets ne se mappent pas » comme argument contre un recalcul uniforme),
ADR-0029 (les agrégats sont dérivés, jamais matérialisés), #425 (`unpriced_models`).
