# ADR-0051 — Une capacité de harnais est un point de dispatch, pas une garde de présence

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Harnais agentique ». **Amende ADR-0045** : « les capacités sont du
> code écrit harnais par harnais » reste vrai, mais la forme livrée n'était qu'une *garde* — cette
> ADR fait de chaque capacité le point où l'implémentation est **choisie**. Prérequis de tout
> deuxième harnais instrumenté.

## Contexte

Sans cette ADR, un contributeur ajoute une variante à un énuméré de capacité, obtient un harnais que
PDO croit instrumenté, et qui lit en réalité les chemins de Claude Code — ça compile, ça ne dit rien,
ni au compilateur ni à l'exécution.

L'inventaire mené pour cette spec l'a mesuré : **quatre des cinq marqueurs posés par ADR-0045 ne sont
jamais lus pour leur valeur, seulement pour leur présence.** Aucun appelant ne fait de `match` sur la
variante ; tous appellent les fonctions de Claude Code inconditionnellement (encodage du répertoire de
travail, résolution du transcript, parseurs d'état de tour et de coût, verbe de reprise, ancre de
limite). Le marqueur ne décide que *si* on les appelle. C'est l'inverse de ce qu'ADR-0045 cherchait :
une capacité *présente* mentait.

## Ce qu'on décide

1. **Chaque capacité est le point où l'implémentation est choisie.** L'appelant obtient le
   comportement du harnais résolu et n'a plus de chemin vers l'implémentation d'un autre harnais.
   Les fonctions propres à Claude Code deviennent l'implémentation de `claude`, au même rang que
   n'importe quel autre nom.
2. **`None` est une valeur explicite, pas un dispatch manquant.** « Absente » et « non branchée »
   cessent d'être indistinguables — c'est ce qui rend le tableau de support du README générable
   depuis le code.
3. **On ouvre les cinq, y compris celles que personne n'implémente.** `copilot` n'en a besoin que de
   trois. La raison est le coût du harnais *suivant* : un trait à moitié dispatché est un piège
   asymétrique — le contributeur ne peut pas savoir lesquelles s'aiguillent sans lire les appelants.

## L'alternative écartée

**Poser un `match harness` à chaque site d'appel claude-en-dur** (sept sites recensés). Suffisant
pour `copilot`, écartée pour deux raisons : chaque site est repayé et surtout **re-cherché** à chaque
nouveau harnais (rien ne les recense) ; et elle disperse la connaissance du harnais loin de sa
déclaration de capacité — exactement ce qu'ADR-0045 refusait, en pire, le descripteur devenant
décoratif.

## Limites acceptées

- **Le refactor touche des consommateurs critiques** (balayage de liveness, fold de coût) dont les
  tests n'exercent que les mécanismes de `claude` : la couverture ne démontrera pas l'aiguillage tant
  qu'un second harnais n'est pas testé de bout en bout.
- **Le plancher de staging reste sandbox-only et propre à `claude`** (ADR-0031) : son dispatch est
  ouvert, mais la configuration d'un harnais est un *prérequis documenté*, pas du code PDO.
- **L'ancre de menu de limite reste une sonde informationnelle** : elle alimente une jauge et ne
  déclenche aucune récupération (ADR-0012). La déclarer absente ne dégrade rien d'actionnable.

## Antériorité

ADR-0045, ADR-0046, ADR-0032 et ADR-0043 (les deux substrats de fin de tour), ADR-0022 (le fold de
coût), ADR-0031, #553, #561.
