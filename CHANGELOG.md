# Changelog

Ce fichier ne consigne que les changements **cassants** et les notes de version qui ne se
déduisent pas d'un titre de commit. L'historique complet vit dans le git log et dans les
[Releases GitHub](https://github.com/Loulen/prompt-driven-orchestrator/releases), générées
depuis les commits.

Le projet suit le versionnement sémantique. Il n'a **aucun objectif de compatibilité
ascendante** : la casse se signale ici et par un bump majeur, jamais en gardant des champs
morts. Seule contrainte non négociable — les **données historiques restent lisibles** : un Run
archivé s'ouvre et se chiffre quelle que soit la version qui a écrit son payload.

## 1.2.0

Deux changements cassants livrés sous un bump **mineur**, par décision explicite du mainteneur : la
v2.0.0 qu'avaient choisie les deux slices n'a jamais été taggée ni publiée (la dernière release est la
v1.0.0), et aucune des deux ruptures ne change la surface qu'un utilisateur pilote au quotidien.

### Cassant — `session_died` est le seul verdict de liveness, l'état `Stale` disparaît (#469)

Un nœud n'est plus déclaré mort sur une absence d'activité : seule la mort de sa session compte, et la
complétion sur fin de tour constatée est opt-in (ADR-0032, ADR-0012). L'état de nœud `Stale` est
supprimé — un client qui l'attendait ne le verra plus. Nouveau réglage `autocomplete_turn_end`
(`PDO_AUTOCOMPLETE_TURN_END`), off par défaut.

### Cassant — `GET`/`PUT /settings` perdent `image_source` et `dockerfile_path` (#471)

La source de l'image d'un Run sandboxé n'est plus un réglage d'instance. Elle appartient au
**profil de staging**, comme le contenu de home et l'env (ADR-0031 §9) : un axe par écran, l'écran
de réglages ne répondant plus qu'à « quel profil un Run prend par défaut ».

- `GET /settings` ne contient plus `image_source`, `dockerfile_path`, ni la disclosure
  `sandbox_image` (le tag que le Dockerfile résolu produisait) ;
- `PUT /settings` portant l'un de ces champs répond **400 en le nommant**, au lieu de 200 en
  l'ignorant silencieusement. Un champ simplement inconnu reste ignoré, comme avant ;
- les colonnes `instance_config.image_source` / `.dockerfile_path` deviennent **inertes**. Elles ne
  sont pas droppées, et une valeur qui y traîne déclenche **un** `warn!` au boot qui la nomme et
  renvoie vers le profil à éditer. Aucun Run n'est affecté ;
- `PDO_SANDBOX_IMAGE_SOURCE` et `PDO_SANDBOX_DOCKERFILE` sont **conservées** et repointées sur le
  nouveau défaut de profil : une instance headless n'a que des profils virtuels et pas d'UI, donc
  l'env reste son seul moyen de changer d'image sans POSTer un profil.

**Migration.** Aucune action requise si les deux réglages n'ont jamais été touchés (leur valeur par
défaut est désormais la constante `DEFAULT_PROFILE_IMAGE` : registre hash-dérivé sur le Dockerfile
seedé, donc un profil qui ne pose pas d'image produit exactement le même ref qu'avant). Si l'un des
deux portait une valeur, le boot le dit : reporter le choix sur le profil concerné via
**Settings → Manage staging profiles…**, ou sur la variable d'environnement du daemon.
