# ADR-0055 — Le daemon résout les binaires de harnais dans le `PATH` de l'utilisateur

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Harnais agentique ». Touche ADR-0019 : le `PATH` de l'unité de
> service cesse d'être la référence pour trouver un harnais.

## Contexte

Sans cette ADR, on lit « binaire introuvable » (ADR-0037 : le probe de `PATH` échoue avant tout effet
de bord) comme « harnais non installé », et on va poser un symlink à la main — machine par machine,
harnais par harnais.

Le daemon lancé comme service porte un `PATH` hérité. Un harnais installé par Homebrew, sous un
préfixe absent de cette liste, **est invisible** alors qu'il est installé et fonctionnel ; le
répertoire nvm hérité pointe par ailleurs sur une version de Node différente de celle du shell — même
l'entrée présente est périmée. C'est la deuxième occurrence, après un harnais installé sous un
répertoire utilisateur. La cause est structurelle : **les harnais s'installent avec des outils
utilisateur, et un service n'hérite pas de l'environnement utilisateur.**

Une mesure oriente la solution : un shell de **login** ne suffit pas — `bash -lc` ne trouve pas le
binaire, parce qu'il ne source que le profil. C'est le shell **interactif** qui le trouve, parce que
c'est là que les gestionnaires de version et de paquets posent leurs ajouts de `PATH`.

## Ce qu'on décide

**Le daemon résout les binaires de harnais dans le `PATH` de l'utilisateur, obtenu de son shell tel
qu'il l'utilise, et non dans celui dont le service a hérité.** La correction est faite une fois, au
niveau du daemon, pas par harnais : elle vaut pour les harnais embarqués, ceux déclarés sur disque,
et tous les suivants.

## Les alternatives écartées

**Exiger un symlink.** Refusée par la personne qui maintient le produit : elle fait porter à
l'utilisateur un défaut de PDO, une fois par harnais et par machine, sans que le message d'erreur l'y
conduise.

**Mettre un chemin absolu dans le descripteur.** Rend un descripteur non portable, et ne résout rien
pour un harnais **embarqué**, dont le nom de binaire est compilé.

**Enrichir le `PATH` de l'unité de service à sa génération.** Écartée par la mesure : le `PATH` figé
était déjà périmé sur la machine de test, et surtout un harnais installé **après** la génération de
l'unité reste invisible — le cas normal, puisqu'on installe un harnais pour l'essayer.

**Sonder une liste d'emplacements connus.** La liste est ouverte, fausse au prochain gestionnaire de
paquets, et demande à PDO de connaître l'écosystème d'installation de chaque plateforme.

## Limites acceptées

- **Le daemon hérite de ce que font les fichiers de configuration du shell**, lenteur et effets de
  bord compris. Le coût est payé au démarrage, pas par spawn.
- **Un changement de `PATH` côté utilisateur n'est pas vu immédiatement** : il faut une
  re-résolution. Même contrainte et même déclencheur que le catalogue de modèles (ADR-0053).
- **Le `PATH` du daemon devient dépendant de l'utilisateur** qui l'exécute — déjà vrai en pratique,
  désormais explicite.
- **Le message d'échec doit changer de sens** : il doit dire dans quel `PATH` la recherche a eu lieu,
  puisque « introuvable » ne peut plus se lire « non installé ».

## Antériorité

ADR-0037 (échec de spawn avant tout effet de bord), ADR-0045 (le descripteur déclare un `binary`
probé sur le `PATH`), ADR-0019, ADR-0053.
