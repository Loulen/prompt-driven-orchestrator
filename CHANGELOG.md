# Changelog

Ce fichier ne consigne que les changements **cassants** et les notes de version qui ne se
déduisent pas d'un titre de commit. L'historique complet vit dans le git log et dans les
[Releases GitHub](https://github.com/Loulen/prompt-driven-orchestrator/releases), générées
depuis les commits.

Le projet suit le versionnement sémantique. Il n'a **aucun objectif de compatibilité
ascendante** : la casse se signale ici et par un bump majeur, jamais en gardant des champs
morts. Seule contrainte non négociable — les **données historiques restent lisibles** : un Run
archivé s'ouvre et se chiffre quelle que soit la version qui a écrit son payload.

## 2.0.0

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
