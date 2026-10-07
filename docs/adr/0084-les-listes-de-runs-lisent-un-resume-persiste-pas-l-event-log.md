# Les listes de Runs lisent un Résumé persisté, pas l'event log

> Statut : accepted (grilling du 2026-10-07, CORE-31). Vocabulaire : CONTEXT.md « Résumé de Run ».

## Contexte

L'état d'un Run est la projection de ses événements (« Source de vérité = event log »). Les
lectures fréquentes rejouaient cette projection à chaque appel : `GET /runs` rechargeait et
rejouait l'event log complet de **chaque** Run de la base, puis demandait une autorisation par Run ;
`GET /runs/{id}/children` faisait de même pour les enfants ; l'autorisation d'attacher un terminal
rejouait l'event log du Run pour connaître ses Projets avant de répondre à l'upgrade.

Mesuré le 2026-10-07 sur une instance de 89 Runs : `GET /runs` 60 à 120 ms (pour 30 Ko de
réponse), `GET /runs/{id}/children` 50 à 110 ms, appelé toutes les 3 s par la vue d'un Run ; et le
front rappelait `GET /runs` à chaque événement du flux, depuis chaque onglet, sans regroupement.
Le coût croît avec le nombre de Runs et la longueur de leurs journaux, pas avec ce qu'on affiche.

## Décision

1. **Un Résumé de Run est persisté** dans la base, à côté de l'event log : ce que les listes et les
   contrôles d'accès lisent d'un Run (statut, nom, pipeline, parent, Projets…).
2. **Il est écrit dans la même transaction que l'événement qui le change.** Il ne peut donc jamais
   être en retard sur l'event log, et rien ne l'invalide. L'event log reste la seule source de
   vérité : le Résumé se reconstruit entièrement en rejouant les événements (au démarrage, après une
   migration).
3. **Les listes de Runs, les enfants d'un Run et l'autorisation d'attacher un terminal lisent le
   Résumé**, sans rejouer d'event log. Le détail d'un Run continue de projeter ses événements.

## Options écartées

- **Un cache en mémoire invalidé par événement.** Un état de plus à garder cohérent, perdu à chaque
  redémarrage, et périmé dès qu'un chemin d'écriture oublie d'invalider. Un pansement sur le coût,
  pas une lecture correcte par construction.
- **Seulement regrouper les rafraîchissements du front.** Utile (et fait aussi), mais chaque appel
  restant coûte toujours un rejeu de toute la base.
