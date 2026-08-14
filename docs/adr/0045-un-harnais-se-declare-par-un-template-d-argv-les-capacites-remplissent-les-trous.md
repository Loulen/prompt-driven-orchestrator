# ADR-0045 — Un harnais se déclare par un template d'argv ; les capacités remplissent les trous ; l'absence est dite, jamais suppléée

> Statut : accepted (grilling du 2026-08-14, mesures sur `claude` 2.x et `opencode` 1.18.18).
> Vocabulaire : CONTEXT.md §*Harnais agentique*. Précédence et conditionnement du modèle → **ADR-0044**.
> **Amende ADR-0032** : « la mort de session est le seul verdict de mort » devient un **critère
> d'éligibilité** d'un harnais, pas seulement un constat sur `claude`.

## Contexte

Le lancement d'un nœud porte aujourd'hui cinq flags propres à Claude Code, un fichier de réglages
injecté (ADR-0043), une identité de session épinglée (#473), un parse de transcript JSONL et une ancre
de texte dans le pane. Accueillir un second harnais demande de trancher ce qui se **déclare** et ce qui
s'**écrit**.

## Ce qu'on décide

1. **Un harnais se déclare par deux templates d'argv** (lancement, reprise) et un bloc d'env. Règle
   unique : **un token contenant un trou vide est supprimé en entier**. Les trous sont le prompt, le
   modèle, l'effort, l'identité de session et le fichier de réglages.
2. **Les capacités remplissent les trous.** Une capacité est du **code** écrit harnais par harnais :
   coût, résolution du transcript, complétion sur fin de tour, détection de menu de limite, plancher de
   staging, épinglage d'identité. Sans capacité, le trou reste vide, le token disparaît, et la feature
   est **absente et nommée absente** (un coût illisible s'affiche « — », jamais `$0`).
3. **Éligibilité : résident.** Un harnais qui sort en fin de travail est refusé.
4. **PDO ne valide pas un descripteur.**

## Pourquoi, et ce qui a tué les alternatives

**Des champs nommés n'achètent rien — deux harnais suffisent à le montrer.** `--model X` contre
`-m X` ; prompt en argument positionnel contre `--prompt` ; et un harnais **sans aucun axe d'effort au
lancement** (mesuré : sur `opencode` 1.18.18, l'effort se règle par une commande *dans* la session,
pas par un flag). Un champ nommé par cas devient une épellation par cas, alors que le template les
couvre sans les nommer — et l'absence d'un trou est elle-même le signal exploitable par l'UI, qui
grise le picker sans réglage supplémentaire.

**Déclarer les sondes dans un mini-langage est réfuté par la mesure.** Le fold de coût de Claude Code
dédupe sur deux clés et somme quatre buckets de cache avec des multiplicateurs distincts ; `opencode`
écrit lui-même un coût par message et n'expose que deux buckets (read/write). Non seulement aucun
langage déclaratif raisonnable n'exprime les deux, mais **un recalcul uniforme est impossible** : les
buckets ne se mappent pas l'un sur l'autre. Le coût reste donc du code, avec deux formes légitimes —
**rapporté** par le harnais, ou **dérivé** par PDO (tokens × table de prix, ADR-0034).

**Un trait fermé à N implémentations est écarté** pour deux raisons : il interdit d'ajouter un harnais
sans recompiler, et il ferait remonter les particularités de Claude Code (le sibling `.claude.json`,
l'appariement de transcript par répertoire de travail) dans l'interface que tous les autres doivent
implémenter.

**Le résident est obligatoire, et le one-shot est refusé malgré son avantage.** En one-shot la fin de
tour serait **gratuite** — la sortie du process *est* le signal — là où `claude` a demandé ADR-0032 §2
puis ADR-0043 pour l'obtenir. On refuse quand même : un nœud doit rester attachable et conversationnel
jusqu'à son reap, c'est le principe même du produit (ADR-0012), et en one-shot la mort de session
cesse d'être un verdict d'échec (ADR-0032). Deux invariants ne s'échangent pas contre un confort
d'implémentation.

**Pas de validation du descripteur** (ADR-0001) : PDO ne sait pas ce que le template *veut dire*. La
conséquence est réelle et connue — un descripteur sans flag d'autonomie donne un nœud arrêté sur un
dialogue de permission, vivant et immobile, que **rien** ne détecte depuis la suppression du filet de
staleness (#469). Le premier nœud lancé le dit ; PDO ne le devine pas.

## Limites acceptées

- **Un harnais qui ne laisse pas imposer l'identité de session** ne peut être attribué que par
  répertoire de travail — précisément le trou que #473 vient de fermer pour `claude`. Mesuré sur
  `opencode` : lancer avec un id de session neuf répond « Session not found » et sort en 1 — le
  sélecteur *continue* une session, il ne la crée pas. Deux nœuds `doc-only` concurrents partagent le
  worktree du Run, donc leurs sessions se confondent : coût attribué au mauvais nœud, fin de tour lue
  sur la mauvaise session. Accepté en v1.
  Le levier mesuré n'est pas de relocaliser le store : c'est que ce harnais **sert une API HTTP locale**
  et accepte un **port au lancement**. PDO attribuant déjà un port par nœud, il peut créer la session
  lui-même puis s'y attacher, et lire messages, coût et arrêt par cette API — sans coupler PDO au
  schéma d'une base étrangère. À vérifier avant de s'engager : le listing de sessions de cette API est
  **global**, pas scopé au port, donc seule la création-puis-attache ferme réellement le trou.
- **Le store d'un harnais n'est pas un contrat.** Mesuré : `opencode` 1.18.18 écrit ses sessions et ses
  messages dans un SQLite, et son ancien store de fichiers JSON — encore présent sur disque, avec des
  mois de données — n'est plus écrit du tout. Une capacité de coût branchée sur la forme *observée* du
  store lit de l'historique mort et rapporte zéro sur chaque Run : la lecture passe par l'API du harnais
  quand il en a une.
- **Un modèle qu'un harnais ne peut pas honorer ne fait pas forcément échouer le lancement.** Mesuré :
  `opencode` avec un modèle valide mais injoignable (fournisseur non authentifié) **retombe en silence**
  sur son défaut et rend un tour vert sous un autre modèle ; avec un modèle joignable, le flag est bien
  honoré. PDO ne peut donc pas compter sur un échec bruyant pour révéler un réglage non tenu — le mode
  d'échec appartient au harnais.
- **Le choix « reprendre par identité ou en aveugle » reste en code** : il parle des lignes d'event log
  antérieures à #473, pas du harnais.
- **Le sandbox est hors périmètre** : le plancher de staging reste propre à `claude` (ADR-0031). Un
  autre harnais dans un Run sandboxé ne tient que par le Dockerfile de l'utilisateur et les exceptions
  `$HOME` de son profil, sans aucune garantie du plancher — et PDO le dit une fois, visiblement.
- **Un harnais déclaré sans capacité est utilisable mais aveugle** : il tourne, on l'attache, il
  complète ; il ne rapporte ni coût ni fin de tour. C'est la contrepartie assumée de « on peut ajouter
  un harnais à la volée ».
