# ADR-0052 — Un coût rapporté se convertit par une constante et ne passe pas par la table de prix

Sans cet ADR, un agent ferait passer le coût rapporté par un harnais tiers par la table de prix
d'ADR-0034, ou le recalculerait depuis ses tokens — et double-compterait silencieusement tout le
cache.

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Coût dérivé / coût rapporté ». **Amende ADR-0022** (le coût n'est plus
> seulement dérivé de tokens) et **ADR-0034** (la table de prix cesse d'être le passage obligé).

## Contexte

**Les buckets de cache ne se mappent pas.** Le fold de coût de Claude Code somme quatre buckets à
multiplicateurs distincts (création 5 min à 1,25×, création 1 h à 2×, lecture à 0,1×, plus l'input
nu). `copilot` n'en expose que deux, lecture et écriture, sans multiplicateur publié par bucket.

**Et surtout, le total d'input n'a pas le même sens.** Mesuré sur une session réelle : `inputTokens`
valait 26 513, exactement la somme de l'input nu (2), du cache lu (16 559) et du cache écrit (9 952).
C'est un **total incluant le cache**, là où Claude Code rapporte un input *hors* cache. Appliquer la
formule à cinq termes à ces champs double-compte donc tout le cache, silencieusement, et d'autant
plus que la session est longue.

**La table de prix, elle, est structurellement Anthropic.** Elle ne lit que le fournisseur
`anthropic` de sa source, ne retient que les identifiants commençant par `claude-`, et échoue si
cette récolte est vide.

Enfin, l'unité de facturation a changé sous nos pieds : `copilot` compte en **crédits IA**, un crédit
valant un centime de dollar, et la *premium request* n'est plus que le tier historique.

## Ce qu'on décide

### 1. Deux formes légitimes de coût, jamais mélangées

Un coût **dérivé** est recalculé par PDO depuis des tokens et la table de prix résolue : c'est le
chemin de `claude`, inchangé. Un coût **rapporté** est celui que le harnais a compté lui-même, dans
son unité de facturation.

### 2. Un coût rapporté se convertit par une constante publiée, sans table de prix

La conversion vers le dollar est une **constante documentée par le fournisseur**, pas une estimation
: elle ne dégrade donc pas l'honnêteté du chiffre, et elle rend les deux formes additionnables. Un
coût rapporté ne consulte aucune table — il ne peut donc pas produire de `unpriced_models`, ni faire
grossir une table Anthropic avec des familles qui n'en relèvent pas.

### 3. Un total de Run est ventilé par harnais

Le total reste sommable en dollars, mais il se **dit** par harnais : « X via `copilot`, Y via
`claude` ». C'est ce qui rend un Run mixte lisible au lieu de l'annuler.

**Un total indisponible n'efface pas la ventilation.** Quand un nœud tourne sur un harnais sans
source de coût (ADR-0045), c'est la **somme** qui est refusée, pas la connaissance : les tranches que
PDO sait calculer se disent quand même, sous le « — » et sa raison. La première implémentation
court-circuitait avant de les calculer, rendant la ventilation invisible précisément dans le Run qui
mélange trois harnais (FP #617). Une tranche n'est pas une fraction de total : elle vaut par
elle-même, avec sa forme.

### 4. Pas de conversion de devise

Le chiffre reste en dollars. Convertir demanderait un taux de change, donc une source réseau sur un
chemin de lecture qu'ADR-0034 a délibérément gardé local.

## Les alternatives écartées

**Recalculer le coût rapporté depuis ses tokens.** L'option qui semblait la plus propre avant mesure,
et que la mesure a tuée : le double-comptage du cache décrit plus haut. On aurait obtenu un chiffre
plus faux que celui du harnais, en s'appuyant sur sa propre télémétrie pour le produire.

**Ne rendre que l'unité native** (crédits, requêtes) sans conversion, en refusant tout total de Run
mixte. Position initiale du grilling, défendable : elle refuse de fabriquer des dollars pour un nœud
sous abonnement, où le coût marginal réel est nul jusqu'à épuisement du quota. Écartée parce que la
conversion est une constante publiée : le prix payé n'achetait plus rien.

**Étendre la table de prix aux modèles non-Anthropic.** Recréerait par la table le double-comptage
qu'on vient d'écarter par la formule, et ferait de PDO le mainteneur d'un catalogue multi-fournisseurs
que sa source ne sert pas.

## Limites acceptées

- **Un coût rapporté vaut ce que vaut la télémétrie du harnais.** PDO cesse d'être l'arbitre du
  chiffre pour ce harnais ; une erreur de comptage du fournisseur devient invisible.
- **L'unité de facturation d'un harnais peut changer**, et vient de le faire. La constante de
  conversion est une donnée à surveiller, au même titre que la table de prix.
- **La précision des deux moitiés d'un Run mixte diffère** sans que le total le dise autrement que
  par sa ventilation.

## Antériorité

ADR-0022, ADR-0034, ADR-0045, ADR-0029 (les agrégats sont dérivés, jamais matérialisés), #425
(`unpriced_models`).
