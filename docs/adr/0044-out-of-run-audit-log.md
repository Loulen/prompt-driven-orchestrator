# ADR-0044 — Un troisième journal : `audit_log` pour les mutations de config hors-Run

> Statut : accepted (grilling #507). Vocabulaire : CONTEXT.md § « Trigger / Persistence »
> (« trois journaux, trois questions »).

## Contexte

Sans cette ADR, une mutation de configuration faite **hors Run** (désactiver un Trigger, éditer son
cron, mettre les Triggers en pause) ne laisse aucune trace : l'`event_log` exige un `run_id` et
`trigger_fires` ne journalise que les évaluations de scheduling. Conséquence mesurée : un Trigger
coupé manuellement a été diagnostiqué à tort comme une panne de scheduler (fausse issue #505), parce
que rien ne prouvait le geste.

## Décision

Une **troisième table**, `audit_log`, sans `run_id`, append-only, partageant le pool `pdo.db`. v1
journalise les **mutations de Trigger** (create / patch / delete) et la **pause globale**,
instrumentées au **handler HTTP** (pas au store : seul le handler a l'origine et l'avant→après ; le
store ne voit que le delta). Lue par `GET /audit`.

L'origine est **best-effort et déclarative** : un en-tête `X-PDO-Actor` stocké dans une colonne
nommée **`actor_hint`** — jamais `actor`. Le daemon bind 0.0.0.0 sans auth : l'origine est un indice
falsifiable, **jamais un gate de comportement**. « Origine inconnue » est acceptable ; « aucune
entrée » ne l'est pas.

Invariant : l'audit peut **sous-rapporter**, jamais **sur-rapporter**. On lit l'avant, on mute, on
écrit l'audit **après le commit** ; un échec d'écriture se logue et **n'échoue pas** la mutation. Le
best-effort exclut le transactionnel — une transaction laisserait un audit verrouillé annuler la
mutation.

## Alternatives écartées

- **Rendre `Event.run_id` optionnel** : c'est la clé de sharding de l'event log ; le relâcher
  fabrique des Runs fantômes et casse un invariant load-bearing.
- **Étendre `trigger_fires`** : erreur de catégorie (`trigger_id NOT NULL`, colonnes de scheduling,
  JOIN de stats). Un audit hétérogène (Triggers + pause d'instance) n'y entre pas.

## Conséquences

- L'invariant « toute mutation de config est auditée » est tenu par **discipline au seam handler**,
  pas par le type-système : un futur appelant non-handler de `trigger_store::update` échapperait
  silencieusement à l'audit. À garder à l'esprit avant d'ajouter un chemin de mutation
  bulk/programmatique.
- Rétention non bornée assumée (débit minuscule ; un cap serait une suppression non auditée).
- `PUT /settings` et le signal *vivant* `overdue` restent hors v1 (suivi côté #222).
