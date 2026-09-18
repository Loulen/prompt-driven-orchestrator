# La fraîcheur de la branche source est visible au lancement, et PDO ne fait que fetch et fast-forward

> Statut : accepted (grilling #800). Vocabulaire : CONTEXT.md § « Branche source », « Écart amont », « Synchro de la branche source », « Dérive de la source ». Amende #571 (« la fraîcheur appartient à l'opérateur, jamais au daemon ») et précise ADR-0042 (la coupe reste sans fetch).

## Contexte

Le formulaire de lancement proposait la branche source dans un `<select>` nu : ni écart avec la
branche de suivi, ni date, ni fetch. #571 avait tranché « le daemon ne parle jamais au remote,
la fraîcheur est à l'opérateur ». Mesuré à l'usage : on lance des Runs sur un `main` local en retard
de plusieurs commits sans s'en rendre compte, et on le découvre au merge-back. La règle « à
l'opérateur » ne tenait que si l'écart était visible ; il ne l'était pas.

## Décisions

**1. Le daemon parle au remote, mais seulement en lecture et sur intention humaine.** Le formulaire
fetch à l'ouverture et au changement de dépôt ; le bouton de synchro refetch. Un Trigger fetch avant
de couper. Le fetch est non interactif (pas de prompt d'identifiants), borné dans le temps, un seul
en vol par dépôt ; son échec dégrade l'écart en « inconnu, daté du dernier fetch réussi » et ne
bloque jamais un lancement — un dépôt sans remote reste un cas légitime. Écarté : un fetch
périodique en tâche de fond (du réseau sur le dépôt des gens sans qu'ils l'aient demandé) ; un fetch
à l'affichage d'un Run (ouvrir une page ne déclenche pas de réseau).

**2. La seule écriture que PDO se permet sur une branche locale est un fast-forward sans perte,
sur clic explicite.** Arbre du checkout propre, aucune divergence, branche non checkoutée dans un
autre worktree ; sinon refus lisible et proposition de couper sur la branche de suivi. Jamais de
merge, de rebase ni de push ; jamais de fast-forward depuis un Trigger (personne pour arbitrer, et
ADR-0012 : le runtime n'agit pas sur le dépôt de sa propre initiative). Écartées : **ne jamais
toucher** (#571) — le bouton ne servirait à rien dans le cas courant où `main` est checkouté ;
**pull complet** à la VS Code — un merge silencieux dans le checkout de quelqu'un.

**3. La coupe ne change pas.** La branche source reste stockée verbatim et résolue sans fetch au
moment de créer le worktree ; le fetch a eu lieu avant, dans le geste ou dans le Trigger. Le défaut
proposé reste une branche locale même en retard : l'écart s'affiche, il ne réécrit pas le choix.

**4. Le Run porte sa dérive, pas un instantané.** Ce qu'on veut savoir sur un Run vivant, c'est
combien de commits il a produits et combien sont arrivés sur sa source depuis le fork ; c'est une
mesure sur les réfs locales, recalculée à l'affichage. Écarté : figer l'écart amont dans l'événement
de démarrage — il répond à « d'où est-on parti ? », que personne ne pose une fois le Run lancé.

## Conséquences

- Les Runs créés par la CLI (`pdo run create`, enfants d'un agent) gardent la coupe sans fetch :
  leur source est en général la branche du Run parent, locale par construction.
- L'avertissement porté par la branche source d'un Trigger dit la vérité : une locale ne bouge pas
  avec le fetch ; c'est la branche de suivi qui est fraîche au déclenchement.
