# ADR-0055 — Le daemon résout les binaires de harnais dans le `PATH` de l'utilisateur

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Harnais agentique ». Touche ADR-0019 (le daemon comme service
> persistant) : le `PATH` de l'unité cesse d'être la référence pour trouver un harnais.

## Contexte

Un harnais est probé sur le `PATH` au spawn, et son absence est un échec immédiat, avant tout effet
de bord (ADR-0037). C'est le bon comportement, mais il repose sur l'hypothèse que le `PATH` du daemon
et celui de l'utilisateur se ressemblent. Mesuré sur la machine de développement, ils ne se
ressemblent pas.

Le daemon, lancé comme service, porte un `PATH` qui contient `~/.local/bin`, un répertoire nvm, et
les chemins système. Le harnais qu'on ajoute est installé par Homebrew, sous un préfixe absent de
cette liste : **le daemon ne le voit pas**, et le spawn échoue en disant qu'un binaire installé et
fonctionnel n'existe pas. Le répertoire nvm présent dans ce `PATH` pointe par ailleurs sur une
version de Node différente de celle que le shell de l'utilisateur utilise — donc même l'entrée
héritée est périmée.

Ce n'est pas un cas isolé : c'est la deuxième fois, après un harnais installé sous un répertoire
utilisateur qui subissait exactement le même sort. La cause est structurelle, pas propre à un
gestionnaire de paquets : **les harnais s'installent avec des outils utilisateur, et un service
n'hérite pas de l'environnement utilisateur.**

Le contournement connu — un symlink vers un répertoire que le daemon voit — a été explicitement
refusé au grilling : c'est une étape manuelle, à répéter par harnais et par machine, que rien
n'indique à celui qui rencontre le message d'erreur.

Une mesure a orienté la solution. Un shell de **login** ne suffit pas : `bash -lc` ne trouve pas le
binaire, parce qu'il ne source que le profil. C'est le shell **interactif** qui le trouve, parce que
c'est là que les gestionnaires de version et de paquets posent leurs ajouts de `PATH`. La bonne
référence n'est donc pas « le shell de login », c'est **le `PATH` tel que l'utilisateur l'a quand il
tape la commande à la main**.

## Ce qu'on décide

**Le daemon résout les binaires de harnais dans le `PATH` de l'utilisateur, obtenu de son shell tel
qu'il l'utilise, et non dans celui dont le service a hérité.**

La correction est faite une fois, au niveau du daemon, pas par harnais : elle vaut donc pour les
harnais embarqués, pour ceux déclarés sur disque, et pour tous les suivants.

## Les alternatives écartées

**Exiger un symlink.** Refusée par la personne qui maintient le produit : le contournement fonctionne
mais il fait porter à l'utilisateur un défaut de PDO, une fois par harnais et par machine, et le
message d'erreur ne l'y conduit pas.

**Mettre un chemin absolu dans le descripteur.** Écartée : elle rend un descripteur non portable
d'une machine à l'autre, et elle ne résout rien pour un harnais **embarqué**, dont le nom de binaire
est compilé.

**Enrichir le `PATH` de l'unité de service à sa génération.** C'est ce que fait déjà la génération
d'unité pour les binaires connus. Écartée par la mesure : le `PATH` ainsi figé était déjà périmé sur
la machine de test (mauvaise version de Node), et surtout un harnais installé **après** la génération
de l'unité reste invisible — ce qui est le cas normal, puisqu'on installe un harnais parce qu'on veut
l'essayer.

**Sonder une liste d'emplacements connus** (préfixe Homebrew, nvm, `~/.local/bin`, répertoires de
gestionnaires divers). Écartée : la liste est ouverte, elle est fausse au prochain gestionnaire de
paquets, et elle demande à PDO de connaître l'écosystème d'installation de chaque plateforme.

## Limites acceptées

- **Le daemon hérite de ce que font les fichiers de configuration du shell**, y compris leur lenteur
  et leurs effets de bord. Le coût est payé au démarrage, pas par spawn.
- **Un changement de `PATH` côté utilisateur n'est pas vu immédiatement** : il faut que le daemon
  re-résolve. Même contrainte que le catalogue de modèles, et même déclencheur possible (ADR-0053).
- **Le `PATH` du daemon devient dépendant de l'utilisateur** qui l'exécute. C'était déjà vrai en
  pratique — le daemon tourne sous cet utilisateur et lit ses répertoires de configuration — mais ça
  devient explicite.
- **Le message d'échec doit changer de sens.** « Binaire introuvable » ne peut plus se lire « non
  installé » : c'est précisément la confusion que cette ADR supprime, et le diagnostic doit dire dans
  quel `PATH` la recherche a eu lieu.

## Antériorité

ADR-0037 (l'échec de spawn avant tout effet de bord, dont le probe de binaire), ADR-0045 (le
descripteur déclare un `binary`, probé sur le `PATH`), ADR-0019 (le daemon comme unité de service
persistante), ADR-0053 (même question de fraîcheur, même déclencheur de re-résolution).
