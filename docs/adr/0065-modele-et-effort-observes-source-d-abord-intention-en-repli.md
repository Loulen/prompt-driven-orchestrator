# Le modèle et l'effort d'une exécution se lisent dans la source du harnais d'abord, dans l'événement de démarrage en repli ; un modèle est son id verbatim, quel que soit le harnais

> Statut : accepted (grilling du 2026-09-07, #733). Vocabulaire : CONTEXT.md §*Modèle et effort*
> (« observé » / « demandé »). **Amende ADR-0029** : la contribution de coût porte désormais le
> modèle et l'effort de l'exécution, et Stats gagne un axe « par modèle ».
> Amendé par ADR-0077 : une absorption explicite de l'opérateur peut réunir deux ids de modèle.

## Contexte

Stats ne sait comparer les modèles : le modèle et l'effort résolus sont gelés au spawn (ADR-0046)
mais jetés à la frontière de la contribution de coût, et le fold `claude` lit le modèle de chaque
message pour le tarifer puis l'oublie. Vérifié sur les sources réelles le 2026-09-07 : `pi` écrit
le modèle et le fournisseur sur chaque message ainsi qu'un événement de changement de niveau de
réflexion ; `copilot` (1.0.83) écrit le modèle choisi et l'effort de raisonnement à l'ouverture de
session et le modèle sur chaque point d'usage ; `opencode` porte tout cela dans sa base mais n'a
pas de capacité coût (#561). Tout harnais qui a une source de coût dit donc quel modèle a tourné.

## Décision

1. **Source d'abord, intention en repli.** Le modèle d'une exécution est celui que la source du
   harnais rapporte, message par message ; l'effort est celui que la source rapporte quand elle le
   porte (`pi`, `copilot`). L'événement de démarrage ne sert que quand la source est muette
   (`claude` pour l'effort). Chaque valeur dit sa **provenance** — observée ou demandée — et
   l'interface la montre. Un harnais sans source reste « — », jamais un bucket « défaut de X ».
2. **Un modèle est son id verbatim, tous harnais confondus.** Le même id lancé via deux harnais est
   une seule ligne, le harnais restant une colonne. Pas de table d'alias, pas de repli sur la
   famille non datée de la table de prix : un alias épinglé (`sonnet`) et un id observé
   (`claude-sonnet-4-5-20250929`) font deux lignes, et l'écart se voit au lieu d'être deviné. Seule
   une **absorption** posée par l'opérateur les réunit : elle est visible et réversible (ADR-0077). Le
   fournisseur (`openrouter`) n'entre pas dans l'identité.
3. **Une exécution compte dans chaque bucket modèle où elle a coûté.** Le coût se ventile par
   message ; le pic de contexte suit le fichier de session (le sous-agent a le sien). La moyenne
   par exécution d'un bucket reste ainsi cohérente avec son coût.

## Alternatives écartées

- **Épinglé partout** (lire l'événement de démarrage seul) : réfuté par `opencode`, qui retombe en
  silence sur un autre modèle que celui écrit, et par les sous-agents `claude` lancés sur un autre
  modèle que la session : on comparerait des étiquettes, pas des exécutions.
- **Normaliser les ids** (table d'alias, famille datée) : périme à chaque sortie de modèle, même
  raison que le catalogue déduit (ADR-0053) ; et la version exacte est précisément ce qu'on compare.
- **Clé harnais × modèle** : contredit la demande — un modèle est un modèle, le harnais est une
  dimension à part, déjà en colonne.
- **Imputer tout le NodeRun au modèle principal** : simple, mais rend le coût moyen par exécution
  d'un bucket incohérent avec son coût et efface la ventilation que le fold sait déjà faire.
- **« Effort demandé partout » pour une sémantique uniforme** : envisagé, écarté par l'utilisateur :
  la vérité terrain prime, la provenance en infobulle lève l'ambiguïté.

## Conséquences

- La contribution de coût (ADR-0058) porte modèle, effort et leurs provenances ; le memo de Stats
  se réindexe sans matérialisation (ADR-0029 préservé).
- L'effort observé de `pi` et `copilot` rend visible pour la première fois « effort demandé ≠
  effort obtenu » ; l'écart n'est pas un bug, c'est une mesure.
- `opencode` restera absent de l'axe tant que #561 n'existe pas.
