# Un Nœud à la demande n'est ni attendu ni skippé par le graphe

> Statut : accepted (grilling du 2026-10-08, CORE-18). Vocabulaire : CONTEXT.md « Nœud à la demande »,
> « Attente des nœuds lancés », « Forcer un nœud skipped ».

Un Pipeline peut porter des nœuds que seul l'utilisateur décide de lancer (une revue, un
diagnostic, un rapport) : connectés ou non au reste du graphe, ils ne doivent ni bloquer le Run
quand on ne les lance pas, ni disparaître parce que leur amont n'a pas tiré. Nous en faisons une
**option d'un Node** (`on_demand`), comme `interactive`, plutôt qu'un type de node : un nœud à la
demande reste un agent ou un script.

- **Jamais lancé ni skippé par le runtime.** Un nœud sans edge entrante est aujourd'hui une racine
  lancée au démarrage, et un nœud dont toutes les edges entrantes sont mortes est skippé. Le nœud à
  la demande sort de ces deux règles : seul le ▶ de l'utilisateur le lance.
- **Ses inputs ne sont pas requis.** Il prend la dernière itération terminée de chaque amont
  connecté, s'il y en a une.
- **Il ne retient pas l'aval.** Ses edges sortantes ne comptent pas dans la barrière de l'aval
  (ADR-0082) tant qu'il n'est pas lancé ; une fois terminé, elles partent normalement vers un aval
  pas encore lancé. Un aval déjà terminé n'est pas relancé : pas de cascade jusqu'à `End`.
- **La fin du Run ne change pas par défaut.** Le Run se termine à `End` ; l'option de Pipeline
  « Attente des nœuds lancés » (désactivée par défaut) le fait attendre que tout nœud lancé ait
  abouti.

## Considered Options

- **Un type de node `on_demand`** : rejeté, il aurait fallu le décliner en agent et script, et le
  précédent `interactive` / `orchestrator` en fait une option orthogonale au type.
- **L'aval attend le nœud à la demande comme tout amont** : rejeté, un nœud jamais lancé
  bloquerait l'aval et donc le Run, ce qui force l'utilisateur à le lancer ou à le skipper.
- **Relancer l'aval déjà terminé quand le nœud à la demande finit** : rejeté, une relance en
  cascade rouvrirait tout jusqu'à `End` pour un geste ponctuel ; Retry sur l'aval reste le geste
  explicite.
