# Un WebSocket ne s'ouvre qu'après une sonde HTTP réussie

> Statut : accepted (grilling du 2026-10-07, CORE-31) ; points 5 et 6 ajoutés au grilling du
> 2026-10-08 (CORE-33, rattrapage de l'interface à jour après une absence). Vocabulaire :
> CONTEXT.md « Ouverture sondée ».

## Contexte

L'interface tient plusieurs WebSockets vers le daemon : le flux d'événements, chaque terminal, et
ceux qu'ouvrent les interfaces des Extensions. Le client du flux d'événements se rouvrait toutes
les 3 s, indéfiniment, que le daemon réponde ou non.

Firefox (et ses dérivés) n'admet **qu'un seul WebSocket en cours de connexion par adresse IP**,
tous ports et tous onglets confondus (`nsWSAdmissionManager`), et retarde chaque nouvel essai vers
un `host:port:path` qui a échoué : 200 à 400 ms, puis ×1,5 par échec, plafonné à 60 s. Un essai
retardé garde sa place en tête de file. Chrome sérialise par `IP:port` et n'a pas ce délai.

Mesuré le 2026-10-07 (profil Firefox 147 avec les logs `nsWebSocket`) : un onglet resté ouvert sur
un **autre daemon, arrêté, du même hôte** rouvrait son flux d'événements toutes les 3 s ; Firefox
retenait chaque essai environ 56 s, et le socket d'un onglet sain vers le daemon vivant est resté
57 s en file (`CONNECTING_QUEUED`) avant d'être envoyé, puis s'est connecté en 0,35 s. Reproduit
sur une instance jetable : 39 ms sans l'onglet mort, 7 s puis 24 s puis ~52 s à mesure que le délai
de l'onglet mort grimpait. Côté serveur, toutes les poignées de main répondaient en moins de 15 ms.
Le client sondait déjà `/actors/me` après une coupure, mais rouvrait le socket même quand la sonde
échouait.

## Décision

1. **Un client n'ouvre, ni ne rouvre, un WebSocket qu'après une requête HTTP réussie vers le même
   daemon.** Tant que la sonde échoue, c'est elle qui se répète, espacée d'un backoff exponentiel
   avec jitter, plafonné ; le socket n'est jamais tenté à l'aveugle. Une requête HTTP ne passe pas
   par la file d'admission des WebSockets : un daemon arrêté ne bloque plus les sockets des autres
   onglets.
2. **Un onglet caché ne sonde pas** (`document.visibilityState`) ; il reprend dès qu'il redevient
   visible. Ses sockets ouverts restent ouverts : un socket établi ne gêne aucune file. La
   visibilité, pas le focus : un tableau de bord affiché sur un second écran doit se rétablir sans
   clic.
3. **Le serveur répond à la fermeture du client** par une trame de fermeture avant de couper : une
   fermeture sans trame (1006) compte comme un échec pour le navigateur.
4. **Une seule implémentation**, dans le front du core, qui l'utilise pour son flux d'événements et
   ses terminaux, et qui l'expose aux interfaces des Extensions par le module partagé
   `@pdo/extension-ui` (ADR-0081). Une Extension n'écrit pas sa propre boucle de reconnexion.
5. **Un socket muet est un socket mort.** Le daemon bat sur chaque socket (flux d'événements et
   terminal) ; un client qui n'a rien reçu pendant le délai de silence ferme le socket et repasse
   par la sonde. Une connexion TCP à moitié ouverte (veille, changement de réseau, proxy qui coupe
   pour inactivité) ne se signale par aucune fermeture : sans ce délai, l'onglet reste figé.
6. **Toute (ré)ouverture relit l'état complet**, de même que le retour sur un onglet caché et le
   `resync` que le daemon envoie à un client qui a perdu des événements : le flux ne rejoue pas ce
   qui s'est passé pendant la coupure.

## Options écartées

- **Multiplexer tous les canaux sur un seul WebSocket par onglet.** N'aurait rien changé : la file
  est commune à tous les onglets et à tous les ports, et c'est l'onglet d'un autre daemon qui la
  bloquait. Le multiplexage ajoutait un protocole de canaux, une autorisation par canal, une panne
  commune à tous les canaux et un seam d'API d'extension.
- **Un simple backoff sur le socket.** Chaque essai reste un échec WebSocket : il nourrit le délai
  du navigateur et garde la tête de file.
- **Accepter l'upgrade puis fermer avec un code applicatif (4401, 4403)** au lieu de refuser en
  `401` / `403`. Changeait le contrat de l'Admission d'une requête et ne couvrait pas un daemon
  arrêté, qui ne répond pas du tout.
- **Désactiver `network.websocket.delay-failed-reconnects`.** Ne corrige que le navigateur de celui
  qui le fait, et pas l'attente du timeout d'ouverture (20 s).
