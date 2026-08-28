# ADR-0031 — Profils de staging (home, environnement et image d'un Run sandboxé)

Sans cette ADR, on traiterait le contenu du *staged Claude home* comme une constante Rust et le mode
sandbox comme le seul levier — c'est-à-dire qu'un pipeline ayant besoin des skills mais pas des serveurs
MCP paierait ~1 Go par Run ou perdrait tout, et qu'aucun Run sandboxé n'aurait accès à ce qui vit hors
de `~/.claude` (identité git, auth `gh`).

> Statut : accepted (PRD #403 ; §1 en #426, §2-§7 en #432, §8 en #468, §9 en #467/#471). Complète
> ADR-0030 : celle-là dit *où* tourne un Run sandboxé, celle-ci *avec quoi*. Le nom de stockage
> `sandbox_profiles` est conservé malgré ce périmètre élargi : le renommer coûterait une repointe des
> trois stockages qui comparent son nom pour un gain de prose.

Le contenu du *staged Claude home* devient un **profil de staging** : une liste nommée, éditable,
sélectionnable par Run et par Trigger — qui porte aussi l'environnement du conteneur (§8) et sa source
d'image (§9).

## Ce qu'on décide

1. **Le plancher est une liste de garanties, pas de fichiers verrouillés** *(#426)*. Quel que soit
   le profil, la prep garantit qu'au démarrage la session dispose de : credentials valides, managed
   settings de l'org consentis, bypass permissions accepté, confiance pré-accordée à la racine du
   Run, répertoire de transcripts vide. Chaque garantie est satisfaite **soit** par une entrée du
   profil, **soit** par une synthèse de repli. C'est ce qui rend le décochage sûr sans avoir à
   l'interdire. Formulé en fichiers, le plancher se contredirait dès le premier cas réel : les
   settings sont copiés depuis l'hôte en `full` mais synthétisés à une seule clé en `minimal`, et
   un utilisateur qui décoche les siens (ses hooks hôte n'existent pas dans le conteneur) doit
   obtenir la synthèse, pas un refus.

2. **Un profil est un *diff* contre le défaut, jamais un instantané.** Le stockage retient
   l'intention (`disabled`, `extras`), pas la liste effective. Un instantané figerait l'install :
   le jour où une version de PDO ajoute une entrée au défaut — ce que le plancher vient de faire —
   les profils existants ne la verraient jamais. Corollaire : `minimal` et `full` sont des
   **défauts virtuels** (aucune ligne en base) jusqu'à édition ; les éditer matérialise une ligne
   portant elle aussi un diff.

3. **Une entrée est un chemin relatif à `$HOME`** (ex. `.claude/skills`, `.gitconfig`,
   `.config/gh`). Refusés : chemin absolu, `..`, toute sortie de `$HOME`, et le puits de
   transcripts runtime sous `.claude` (le copier casserait l'idempotence du merge-back et le calcul
   de coût). `.ssh`, `.aws`, `.gnupg` sont **autorisés avec avertissement** — les interdire serait
   du théâtre alors qu'ADR-0030 assume déjà l'uid hôte, le repo monté rw et de vraies credentials
   Claude.

4. **Les entrées hors `.claude` sont copiées puis montées, jamais bind-montées depuis l'hôte**, en
   rw. L'invariant « le vrai `~/.claude` n'est jamais monté » s'étend au reste de `$HOME`. Un bind
   direct exposerait l'hôte à l'écriture du conteneur : un agent qui bute sur l'auto-détection
   d'email git fait très naturellement `git config --global`, et réécrirait le `~/.gitconfig` de
   l'utilisateur. Les écritures utiles du conteneur (refresh de token) sont perdues au teardown —
   assumé, le merge-back ne remonte que les transcripts. **Dédup obligatoire** : une entrée sous
   `.claude/` ne reçoit pas son propre mount, elle est déjà servie par le mount `.claude` (un
   double bind serait accepté par Docker et résolu par profondeur de chemin — un bug de dimanche).

5. **Le champ sandbox reste une valeur unique : `off` ou un nom de profil.** Pas de liste par Run
   ni par Trigger ; la précédence existante (explicite → Trigger → défaut d'instance) ne bouge pas.
   L'alternative — le réglage-liste sur les trois tiers — imposerait le widget d'édition à trois
   endroits et une composition de diffs entre tiers dont aucune sémantique n'est devinable.

6. **Le nom du profil ET la liste résolue sont gelés dans `RunStarted`.** La prep lit l'état du
   Run, jamais le réglage vivant : elle est rejouée à quatre moments (création, boot recovery,
   résurrection de session, run-shell) et elle est additive — sans gel, un daemon redémarré après
   une édition du profil produirait un home incohérent entre deux nœuds du même Run. Le gel de la
   **liste** en plus du **nom** évite qu'éditer un profil réécrive rétroactivement ce qu'un Run
   passé a stagé. **L'env de §8 et la source d'image de §9 sont gelés au même endroit et à la même
   création** (clés sœurs écrites au même resolve : deux lectures pourraient enjamber une écriture
   concurrente et geler une liste d'une révision avec un env — ou une image — d'une autre). Pour
   l'image l'enjeu est le plus visible : sans gel, deux nœuds du même Run peuvent tourner dans
   **deux images différentes**, et le second échouerait sur des outils que le premier avait.

7. **Un nom de profil inconnu échoue fort, partout.** 400 à la création de Run, échec visible du
   tir de Trigger, `RunFailed` explicite en boot recovery. Jamais de retombée silencieuse sur le
   défaut d'instance — le comportement que produirait naturellement un parse tolérant, et
   qu'ADR-0030 pt 4 interdit déjà pour l'indisponibilité de Docker. Côté UI, supprimer un profil
   référencé liste ses référents avant confirmation : garde-fou souple, pas d'intégrité
   référentielle en base.

8. **Un profil porte aussi un `env`, posé au `docker create`** *(#468)*. Une map `{CLÉ: valeur}`
   posée au create, à côté des vars run-constantes de PDO. L'essentiel des arbitrages :

   - **Au create et non à l'exec** : ce sont des constantes de *Run*, pas des variables de *nœud* ;
     les nœuds suivants héritent de l'environnement du conteneur, donc le gel de §6 est satisfait
     sans code supplémentaire (Docker ne réévalue pas l'env d'un conteneur existant).
   - **Ce n'est PAS un diff** (contrairement à `disabled`/`extras`) : il n'y a pas d'env par défaut
     à fold — la map stockée *est* la map effective.
   - **Trois clés réservées** (celles que PDO possède : le home, l'URL du daemon, l'id du Run),
     refusées par un **400 qui les nomme** — jamais un skip silencieux. La liste est possédée par
     le module qui pose l'env, consommée par le validateur (un seul littéral, précédent #447). La
     clé de désactivation du trafic Claude Code n'est volontairement pas réservée (la surcharger
     est légitime) et elle est alors remplacée *en place* — aucune clé posée deux fois, car
     « laquelle gagne » dépendrait de la couche qui lit l'environnement.
   - **Les VALEURS ne sont jamais loggées, les noms oui** : un chemin relatif à `$HOME` n'est pas
     un secret, une valeur d'env l'est souvent, et le journal survit au Run *et* au profil — une
     fuite y est un incident irréversible.
   - **Ce n'est pas un coffre-fort, et l'UI l'écrit.** Les valeurs sont en clair dans la base, dans
     le payload gelé et dans l'inspection du conteneur. La sandbox n'étant pas une frontière de
     sécurité (ADR-0030), les masquer dans l'éditeur serait du théâtre — pire, ça laisserait croire
     que PDO protège quelque chose. La phrase est du texte **porteur** : sans elle quelqu'un y met
     une clé API en croyant à un secret store.

   Ce que ça débloque : un serveur MCP fourni par un plugin est hors du contrôle de PDO et ne se
   configure que par variables d'environnement (chemins d'exécutables, proxys d'entreprise,
   endpoints). Sans §8, le profil décrivait le contenu du home et **rien** de l'exécution.

9. **Un profil porte aussi sa source d'image** *(#467, complété #471)*. Jusque-là l'image était un
   réglage d'instance : impossible d'avoir un profil chrome-devtools et un profil minimal côte à
   côte. Un profil (tout nom sauf `off`, qui n'a pas de conteneur) peut porter soit un Dockerfile
   pointé (`kind: dockerfile`), soit un ref registry libre (`kind: registry`). **La rationale
   complète — précédence `profil → env → défaut de profil`, retrait du réglage d'instance, et ce
   qu'un ref registry explicite perd (pas de repli build, pas de retag, erreur dure nommée) — vit
   dans ADR-0030 pt 7** : c'est l'invariant du tag qui est en jeu, et il appartient à ADR-0030. Ce
   qui appartient à cette ADR :

   - **Ne rien poser est un état de première classe, et le défaut** : clé absente du payload gelé,
     et un profil qui ne pose rien produit des arguments de create bit pour bit identiques à
     avant — la propriété qui rend la slice sûre pour les instances existantes.
   - **Gel par Run, au même endroit que la liste et l'env** (§6). Le gel porte sur le **choix du
     profil** ; les tiers env, eux, restent relus à chaque prep (ADR-0015 : « un changement mord au
     prochain ensure »). Un Run archivé dont le payload porte les anciens champs d'instance s'ouvre
     et se chiffre toujours : ce qui est retiré est une surface d'API, pas un format de lecture.
   - **Stockage dédié, dégradation indépendante** : la source d'image se valide seule et se dégrade
     seule en « rien posé », plutôt qu'un profil entier illisible ; une forme taguée exclusive
     plutôt que deux champs qui pourraient se contredire.
   - **La validation d'un ref est syntaxique seulement** : PDO refuse ce qui ne peut pas être tiré
     du tout (vide, espaces, tiret initial lu comme un flag) mais ne sonde ni l'existence ni le
     contenu.

   Consommateur direct : la variante d'image chrome-dev (#466) devient sélectionnable **par
   profil** au lieu de l'être pour toute l'instance.

## Alternatives écartées

- **Extras seulement, sans décochage.** Simple, mais ne règle pas le poste de coût — le seul
  élément qu'on veuille vraiment retirer est dans le défaut.
- **Override complet de la liste.** Plus souple, moins découvrable, et prive l'install des
  évolutions futures du défaut (le plancher serait réinjecté).
- **Réglage-liste sur les trois tiers** (Run / Trigger / instance). Écarté en §5.
- **Manifeste dans le staging dir** plutôt que gel dans `RunStarted` : éclate le contrat
  d'isolation d'un Run sur deux stockages, et disparaît au teardown.
- **Interdire `.ssh`/`.aws`/`.gnupg`.** Incohérent avec la posture v1 d'ADR-0030 (cf. §3).
- **Env posé à l'exec plutôt qu'au create** (§8) : traiterait une constante de Run comme une
  variable de nœud — re-passage à chaque session, et le gel de §6 demanderait du code.
- **Env chiffré / secret store** (§8). Écarté pour v1, et pas par paresse : la sandbox n'est pas
  une frontière de sécurité, donc un chiffrement au repos protégerait la base contre un attaquant
  qui a déjà le conteneur. Dire la vérité dans l'UI est plus honnête que du chiffrement décoratif.
  Un vrai secret store devient défendable le jour où ADR-0030 change de posture.
- **Env sur les trois tiers** : même argument que §5, plus la question « une clé du tier bas
  est-elle écrasée ou fusionnée ? », dont aucune réponse n'est évidente.
- **Une 3e valeur de la source d'image d'instance** plutôt qu'une forme portée par le profil : le
  *choix* sur l'instance et la *valeur* sur le profil — deux réglages à tenir cohérents, et un état
  absurde atteignable.
- **Retenir les réglages d'image d'instance** une fois le profil capable de tout dire — d'abord
  l'option conservée par #467 (« les retirer serait une migration forcée »), **renversée en
  #471** : l'argument était faux sur son point central. Les retirer n'impose aucune migration,
  parce que leur valeur par défaut devient une constante de la couche de défauts de profil — un
  profil qui ne pose rien produit exactement le même ref qu'avant. Ce que #467 avait confondu :
  « le défaut doit exister » (vrai) et « le défaut doit être réglable sur cet écran » (faux).
- **Rendre le pull d'un ref explicite tolérant** (retomber sur un build ou l'image hash-dérivée) :
  la plus tentante et la plus dangereuse — elle démarrerait le Run dans une image sans rapport avec
  celle demandée. Voir ADR-0030 pt 7.
- **Geler aussi les tiers d'instance dans `RunStarted`** : changerait le contrat ADR-0015 de
  réglages existants pour tous les Runs, y compris sans profil — une régression de comportement
  déguisée en amélioration.

## Limites acceptées

- Le blast radius filesystem n'est plus « rien d'autre que `.claude` » mais « ce que le profil
  déclare » : le refus par défaut de `$HOME` devient une liste d'exceptions déclarées et visibles
  (cf. ADR-0030, « Pourquoi »).
- Les profils vivent en base, pas sur disque : ils ne se versionnent pas avec le repo. Assumé — un
  profil référence des chemins et des refs spécifiques à la machine. Le Dockerfile, lui, reste sur
  disque précisément parce qu'il est fait pour être partagé.
- Une édition de profil ne rattrape pas les Runs en vol (conséquence directe du gel, §6).
- **Les valeurs d'env sont en clair, dans trois stockages** (§8), dont le fichier d'événements du
  Run, immuable : retirer une valeur du profil ne la retire pas des Runs qui l'ont gelée — la seule
  remédiation est de faire tourner le secret. C'est le prix du gel, et c'est pourquoi l'UI dit que
  ce n'est pas un coffre-fort.
- **`env` n'est pas un diff** (§8), donc pas de forward-compatibility à la §2 : si une version
  future de PDO posait une var par défaut, un profil ne pourrait pas la « décocher ». Acceptable
  tant que PDO ne pose que des vars run-constantes — qui sont, elles, réservées.
- **Un profil `kind: registry` ne bénéficie plus du repli build** (§9) : un registre injoignable
  fait échouer ses Runs. C'est le prix explicite du ref libre, écrit dans l'éditeur à l'endroit du
  choix.

## Amendements (#432)

### A1 — §6 : un Run d'avant les profils relit le défaut vivant

Un payload `RunStarted` qui porte un nom de profil virtuel **sans** liste gelée fait re-résoudre le
défaut virtuel maintenant — entorse **sciemment** consentie au gel. Ce cas n'est atteignable que pour un
Run créé par un daemon pré-#432, dont le staging a été purgé, puis repris. Les deux alternatives sont
pires : `RunFailed` sur un Run parfaitement résoluble, ou figer dans le Rust pour toujours le défaut tel
qu'il était — ce qui contredirait §2. Un nom d'**utilisateur** sans liste gelée échoue dur.

### A2 — `$HOME` n'est pas inscriptible dans le conteneur

`git config --global` depuis le conteneur ne modifie **pas** la copie stagée : `$HOME` n'existe pas dans
l'image (l'image de base livre un autre home), donc Docker le crée comme parent des mounts, possédé par
root ; le motif *lock-file-puis-rename* échoue alors en permission refusée. Condition pré-existante, pas
une régression des profils. Ce qui est vrai : (1) le `~/.gitconfig` de l'hôte n'est **jamais** muté —
doublement garanti (copie-puis-mount de §4, et l'échec ci-dessus) ; (2) une écriture du conteneur sous
`$HOME` atterrit dans la copie stagée, pour une entrée répertoire inscriptible. Rendre `$HOME`
inscriptible toucherait les identity mounts d'ADR-0030 §1 : arbitrage produit à part. Sans lui,
`git config --global`, `gh auth login` et tout outil qui crée un dotfile *nouveau* échouent.
