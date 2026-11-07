# Les alertes d'attente sont émises par le navigateur, opt-in par navigateur, et un seul onglet émet

> Statut : accepted (grilling pdo-issues#1). Vocabulaire : CONTEXT.md § « Alertes d'attente ». Remplace la ligne « Pas de notifications système v1 » de CONTEXT.md, et s'appuie sur ADR-0069 (l'attente est déclarée, jamais inférée).

## Contexte

Quand un nœud passe en attente humaine, seule la pastille orange le signale : l'utilisateur qui n'a
pas l'onglet PDO sous les yeux perd du temps à aller vérifier. Le glossaire avait repoussé les
notifications système à « opt-in plus tard ». Ce plus tard est arrivé.

## Décisions

**1. L'alerte est calculée et émise côté navigateur, à partir du flux d'événements live.** La
WebSocket porte déjà tous les événements de tous les Runs. Le navigateur réagit aux transitions vers
une attente humaine (attente déclarée, complétion non libérée, parking d'incident) avec une
notification (Notification API) et/ou un son synthétisé (WebAudio). Le daemon n'apprend rien de
nouveau.

Écartée : **une notification OS émise par le daemon** (`notify-send`, osascript…). Elle ne vaut que
pour la machine du daemon, et PDO tourne couramment sur une VM distante consultée depuis un autre
poste. Elle demande une dépendance par OS. Surtout, elle ignore si un onglet PDO est sous les yeux,
alors que c'est la condition même de l'alerte.

**2. Le réglage est une préférence du navigateur, et il est opt-in.** Deux bascules indépendantes,
notification et son, désactivées par défaut et stockées dans le navigateur, pas dans `PUT /settings`.
La permission de notification est elle-même accordée par navigateur, et un son n'a de sens que sur la
machine qui l'émet. Une instance partagée ne doit pas sonner chez tout le monde. Conséquence
acceptée : hors contexte sécurisé (accès par IP LAN en http), la notification est indisponible et le
réglage le dit.

**3. On n'alerte que sur une transition live, et seulement quand aucun onglet PDO n'est visible.**
Une attente déjà en cours au chargement, ou rattrapée après une reconnexion, n'alerte pas : un
rechargement ne doit pas déclencher une avalanche. Un onglet visible voit déjà la pastille.
`children_pending` n'est pas une attente humaine, et l'attente dérivée d'un parent (ADR-0069 §4)
n'émet pas d'événement : l'enfant alerte pour lui-même, puisque tous les Runs sont écoutés.

**4. Un seul onglet émet.** Chaque onglet ouvre sa propre WebSocket : sans coordination, N onglets
donneraient N sons. Les onglets se coordonnent localement, par un canal entre onglets du même
navigateur. La notification est en plus étiquetée par `(run, node, iter)`, ce qui permet au
navigateur de remplacer les doublons. Le son est limité à un par fenêtre de quelques secondes, pour
qu'une rafale ne devienne pas une alarme.
