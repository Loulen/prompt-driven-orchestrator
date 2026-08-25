# ADR-0051 — Une capacité de harnais est un point de dispatch, pas une garde de présence

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Harnais agentique ». **Amende ADR-0045** : « les capacités sont du
> code écrit harnais par harnais » reste vrai, mais la forme livrée n'était qu'une *garde* — cette
> ADR fait de chaque capacité le point où l'implémentation est **choisie**. Prérequis de tout
> deuxième harnais instrumenté.

## Contexte

ADR-0045 a posé cinq capacités (coût, résolution de transcript, substrat de fin de tour, ancre de
menu de limite, plancher de staging) comme du code par harnais, chacune matérialisée par un
marqueur. La livraison a produit un trait dont toutes les méthodes valent `None` par défaut, une
seule implémentation, et un `match` qui ne connaît qu'un nom.

L'inventaire mené pour cette spec a mesuré la conséquence, qui n'était pas prévue : **quatre des
cinq marqueurs ne sont jamais lus pour leur valeur, seulement pour leur présence.** Aucun appelant
ne fait de `match` sur la variante. Les appelants appellent les fonctions de Claude Code
inconditionnellement — l'encodage du répertoire de travail en nom de dossier, la résolution du
fichier de transcript, le parseur d'état de tour, le parseur de coût et sa dédup, le verbe de
reprise, les deux chaînes de l'ancre de limite. Le marqueur ne décide que *si* on les appelle.

Le mode de défaillance qui en découle est silencieux et coûteux : **ajouter une variante à l'un de
ces énumérés compile et ne change rien.** Un contributeur qui déclare `TranscriptResolution::Foo`
et un `FooProbes` obtient un harnais que PDO croit instrumenté et qui lit les chemins de Claude
Code. Rien ne le dit, ni au compilateur, ni à l'exécution.

C'est exactement l'inverse de ce qu'ADR-0045 cherchait : « une capacité absente se dit plutôt que
de mentir ». Ici une capacité *présente* mentait.

## Ce qu'on décide

### 1. Chaque capacité est le point où l'implémentation est choisie

Pour les cinq capacités, l'appelant obtient le comportement **du harnais résolu** et n'a plus de
chemin vers l'implémentation d'un autre harnais. Les fonctions propres à Claude Code cessent d'être
appelables directement depuis les consommateurs génériques (veille, coût, reprise) : elles
deviennent l'implémentation de `claude`, au même rang que celle de n'importe quel autre nom.

### 2. `None` est une valeur explicite, pas un dispatch manquant

Un harnais qui ne veut pas d'une capacité la déclare absente. La distinction porte : « absente »
et « non branchée » cessent d'être indistinguables. C'est ce qui rend le tableau de support du
README vérifiable, donc générable depuis le code.

### 3. On ouvre les cinq, y compris celles que personne n'implémente

`copilot` n'a besoin que de trois capacités (transcript, fin de tour, coût) et déclare les deux
autres absentes. On ouvre quand même le dispatch des cinq. La raison est le coût du harnais
*suivant* : un trait à moitié dispatché est un piège asymétrique — le contributeur découvre que
deux capacités s'aiguillent et que trois font semblant, et il n'a aucun moyen de savoir lesquelles
sans lire les appelants. L'objectif est qu'ajouter un harnais soit une opération sans surprise.

## L'alternative écartée

**Poser un `match harness` à chaque site d'appel claude-en-dur** (sept sites recensés). Moins cher
à écrire, et strictement suffisant pour `copilot`. Écartée pour deux raisons.

La première est le coût récurrent : chacun de ces sites est repayé, et surtout **re-cherché**, à
chaque nouveau harnais. Rien ne les recense ; on les retrouve en cherchant le mot `claude` dans le
code.

La seconde est plus dirimante : elle disperse la connaissance du harnais **loin de sa déclaration
de capacité**. ADR-0045 a justement refusé un trait fermé à N implémentations pour ne pas faire
remonter les particularités de Claude Code (le sibling `.claude.json`, l'appariement de transcript
par répertoire de travail) dans l'interface commune. Un `match` par site produit le même résultat
en pire : les particularités restent dans les consommateurs, et le descripteur de capacité devient
décoratif.

## Limites acceptées

- **Le refactor touche des consommateurs critiques** — le balayage de liveness et le fold de coût,
  tous deux couverts par des tests qui n'exercent que les mécanismes de `claude`. La couverture ne
  démontrera donc pas l'aiguillage tant qu'un second harnais n'est pas testé de bout en bout.
- **Le plancher de staging reste sandbox-only et propre à `claude`** (ADR-0031). Son dispatch est
  ouvert, mais aucun second harnais ne le remplira : la configuration d'un harnais est un
  *prérequis documenté*, pas du code PDO (CONTEXT.md § « Harnais agentique »).
- **L'ancre de menu de limite reste une sonde informationnelle** : elle alimente une jauge et ne
  déclenche aucune récupération, celle-ci restant une décision humaine (ADR-0012). Un second
  harnais peut légitimement la déclarer absente sans dégrader quoi que ce soit d'actionnable.

## Antériorité

ADR-0045 (les capacités sont du code, l'absence se dit), ADR-0046 (le harnais est un axe à quatre
tiers), ADR-0032 et ADR-0043 (les deux substrats de fin de tour), ADR-0022 (le fold de coût),
ADR-0031 (le plancher de staging), #553 (la livraison des capacités-comme-code), #561 (l'audit
d'`opencode` comme plancher lançable, qui décrivait le symptôme sans nommer la cause).
