# Un terminal partagé n'a qu'un pilote, qui seul pèse sur la taille

> Statut : accepted (grilling #867). Amende ADR-0005 : le multi-client par session n'est plus
> « gratuit », il a un pilote. Vocabulaire : CONTEXT.md § « Terminal partagé ».

## Contexte

Chaque terminal ouvert dans l'UI est un client `tmux attach` distinct. PDO ne règle pas
`window-size` : tmux applique `latest`, et la fenêtre prend la taille du dernier client actif. Sur
une instance distante ouverte depuis deux navigateurs de tailles différentes, la fenêtre change de
taille à chaque changement d'activité et le terminal clignote chez tout le monde.

## Décisions

**1. Entre deux postes, un seul pilote.** Le pilote est le seul client tmux qui pèse sur la taille
de la fenêtre et le seul dont les frappes passent. Les spectateurs voient tout en temps réel. Le
premier arrivé pilote. La prise de main est un geste explicite, jamais automatique entre deux postes
présents. Quand un seul poste regarde, il n'y a ni rôle ni indicateur : c'est le comportement
d'avant.

**2. La taille passe par le drapeau client `ignore-size` de tmux.** Les clients des spectateurs le
portent. La prise de main le retire au nouveau pilote et le pose sur l'ancien, à chaud
(`refresh-client -f`), sans rattacher personne. Mesuré sur tmux 3.4 avec un client à 120×40 et un à
60×20 : la fenêtre suit exactement le client sans `ignore-size`, dans les deux sens de bascule.

**3. La lecture seule est tenue par le pont, pas par tmux.** Le pont est déjà sur le chemin des
octets. Il ignore les frappes d'un spectateur, et il pousse à chaque poste son rôle sur le socket du
terminal. Écarté : le drapeau `read-only` de tmux. Mesuré sur 3.4, `refresh-client -f '!read-only'`
est ignoré sans erreur, et `switch-client -r` ne sait qu'**inverser** `read-only` et `ignore-size`
ensemble : sur un état mal connu, il inverse dans le mauvais sens.

**4. Le poste est le navigateur.** Il se déclare à l'ouverture du socket avec un identifiant
aléatoire conservé côté client. Deux onglets d'un même navigateur sont le même poste et restent tous
deux pilotes : entre eux, la taille se dispute comme avant, c'est assumé. Écartés : l'adresse IP
(derrière un reverse proxy, tous les postes ont la même) et l'onglet (la friction apparaîtrait pour
un utilisateur seul avec deux onglets).

## Options écartées

- **`window-size manual` et un redimensionnement piloté par le daemon** : plus d'état côté daemon,
  et la taille serait figée même pour un poste seul, ce qui casse le « transparent ».
- **Le spectateur n'envoie simplement pas de resize** : son PTY garde sa taille par défaut et pèse
  quand même sur la fenêtre.

## Conséquences

- Un `tmux attach` direct et « Détacher vers un terminal OS » restent des clients tmux ordinaires,
  hors des rôles : ils pèsent encore sur la taille.
- Comme un spectateur ne peut pas taper, il ne peut pas non plus lever une attente déclarée.
