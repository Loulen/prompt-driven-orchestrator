# ADR-0054 — PDO valide le descripteur sur un point : le harnais doit être le leader du pane

Sans cet ADR, un agent appliquerait le « PDO ne valide pas un descripteur » d'ADR-0045 sans exception,
et accepterait un template de lancement sans `exec` — ce qui rend silencieusement faux le seul verdict
terminal de liveness.

> Statut : accepted (grilling du 2026-08-25, spec « copilot, deuxième harnais first-party »).
> Vocabulaire : CONTEXT.md § « Harnais agentique ». **Amende ADR-0045**. **Protège ADR-0032**.

## Contexte

ADR-0032 a supprimé tout seuil d'idle et fait de la mort de session le **seul** verdict terminal de
liveness. Sa justification est une propriété de construction : le process de l'agent est le leader du
pane, donc il sort et la session meurt.

Or cette propriété est tenue par la **forme du template d'argv** : `exec <binaire> …`. C'est `exec`
qui remplace le shell du pane par le harnais. Depuis ADR-0045, ce template est de la **donnée écrite
par l'utilisateur**. Un descripteur qui omet `exec` laisse le shell leader du pane : le harnais peut
sortir — plantage, erreur dure de provider, flag inconnu — **sans que la session meure**. Le nœud
reste `Running` indéfiniment, vivant et muet, et aucun filet ne le rattrape (plus de seuil d'idle
depuis #469).

ADR-0045 avait assumé une conséquence de cette famille : un descripteur sans flag d'autonomie donne
un nœud arrêté sur un dialogue de permission. Les deux cas n'ont pas la même gravité. Le nœud arrêté
sur un dialogue est **récupérable par un humain qui s'attache**. Le nœud dont le harnais est mort
derrière un shell survivant n'est récupérable par personne, parce que **rien ne dira jamais qu'il est
mort**. La première conséquence dégrade un nœud ; la seconde retire à PDO son unique verdict
terminal.

## Ce qu'on décide

**Un descripteur dont le template de lancement ne fait pas du binaire déclaré le leader du pane est
refusé au chargement**, avec un diagnostic qui le nomme, et le tier suivant reprend la main —
exactement la forme de refus déjà en place pour une ligne sans `binary` ou sans `launch`.

C'est la **seule** validation que PDO fait d'un descripteur. Elle ne porte ni sur le sens des
arguments, ni sur l'existence des flags, ni sur l'autonomie, ni sur la résidence.

## Le contre-argument, et pourquoi on passe outre

ADR-0001 et ADR-0045 disent la même chose : PDO ne se met pas entre l'utilisateur et son outil, et
une validation par cas est le premier pas vers le mini-langage de descripteur qu'ADR-0045 a refusé.
Le risque est réel : **une exception invite la deuxième**, et la deuxième sera « valider qu'il y a un
flag d'autonomie », qui doit rester non validé.

Ce qui distingue celle-ci, et qui doit servir de test à toute demande future d'exception : elle ne
protège pas l'utilisateur de son propre descripteur, elle protège **un invariant que PDO publie**. Un
flag d'autonomie manquant produit un nœud en attente, état que PDO sait représenter. Un leader de
pane qui n'est pas le harnais rend fausse une affirmation que PDO fait partout ailleurs.

Formulé comme règle : **PDO ne valide un descripteur que là où le descripteur peut rendre PDO
menteur.**

## Les alternatives écartées

**Ne rien valider et documenter le piège.** Écartée parce que le piège est indétectable après coup :
l'utilisateur n'observe pas un message d'erreur, il observe un Run qui ne finit jamais.

**Observer à l'exécution plutôt que parser** : comparer, après le spawn, le process leader du pane au
binaire déclaré — l'invariant *lui-même* plutôt qu'un proxy syntaxique, donc strictement plus juste.
Écartée comme mécanisme principal : elle constate après que `NodeStarted` est durable, là où ADR-0037
veut l'échec avant tout effet de bord ; et elle ajoute une sonde par spawn pour un défaut qui est une
propriété statique. Elle reste le repli si le contrôle statique se révèle trop grossier.

**Valider le template entièrement** — flags connus, arité, autonomie. Refusée par ADR-0045 : PDO ne
connaît pas les flags des harnais, cette connaissance périme à chaque release (ADR-0053), et une
validation fausse est pire qu'aucune.

## Limites acceptées

- **Le contrôle est syntaxique, donc c'est un proxy.** Il vérifie que le lancement commence par
  `exec` suivi du binaire déclaré. Un template qui passe le contrôle et casse quand même l'invariant
  reste concevable.
- **Un descripteur légitime peut être refusé.** Quelqu'un qui a une raison d'envelopper son harnais
  perd la possibilité de le déclarer. Assumé : cette forme rend le verdict de mort faux.
- **La résidence reste non vérifiée.** Un harnais non résident sort en fin de travail, et cette
  sortie **est** une mort de session, donc l'invariant tient — le nœud échoue à tort, mais
  visiblement.

## Antériorité

ADR-0032, ADR-0045, ADR-0037 (l'échec avant tout effet de bord), ADR-0001, ADR-0053, #469.
