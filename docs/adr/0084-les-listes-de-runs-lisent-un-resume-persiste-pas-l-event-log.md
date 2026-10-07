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

Précisions de l'implémentation (CORE-32) :

- Les **Projets** d'un Run ne sont pas copiés dans le Résumé : il porte le dépôt principal du
  Run, et ses Projets sont ceux de ce dépôt, résolus à la lecture. Les membres d'un Projet
  changent hors de l'event log ; une copie serait périmée au premier changement de membres.
- Le **coût** d'un enfant reste dérivé à la lecture (ADR-0052) : la lecture des enfants ne rejoue
  que le journal de chaque enfant, jamais celui de tous les Runs.
- Une **marque** (version du Résumé, dernier événement couvert) dit si les Résumés sont ceux du
  journal ; sinon (premier démarrage, forme changée, daemon antérieur qui a écrit dans le
  journal) le démarrage les reconstruit tous, avant de servir.
- Des événements écrits dans le journal **pendant que le daemon tourne**, hors de ses appends
  (un outil qui peuple la base d'une instance lancée), laissent la marque en arrière. La lecture
  suivante du Résumé les rattrape : elle réécrit le Résumé des Runs de ces événements, puis avance
  la marque, dans une transaction. Quand rien n'a été manqué, il ne coûte qu'une lecture indexée.

## Options écartées

- **Un cache en mémoire invalidé par événement.** Un état de plus à garder cohérent, perdu à chaque
  redémarrage, et périmé dès qu'un chemin d'écriture oublie d'invalider. Un pansement sur le coût,
  pas une lecture correcte par construction.
- **Seulement regrouper les rafraîchissements du front.** Utile (et fait aussi), mais chaque appel
  restant coûte toujours un rejeu de toute la base.
