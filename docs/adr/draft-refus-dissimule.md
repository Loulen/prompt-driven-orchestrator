# Un refus dissimulé répond comme une entité inconnue ; le paramétrage partagé et les Stats se lisent sous l'Autorisation d'opération

> Statut : accepted. Complète ADR-0099 (gestes grisés) et la Lecture autorisée (CORE-5).

Une lecture refusée d'une entité répondait `403` avec la Phrase de refus, et un id inconnu `404` :
quelques appels suffisaient à savoir qu'une entité existe et dans quel Projet. Les Stats comptaient
toute l'instance, et le paramétrage partagé (pipelines, bibliothèque, profils agentiques, skills,
réglages, journal d'audit) se lisait sans rien demander.

1. **Un refus peut être dissimulé.** L'Extension marque son refus `concealed`. Sur la lecture
   d'**une** entité (un Run et tout `GET /runs/{id}…`, un Trigger et ses tirs, une Session
   hébergée), le core répond **exactement** comme pour un id qu'il ne connaît pas : même statut,
   même corps, CLI comprise. Ailleurs (liste, mutation, paramétrage), c'est un refus ordinaire. Sans
   la marque, la Phrase de refus reste : qui connaît le Projet doit savoir à qui s'adresser.
2. **La réponse d'une entité inconnue est celle de la route, pas une copie.** Pour les lectures sous
   `/runs/{id}`, la garde rejoue la requête, sans garde, sous un id qu'aucun Run n'a, puis réécrit
   cet id en celui demandé dans la réponse. Une route ajoutée demain répond juste sans que personne
   n'y pense. Un Trigger et une session, lus par leur use case, rendent l'erreur de l'entité
   inconnue (`EntityRead::AsUnknown`).
3. **Un id inconnu est demandé hors Projet.** Le core ne sait pas où il serait : c'est à
   l'Extension de dissimuler aussi ses refus hors Projet pour l'appelant à qui elle dissimule un
   Projet, sinon l'écart `403` / `404` trahit les ids existants.
4. **Le paramétrage partagé se lit par une Opération `read`, hors Projet** (`pipeline`,
   `library_entry`, `agent_profile`, `skill`, `skill_folder`, `instance_settings`, et la nouvelle
   Ressource `audit`), par une garde posée sur tout le routeur, comme celle des lectures d'un Run.
5. **Les Stats ne comptent que ce que leur lecteur lit**, avec le filtre de la liste des Runs
   (`run` × `read` dans le Projet enregistré au lancement) et celui des Triggers. Le nom d'un
   Projet n'apparaît qu'à côté d'un Run : il disparaît avec ses Runs.

## Considered Options

- **Un 404 pour toute lecture refusée** : un Membre privé d'un Droit ne comprendrait plus pourquoi
  un lien de son Projet est « introuvable ».
- **Un corps générique « not found » écrit par la garde** : chaque route a sa propre réponse à un id
  inconnu (`run not found`, `[]`, `node not found in run`…) ; la moindre différence suffit à
  distinguer l'entité dissimulée.
- **Réécrire l'URI dans le layer** : un layer d'axum tourne après le routage, l'id est déjà extrait ;
  d'où le rejeu sur le routeur non gardé.

## Consequences

- `Authorization::Deny` et `Denied` gagnent le champ `concealed` (rupture de l'API d'extension).
- Un rôle sans aucune lecture voit des vues vides avec la phrase, des Stats à zéro, des réglages
  grisés.
