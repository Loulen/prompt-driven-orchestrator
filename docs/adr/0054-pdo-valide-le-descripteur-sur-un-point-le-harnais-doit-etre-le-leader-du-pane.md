# ADR-0054 — PDO valide le descripteur sur un point : le harnais doit être le leader du pane

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Harnais agentique ». **Amende ADR-0045** : « PDO ne valide pas un
> descripteur » gagne une exception, unique et nommée. **Protège ADR-0032** : sans elle, le seul
> verdict terminal de liveness devient silencieusement faux.

## Contexte

ADR-0032 a supprimé tout seuil d'idle et fait de la mort de session le **seul** verdict terminal de
liveness. Sa justification est une propriété de construction : le process de l'agent est le leader du
pane, donc il sort et la session meurt. Cette exactitude n'est pas un choix d'implémentation du
détecteur, c'est ce qui rend le détecteur correct.

Or cette propriété n'est pas tenue par le détecteur. Elle est tenue par la **forme du template
d'argv** : `exec <binaire> …`. C'est `exec` qui remplace le shell du pane par le harnais, et fait
donc du harnais le leader.

Depuis ADR-0045, ce template est de la **donnée écrite par l'utilisateur**. Un descripteur dont le
lancement omet `exec`, ou qui enveloppe le binaire dans un shell, laisse le shell leader du pane. Le
harnais peut alors sortir — plantage, erreur dure de provider, flag inconnu qui fait afficher l'aide
et quitter — **sans que la session meure**. Le nœud reste `Running` indéfiniment, vivant et muet, et
aucun filet ne le rattrape : il n'y a plus de seuil d'idle (#469), la liveness voit une session bien
vivante, et le détecteur de stall voit un nœud qui tourne.

ADR-0045 avait refusé toute validation, en assumant une conséquence de cette famille : un descripteur
sans flag d'autonomie donne un nœud arrêté sur un dialogue de permission, que rien ne détecte. Le
grilling a mesuré que les deux cas ne sont pas de la même gravité. Le nœud arrêté sur un dialogue est
**récupérable par un humain qui s'attache** : la session est là, l'agent attend, l'utilisateur
répond. Le nœud dont le harnais est mort derrière un shell survivant n'est récupérable par personne,
parce que **rien ne dira jamais qu'il est mort** — et le Run reste bloqué derrière lui.

La première conséquence dégrade un nœud. La seconde retire à PDO son unique verdict terminal.

## Ce qu'on décide

**Un descripteur dont le template de lancement ne fait pas du binaire déclaré le leader du pane est
refusé au chargement**, avec un diagnostic qui le nomme, et le tier suivant reprend la main —
exactement la forme de refus déjà en place pour une ligne sans `binary` ou sans `launch`.

C'est la **seule** validation que PDO fait d'un descripteur. Elle ne porte pas sur le sens des
arguments, ni sur l'existence des flags, ni sur l'autonomie, ni sur la résidence. Elle porte sur la
précondition d'un invariant que PDO doit tenir pour tout le monde.

## Le contre-argument, et pourquoi on passe outre

ADR-0001 (*sharp tool*) et ADR-0045 disent la même chose : PDO ne se met pas entre l'utilisateur et
son outil, et une validation par cas est le premier pas vers le mini-langage de descripteur
qu'ADR-0045 a explicitement refusé. Le risque est réel et il faut le nommer : **une exception invite
la deuxième**, et la deuxième sera « valider qu'il y a un flag d'autonomie », qui est le cas voisin
et qui, lui, doit rester non validé.

Ce qui distingue celle-ci, et qui doit servir de test à toute demande future d'exception : elle ne
protège pas l'utilisateur de son propre descripteur, elle protège **un invariant que PDO publie**.
Un flag d'autonomie manquant ne casse rien chez PDO — il produit un nœud en attente, état que PDO
sait représenter. Un leader de pane qui n'est pas le harnais rend fausse une affirmation que PDO fait
partout ailleurs, y compris dans sa documentation et dans ses verdicts d'échec.

Formulé comme règle : PDO ne valide un descripteur que là où le descripteur peut rendre PDO menteur.

## Les alternatives écartées

**Ne rien valider et documenter le piège.** La posture d'ADR-0045, appliquée telle quelle. Écartée
parce que le piège est indétectable après coup : l'utilisateur qui l'a posé n'observe pas un message
d'erreur, il observe un Run qui ne finit jamais, et le diagnostic demande de comprendre la relation
entre `exec`, le leader du pane et le verdict de liveness. Le coût du silence est ici très supérieur
au coût de la garde.

**Observer à l'exécution plutôt que parser** : comparer, après le spawn, le process leader du pane au
binaire déclaré. C'est l'invariant *lui-même* plutôt qu'un proxy syntaxique, donc strictement plus
juste. Écartée comme mécanisme principal pour deux raisons : elle constate après que `NodeStarted`
est durable, là où ADR-0037 veut l'échec avant tout effet de bord ; et elle ajoute une sonde par
spawn pour un défaut qui est une propriété statique du descripteur. Elle reste le repli naturel si le
contrôle statique se révèle trop grossier.

**Valider le template entièrement** — flags connus, arité, autonomie. Refusée par ADR-0045, et la
refuser encore ici : PDO ne connaît pas les flags des harnais, cette connaissance périme à chaque
release (ADR-0053 le mesure), et une validation fausse est pire qu'aucune.

## Limites acceptées

- **Le contrôle est syntaxique, donc c'est un proxy.** Il vérifie que le lancement commence par
  `exec` suivi du binaire déclaré. Un template qui passe le contrôle et casse quand même l'invariant
  reste concevable ; c'est le cas où l'observation à l'exécution prendrait le relais.
- **Un descripteur légitime peut être refusé.** Quelqu'un qui a une raison d'envelopper son harnais
  perd la possibilité de le déclarer. C'est assumé : cette forme rend le verdict de mort faux, donc
  elle n'est pas supportable, même volontairement.
- **La résidence reste non vérifiée.** ADR-0045 en fait le critère d'éligibilité d'un harnais et rien
  dans le code ne le contrôle. Cette ADR ne change pas cela : un harnais non résident sort en fin de
  travail, et cette sortie **est** une mort de session, donc l'invariant tient — le nœud échoue à
  tort, mais il échoue visiblement.

## Antériorité

ADR-0032 (la mort de session est le seul verdict terminal, exacte par construction), ADR-0045 (le
descripteur est de la donnée, PDO ne le valide pas ; les limites acceptées y décrivent le cas
voisin), ADR-0037 (l'échec avant tout effet de bord), ADR-0001 (sharp tool), ADR-0053 (PDO ne peut
pas connaître les flags d'un harnais), #469 (la suppression du filet de staleness, qui retire le
dernier recours).
