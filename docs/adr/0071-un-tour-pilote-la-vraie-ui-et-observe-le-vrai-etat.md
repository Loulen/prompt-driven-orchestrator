# Un Tour pilote la vraie UI et observe le vrai état

> Statut : accepted (grilling #816). Vocabulaire : CONTEXT.md § « Tours guidés — onboarding ».

## Contexte

Un nouvel utilisateur arrive devant un canvas vide et rien ne le guide vers un premier Run ni une
première pipeline. Il fallait un mode tutoriel. Trois façons de le faire : des captures ou une vidéo,
un mode démo sur des données factices, ou des tours qui guident l'utilisateur dans l'application
réelle.

## Décisions

**1. Un Tour guide dans la vraie UI et n'avance que sur l'état réel observé.** Le projecteur grise
tout sauf la cible, l'instruction dit quoi faire, et l'étape passe quand l'état attendu est atteint
(modale ouverte, valeur choisie, Run lancé, node terminé) — jamais sur un délai. Écartées : les
captures et vidéos (elles rotent à chaque changement d'UI et n'apprennent pas le geste) ; un mode démo
sur données factices (deux chemins de code à maintenir, et l'utilisateur ne repart avec rien).

**2. Le tour n'agit jamais à la place de l'utilisateur.** Il observe et bloque ; il ne remplit pas
un champ, ne clique pas, ne lance rien. Skip passe à l'étape suivante et laisse la validation normale
du formulaire faire son travail ; si une action requise manque, l'étape échoue proprement et dit
pourquoi. Corollaire du principe « l'outil n'agit pas de sa propre initiative » (ADR-0012) et de ce
qu'un tour doit enseigner : le geste, pas son résultat. Écarté : un Skip qui auto-remplit — l'utilisateur
finit le tour sans avoir appris où se trouvait le champ.

**3. Le daemon ignore la notion de tour.** Il expose des verbes génériques (créer un dépôt git à
partir de paramètres, créer une pipeline à partir d'un document) et c'est le frontend qui compose un
tour avec. Les définitions de tours sont des données déclaratives côté frontend (cible, instruction,
condition d'avancement, skippable). Écarté : un endpoint « démarrer le tutoriel » qui préparerait
dépôt et pipeline en une fois — il couplerait le daemon à un contenu pédagogique qui changera plus
souvent que lui.

## Conséquences

- Le tour dépend d'un harnais configuré pour aller jusqu'au bout du *First run* ; sans lui, l'étape de
  lancement échoue proprement avec la raison du daemon, ce qui est déjà une leçon.
- Les cibles de tour sont des éléments réels de l'UI : un champ dont les options ne sont pas
  ciblables (`<select>` natif) doit être remplacé par le menu maison avant d'entrer dans un tour.
- Le dépôt d'entraînement vit sous `/tmp` et disparaît avec lui ; aucun nettoyage à gérer.
