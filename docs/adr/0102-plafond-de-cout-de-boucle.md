# Le plafond de coût d'une boucle se vérifie à la re-entry et parque le Run

> Statut : accepted (grilling CORE-213). Vocabulaire : CONTEXT.md « Plafond de coût », « Coût de boucle ». S'appuie sur ADR-0011 (boucles), ADR-0022/0058 (coût dérivé à la lecture), ADR-0049 (le runtime parque, ne déclare jamais forfait).

**Une boucle `bounded` peut porter un plafond de coût en dollars. Il se vérifie uniquement quand une re-entry fire, sur le coût de boucle cumulé ; atteint, il parque le Run en `AwaitingUser` avec une raison dédiée et ne suit jamais la sortie d'épuisement. Le coût reste dérivé à la lecture : calculé au moment de la re-entry et fourni au scheduler comme une entrée, jamais persisté.** `max_iter` ne protège pas d'une boucle courte mais chère qui tourne la nuit : il faut un arrêt sur la consommation elle-même.

- **À la re-entry, pas en direct** : même point de décision que `max_iter`, donc déterministe et rejouable, et aucun travail en cours n'est tué. Le prix est un dépassement d'au plus un lap, accepté.
- **Parquer, pas suivre l'épuisement** : un plafond de coût est un arrêt de sécurité. Suivre une sortie `iter gte $max` enverrait le Run vers l'aval, donc dépenserait encore. La reprise passe par les gestes existants (relever le plafond, `force_route`, `end_region`).
- **Coût en entrée du scheduler** : le scheduler reste une fonction pure de l'état du Run. L'interpréteur lui fournit le coût de boucle, issu du même fold attribué que le total du Run (ADR-0058). Aucune nouvelle source de vérité, aucun coût écrit dans l'event log.
- **Coût inconnu** : il ne compte pas pour `$0` et ne bloque pas la boucle. La part connue compte ; le partiel est dit dans la raison et signalé à la pose du plafond.
- **Override à chaud dans l'event log** : `set_region_max_cost` (sœur de `set_region_max_iter`) prime sur le YAML, survit à une réouverture, et n'a pas de plancher : un plafond sous le coût déjà dépensé arrête la boucle à la re-entry suivante. C'est une Opération de Run comme les autres : même Droit que `set_region_max_iter`, exposée à la CLI et à l'API d'extension.

## Considered Options

- **Vérification en direct (tuer le nœud au franchissement)** : rejetée, non déterministe (les transcripts grossissent pendant la session) et destructrice de travail.
- **Persister le coût par exécution dans l'event log pour que le scheduler le lise** : rejetée, contredit ADR-0022/0029 (dérivé, jamais matérialisé) et crée une seconde vérité qui divergerait de la table de prix résolue.
- **Plafond sur les `collection`** : différé. Une `collection` n'a pas de re-entry (laps concurrents) ; il faudrait vérifier à l'admission, hors scheduler.
