# ADR-0044 — Un troisième journal : `audit_log` pour les mutations de config hors-Run

> Statut : accepted (grilling #507). Vocabulaire : CONTEXT.md § « Trigger / Persistence »
> (« trois journaux, trois questions »). Version 1.25.0 (provisoire — renuméroter au rebase ;
> numéro d'ADR provisoire, next-free au-dessus de `origin/main` au rebase).

## Contexte

PDO a deux journaux, tous deux liés à un Run : l'`event_log` (vérité du Run ; `Event.run_id`
**obligatoire**, la projection refuse tout fragment sans `RunStarted`) et `trigger_fires` (les
évaluations de scheduling, keyées par Trigger). Une mutation de configuration faite **hors Run** —
désactiver un Trigger à la main, éditer son cron, mettre les Triggers en pause — n'a de place dans
aucun des deux. Conséquence mesurée : un Trigger coupé manuellement a été diagnostiqué à tort comme
une panne de scheduler (fausse issue #505), parce que rien ne prouvait le geste.

## Décision

Une **troisième table**, `audit_log` (module frère `audit_log.rs`), sans `run_id`, append-only,
partageant le pool `pdo.db`. v1 journalise les **mutations de Trigger** (create / patch / delete) et
la **pause globale**, instrumentées au **handler HTTP** (pas au store : seul le handler a l'origine et
l'avant→après ; le store ne voit que le delta). Lue par `GET /audit` (feed global décroissant,
filtrable par cible et fenêtre de temps).

L'origine est **best-effort et déclarative** : un en-tête `X-PDO-Actor` (`ui`/`cli`/`unknown`) stocké
dans une colonne nommée **`actor_hint`** — jamais `actor`. Le daemon bind 0.0.0.0 sans auth :
l'origine est un indice falsifiable, **jamais un gate de comportement**. « Origine inconnue » est
acceptable ; « aucune entrée » ne l'est pas.

Invariant : l'audit peut **sous-rapporter**, jamais **sur-rapporter**. On lit l'avant, on mute, on
écrit l'audit **après le commit** ; un échec d'écriture se logue en `error!` et **n'échoue pas** la
mutation. Best-effort exclut le transactionnel (une transaction laisserait un audit verrouillé annuler
la mutation).

## Alternatives écartées

- **Rendre `Event.run_id` optionnel** : `run_id` est la clé de sharding de l'event log
  (`load_all_run_ids`, projection lisant `RunStarted`) ; le relâcher fabrique des Runs fantômes et
  casse un invariant load-bearing. Rejeté.
- **Étendre `trigger_fires`** : erreur de catégorie — `trigger_id NOT NULL`, colonnes de scheduling,
  JOIN de stats. Un audit hétérogène (Triggers + pause d'instance) n'y entre pas. Rejeté.

## Conséquences

- L'invariant « toute mutation de config est auditée » est tenu par **discipline au seam handler**,
  pas par le type-système : un futur appelant non-handler de `trigger_store::update` échapperait
  silencieusement à l'audit. Le seam est load-bearing — à garder à l'esprit avant d'ajouter un chemin
  de mutation bulk/programmatique.
- Rétention non bornée assumée (débit minuscule ; un cap serait une suppression non auditée).
- `PUT /settings` et le signal *vivant* `overdue` (AC5 de #507) restent hors v1 (suivi côté #222).
