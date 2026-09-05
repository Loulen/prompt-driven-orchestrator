# ADR-0045 — Un harnais se déclare par un template d'argv ; les capacités remplissent les trous ; l'absence est dite, jamais suppléée

Sans cet ADR, un agent accueillerait un second harnais par des champs nommés (`model_flag`,
`prompt_flag`…) ou un trait fermé à N implémentations, et supplierait une capacité manquante par un
défaut (un coût illisible affiché `$0`).

> Statut : accepted (grilling du 2026-08-14, mesures sur `claude` 2.x et `opencode` 1.18.18).
> Vocabulaire : CONTEXT.md §*Harnais agentique*. Précédence et conditionnement du modèle →
> **ADR-0046**. **Amende ADR-0032** : « la mort de session est le seul verdict de mort » devient un
> **critère d'éligibilité** d'un harnais, pas seulement un constat sur `claude`.
>
> **Amendé par ADR-0051** : une capacité n'est pas une garde qui décide *si* on appelle
> l'implémentation de `claude`, c'est le point où l'implémentation du harnais résolu est **choisie** ;
> `None` devient une valeur explicite.
> **Amendé par ADR-0054** : « PDO ne valide pas un descripteur » gagne une exception, unique et
> nommée — le template de lancement doit faire du binaire déclaré le leader du pane, faute de quoi le
> critère d'éligibilité posé ici est silencieusement faux.

## Ce qu'on décide

1. **Un harnais se déclare par deux templates d'argv** (lancement, reprise) et un bloc d'env. Règle
   unique : **un token contenant un trou vide est supprimé en entier**. Les trous sont le prompt, le
   modèle, l'effort, l'identité de session et le fichier de réglages.
2. **Les capacités remplissent les trous.** Une capacité est du **code** écrit harnais par harnais :
   coût, résolution du transcript, complétion sur fin de tour, détection de menu de limite, plancher
   de staging, épinglage d'identité. Sans capacité, le trou reste vide, le token disparaît, et la
   feature est **absente et nommée absente** (un coût illisible s'affiche « — », jamais `$0`).
3. **Éligibilité : résident.** Un harnais qui sort en fin de travail est refusé.
4. **PDO ne valide pas un descripteur.**

## Pourquoi, et ce qui a tué les alternatives

**Des champs nommés n'achètent rien — deux harnais suffisent à le montrer.** `--model X` contre
`-m X` ; prompt positionnel contre `--prompt` ; et un harnais **sans aucun axe d'effort au
lancement** (mesuré : sur `opencode` 1.18.18, l'effort se règle par une commande *dans* la session).
Un champ nommé par cas devient une épellation par cas — et l'absence d'un trou est elle-même le
signal exploitable par l'UI, qui grise le picker sans réglage supplémentaire.

**Déclarer les sondes dans un mini-langage est réfuté par la mesure.** Le fold de coût de Claude Code
dédupe sur deux clés et somme quatre buckets de cache à multiplicateurs distincts ; `opencode` écrit
lui-même un coût par message et n'expose que deux buckets. Non seulement aucun langage déclaratif
raisonnable n'exprime les deux, mais **un recalcul uniforme est impossible** : les buckets ne se
mappent pas. Le coût reste donc du code, avec deux formes légitimes — **rapporté** par le harnais, ou
**dérivé** par PDO (ADR-0034).

**Un trait fermé à N implémentations est écarté** : il interdit d'ajouter un harnais sans recompiler,
et ferait remonter les particularités de Claude Code (le sibling `.claude.json`, l'appariement de
transcript par répertoire de travail) dans l'interface que tous les autres doivent implémenter.

**Le résident est obligatoire, et le one-shot est refusé malgré son avantage.** En one-shot la fin de
tour serait **gratuite** — la sortie du process *est* le signal. On refuse quand même : un nœud doit
rester attachable et conversationnel jusqu'à son reap (ADR-0012), et en one-shot la mort de session
cesse d'être un verdict d'échec (ADR-0032).

**Pas de validation du descripteur** (ADR-0001) : PDO ne sait pas ce que le template *veut dire*. La
conséquence est connue — un descripteur sans flag d'autonomie donne un nœud arrêté sur un dialogue de
permission, que **rien** ne détecte depuis #469.

## Limites acceptées

- **Un harnais qui ne laisse pas imposer l'identité de session** ne peut être attribué que par
  répertoire de travail. Mesuré sur `opencode` : un id de session neuf répond « Session not found »
  et sort en 1 — le sélecteur *continue* une session, il ne la crée pas. Deux nœuds `doc-only`
  concurrents partagent le worktree du Run, donc leurs sessions se confondent : coût attribué au
  mauvais nœud, fin de tour lue sur la mauvaise session. Accepté en v1. Le levier mesuré n'est pas de
  relocaliser le store, c'est que ce harnais sert une **API HTTP locale** sur un port imposable au
  lancement : PDO peut créer la session lui-même puis s'y attacher. À vérifier avant de s'engager :
  le listing de sessions de cette API est **global**, pas scopé au port.
- **Le store d'un harnais n'est pas un contrat.** Mesuré : `opencode` 1.18.18 écrit dans un SQLite,
  et son ancien store JSON — encore présent, avec des mois de données — n'est plus écrit. Une
  capacité de coût branchée sur la forme *observée* du store lit de l'historique mort et rapporte
  zéro : la lecture passe par l'API du harnais quand il en a une.
- **Un modèle qu'un harnais ne peut pas honorer ne fait pas forcément échouer le lancement.** Mesuré
  : `opencode` avec un modèle valide mais injoignable **retombe en silence** sur son défaut et rend
  un tour vert sous un autre modèle. PDO ne peut donc pas compter sur un échec bruyant.
- **Le sandbox n'est plus hors périmètre** (amendé par ADR-0063) : chaque harnais first-party déclare
  un *staging set*, copié au spawn du premier nœud qui le résout ; l'image reste l'affaire de
  l'utilisateur et PDO dit une fois, visiblement, quand le binaire en est absent.
- **Un harnais déclaré sans capacité est utilisable mais aveugle** : il tourne, on l'attache, il
  complète ; il ne rapporte ni coût ni fin de tour.
