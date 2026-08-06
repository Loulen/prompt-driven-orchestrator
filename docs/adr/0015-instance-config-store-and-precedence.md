# Configuration d'instance : store SQLite singleton + précédence `stored → env → default`

Trois réglages runtime daemon-wide n'existaient qu'en variable d'environnement (shell + redémarrage) : le **cap de sessions** (`PDO_SESSION_CAP`, défaut 20), le **reaper TTL** (`PDO_REAPER_TTL_SECS`, défaut 3600 s) et le **timeout du guard de Trigger** (`PDO_GUARD_TIMEOUT_MS`, défaut 60 s). CONTEXT.md promettait de longue date une page de réglages instance-wide qui les expose (#129 ; cf. *Cap de sessions*, *Trigger*, *terminal inline*). C'est la mauvaise ergonomie pour l'arc d'autonomie non-attendue (ADR-0012 nomme le cap comme *la* primitive de sécurité contre l'effondrement tmux #77/#78).

**Décision.** On introduit une **Configuration d'instance** : une table SQLite **singleton** `instance_config` (une seule ligne) dans `pdo.db`, calquée sur le store des Triggers (config + état mutable, pas un artefact canvas-backed → mauvais fit YAML, cf. CONTEXT.md *Persistence — table SQLite*) ; des routes daemon `GET`/`PUT /settings` ; un écran de réglages frontend. La résolution de chaque réglage suit l'ordre **`stored → env → default`** : la valeur stockée (UI) **gagne**, la variable d'environnement devient un *bootstrap* consulté seulement quand le stored est `NULL`, le défaut est le plancher. `GET /settings` renvoie par champ `{ effective, source, stored, env, default }` (`source ∈ {stored, env, default}`) pour que l'UI **révèle** un env masqué plutôt que de l'ignorer en silence.

**Pourquoi `stored → env → default` et pas `env → stored → default`.** Une page de réglages qu'une variable d'environnement écrase silencieusement est un **no-op pour les opérateurs mêmes pour qui elle est construite** (l'install prod exporte typiquement `PDO_SESSION_CAP` via systemd) : ils changent la valeur, rien ne se passe, aucun feedback — pire qu'une absence de page. La page doit faire autorité sur ce qu'elle expose. Les deux ordres gardent la suite de tests verte (tous positionnent l'env, aucun n'écrit de stored → `NULL` stored retombe sur l'env dans les deux cas), donc les tests ne départagent pas : c'est l'intention produit qui tranche. Le seul risque — un opérateur qui *épingle* délibérément le cap par env et est surpris que l'UI le surclasse — est désamorcé par la divulgation `source` (l'UI affiche « `PDO_SESSION_CAP=10` est positionné mais masqué par la valeur stockée 30 »).

**Hors scope explicite (décision-frontière).** Toute option « le manager vérifie périodiquement le pipeline » reste **exclue** : elle réveillerait le manager depuis le runtime, ce qui renverse *Pas de polling actif* (CONTEXT.md *Pipeline Manager*) et touche la frontière d'origine-de-l'autonomie d'ADR-0012 (l'autonomie est une propriété du *pipeline*, jamais du runtime). C'est une décision ADR/humaine séparée ; une page de réglages qui la câblerait construirait le couplage par la porte de derrière.

**Amendement #471 — un réglage peut aussi *sortir* de cette table, et le sandbox y perd un tier.**
`image_source` (#411) et `dockerfile_path` (#431) quittent `instance_config`, `GET /settings`, le
validateur de `PUT /settings` et l'écran de réglages. Depuis #467 la source d'image d'un Run se
configurait à **deux** endroits — le profil de staging, qui gagne, et ces deux réglages, qui servaient
de repli — et un seul avait sa place : les réglages répondent « quel profil un Run prend par défaut »,
un profil répond « ce qu'est le sandbox » (son image, le contenu de home, son env). Un axe par écran.
Le symptôme était déjà visible : l'AC 5 de #467 avait produit sous « Sandbox image source » une note
de quatre lignes qui devait renvoyer vers un autre écran — quand un réglage a besoin d'un paragraphe
pour expliquer sa relation avec un autre écran, il est sur le mauvais écran. Le coût est mesuré et nul :
sur la seule instance existante, les deux champs n'ont **jamais** porté de décision (`stored=None`,
`env=None`).

Quatre conséquences pour cette ADR, dont deux amendent ses règles générales :

- **La précédence du sandbox perd le tier `stored`.** Elle devient `profil (par Run, gelé) → env →
  défaut`, où le défaut n'est plus un champ de cette table mais une **constante de la couche de
  défauts de profil**. Un défaut n'a donc pas besoin d'être réglable *ici* pour exister ; c'est ce que le raisonnement de
  #467 avait confondu.
- **Le tier env survit à la disparition du tier stored**, ce qui inverse localement la lecture
  « l'env est un défaut de bootstrap » : `PDO_SANDBOX_IMAGE_SOURCE` et `PDO_SANDBOX_DOCKERFILE`
  deviennent le **seul** levier instance-wide, parce qu'une instance headless fraîche n'a que des
  profils virtuels et pas d'UI. Ce qui disparaît est le champ et l'écran, pas la variable.
- **Retirer un champ de `PUT /settings` est un `400` qui le nomme**, jamais un `200` qui l'ignore. Le
  projet n'a aucun objectif de compatibilité ascendante — la casse se signale par le versionnement
  sémantique et le CHANGELOG, pas en gardant des champs morts (précédent : ADR-0031 a renommé les
  profils `copy`/`pure` en `full`/`minimal` **sans alias**) — mais un client qui envoie un champ
  retiré doit l'apprendre, pas croire qu'il a été appliqué. Corollaire symétrique : un champ
  simplement **inconnu** reste ignoré par serde, comme avant.
- **Une valeur stockée qui devient inopérante se dit une fois.** Les colonnes ne sont pas droppées
  (supprimer une colonne SQLite sur une install vivante n'achète rien) : elles deviennent **inertes**,
  et le boot émet **un** warning qui les nomme et renvoie vers le profil à éditer. Une valeur que
  l'utilisateur a posée ne doit jamais cesser de compter en silence (même principe que #470).

**Conséquences.** L'env devient un défaut de bootstrap (rétro-compatible, ops peut pré-seeder avant première UI) ; `GET /sessions` (barre de statut) reste inchangé, `GET /settings` est la vue riche dédiée à la page ; ajouter un réglage futur = une colonne + un champ. Interagit avec ADR-0012 (le cap = primitive de sécurité) et le principe *Sharp tool* d'ADR-0001 (la page **configure**, elle ne prescrit pas — au plus un avertissement ambre au-delà du seuil d'effondrement tmux, jamais un blocage).
