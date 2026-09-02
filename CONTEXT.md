# Prompt Driven Orchestrator (PDO) — Glossaire métier

Glossaire vivant : définitions, volonté produit, vocabulaire à éviter. Le contrat détaillé d'une
décision vit dans `docs/adr/` (renvois « ADR-NNNN »), l'implémentation dans le code, l'historique
dans git. Ce fichier ne porte que la vérité courante — jamais de changelog ni de détail
d'implémentation (voir `docs/agents/domain.md`).

---

## Pipeline

Un **Pipeline** est un DAG nommé, à **orchestration déterministe**, qui décrit l'enchaînement de rôles d'agents pour accomplir une tâche d'ingénierie.

- **Orchestration déterministe** : aucun *LLM-router*. Le routage entre nœuds suit des prédicats mécaniques portés par les edges conditionnelles (`when:`/`else`) et les régions de boucle du bloc `loops:` (ADR-0011). Aucun LLM ne décide à l'exécution quel nœud activer.
- **Pas de routage probabiliste** : le déterminisme porte sur la *structure d'orchestration* (qui appelle qui dans quel ordre), pas sur le contenu produit par chaque nœud (les LLM aux feuilles restent stochastiques).
- **Graphe modifiable pendant l'exécution** : la topologie n'est pas immuable. L'utilisateur peut éditer le graphe pendant qu'un Run tourne (ADR-0007) et le scheduler se réajuste au prochain tick. Les nœuds en cours d'exécution restent immutables (cf. *Édition pendant un Run*).
- **Multiples pipelines plutôt qu'embranchements** : pour gérer des trade-offs coût/complexité (ex. *quick-fix* vs *feature-with-adversarial-review*), on définit plusieurs pipelines distincts. Pas un seul pipeline avec des branches.

Contrairement à : Liza (pipelines YAML), Langgraph (conditional edges + LLM-router), TPM workflow (orchestrateur LLM qui décide quand spawner).

---

## Node

Unité atomique d'un Pipeline. Un **Node** représente un rôle. Un Node **`agent`** lance un **harnais agentique** (cf. §*Harnais agentique*) à qui on confie un prompt système qui définit sa mission (Implementer, Planner, Reviewer, etc.). Un node **`script`** exécute du **bash déterministe fourni par l'auteur**, sans LLM (ADR-0017).

Un Node se définit par :

- **Nom** — identifiant lisible affiché dans le canvas.
- **Prompt système** — le rôle, écrit dans la zone de texte qui s'ouvre à l'édition.
- **Ports de sortie — déclarés.** Un ou plusieurs documents produits, chacun un port nommé : c'est le **contrat de production** du Node (avec son schéma de frontmatter optionnel, cf. *Blackboard*). Multi-fan-out supporté. Rendu : un dot vert par document, drag-source des edges.
- **Ports d'entrée — émergents.** Un Node ne **déclare pas** ses entrées : elles sont *dérivées des edges entrantes*. Connecter `debugger.repro_steps` vers un Node y crée de facto une entrée `repro_steps`. Plusieurs edges de même nom **poolent** dans une seule entrée-liste — pooling **sémantique**, jamais un groupement visuel des flèches. Sur collision de noms *distincts*, on qualifie par source. L'accumulation cross-itérations est un flag `repeated` porté par l'**edge**, pas par l'entrée.

Asymétrie assumée : le Node *connaît* ses sorties, *découvre* ses entrées au câblage. Conséquence sur la bibliothèque : un Node réutilisable porte ses **outputs + rôle + type**, pas ses inputs (purement pipeline-spécifiques).

Distinct de :

- **NodeRun** — l'exécution d'un Node au sein d'un Pipeline Run précis. Un NodeRun = une session tmux d'un harnais agentique, avec un statut (pending/running/done/failed). Son isolation de travail dépend du Node.

### Isolation de Node

L'**isolation de Node** choisit où un NodeRun travaille : dans un sous-worktree propre, ou directement dans le worktree partagé du Run. Un Node `agent` est isolé par défaut ; partager le worktree du Run est un opt-out explicite de l'auteur du Pipeline. Un Node `script` partage le worktree du Run par défaut et peut opter pour l'isolation. Le choix reste écrit dans les deux cas : le Document de pipeline dit toujours où le Node travaille, sans le déduire de son type ou de son livrable attendu.

Un Node isolé peut travailler en parallèle sans partager son arbre de travail. Un Node non isolé évite le coût d'un sous-worktree ; s'il s'exécute en même temps qu'un autre Node non isolé, l'auteur accepte qu'ils partagent leurs modifications. PDO ne sérialise pas ce choix à sa place. À la complétion, PDO commite tout changement que Git n'ignore pas ; le dépôt cible porte la responsabilité d'ignorer les fichiers qu'il ne veut pas versionner. Deux Runs ne partagent jamais leur worktree, même lorsqu'ils exécutent la même Pipeline (ADR-0060).

Le Node ne porte aucune responsabilité Git particulière : il modifie les fichiers nécessaires. PDO livre les commits existants et crée un commit déterministe s'il reste des changements, avant que le Node soit déclaré terminé et que l'aval démarre.
_Éviter_ : « doc-only » et « code-mutating » ; « worktree par pipeline » pour le worktree partagé d'un Run.

### Harnais agentique

Le **harnais agentique** est le programme qui fait tourner l'agent d'un NodeRun (`claude`, `opencode`, …). PDO le lance, l'attache, le tue ; il n'en embarque et n'en fournit aucun.
_Éviter_ : « CLI » comme terme — réservé au binaire `pdo` (cf. *Prompt augmentation*, « capacités CLI ») ; « modèle » (le harnais est le programme, le modèle est ce qu'il appelle) ; « provider ».

- **Résident, jamais one-shot** : un harnais éligible reste vivant après la fin de son tour, dans une session attachable où l'utilisateur peut reprendre la main — c'est le principe même de PDO (ADR-0012). Un harnais qui sort en fin de travail est **inéligible** : sa mort de session serait indiscernable d'un échec (ADR-0032). Contrat d'éligibilité et forme du descripteur → ADR-0045. _Pour `copilot`, c'est le mode `-i <prompt>` (interactif, prompt auto-exécuté) qui tient ce contrat : `-p` sortirait en fin de tour (inéligible), et le prompt positionnel est refusé par le binaire (slot réservé aux sous-commandes, #615)._
- **Précédence à quatre tiers** : `node` → Run → Projet → Configuration d'instance → plancher (`claude`). Résolue une fois **au spawn** et gelée dans l'événement de démarrage du nœud : une édition de YAML ou de Projet n'atteint jamais une itération vivante (ADR-0007). Contrat → ADR-0046.
- **Épinglage ≠ paramétrage** : épingler un harnais sur un node dit « ce rôle exige ce harnais » et le soustrait à un changement de tier supérieur ; la carte des réglages par harnais dit seulement *comment* le node tourne sur chacun.
- **Instrumentation inégale, jamais silencieuse** : coût, résolution de transcript, substrat de fin de tour, détection de menu de limite, plancher de staging sont des **capacités** écrites harnais par harnais. Absente, la capacité rend la feature absente et le **dit** (« — », jamais `$0`) — ADR-0045.
- **Une capacité est un point de dispatch, pas une case à cocher** : elle nomme *quelle* implémentation PDO appelle, jamais seulement *si* on appelle celle de `claude` (ADR-0051). Un marqueur de capacité qui ne sert que de garde est un piège : ajouter une variante compile et ne change rien. Corollaire : `None` est une **valeur explicite** — l'absence est déclarée, pas déduite d'un dispatch manquant.
- **First-party ≠ déclaré** : un **harnais first-party** a ses capacités implémentées en code et embarquées dans le binaire ; un **harnais déclaré** n'a qu'un descripteur sur disque, donc il se lance, s'attache, se reprend, et se complète à la main — sans coût ni fin de tour. Les deux sont légitimes. La différence est **publiée** dans le *tableau de support* du README, **généré depuis le code** pour qu'il ne puisse pas mentir, avec la dernière version validée de chaque binaire. _Éviter_ : « harnais supporté » sans dire quelles capacités ; « non supporté » pour un harnais déclaré (il tourne).
- **Prérequis de harnais** *(terme)* : ce que PDO **suppose configuré** et ne configure pas — authentification, dossier de travail approuvé, version installée. PDO ne met en scène le home d'aucun harnais hors sandbox : c'est un prérequis documenté dans le README, pas du code. _Éviter_ : traiter un prérequis absent comme un bug de PDO ; « PDO installe le harnais » (il ne l'embarque ni ne le fournit).
- **Sessions d'infra** (Pipeline Manager, résolveur de merge) : elles suivent le harnais **du Run**, sans modèle ni effort propres.
- **Le binaire se résout dans le `PATH` de l'utilisateur, pas dans celui du service** : un harnais installé par un gestionnaire de paquets utilisateur (Homebrew, nvm) est invisible d'un daemon lancé par systemd. PDO enrichit son `PATH` depuis le shell de login plutôt que d'exiger un symlink (ADR-0055). _Éviter_ : « le binaire n'est pas installé » pour un binaire absent du `PATH` du daemon.

### Modèle et effort (par node, conditionnés par le harnais)

Le **modèle** dit *quel agent* tourne, le **niveau d'effort** *combien il réfléchit* : orthogonaux entre eux, mais **aucun des deux n'a de sens hors d'un harnais** — un slug Anthropic ne veut rien dire pour `opencode`. Ils vivent donc dans une carte du node, une entrée par harnais, et se lisent dans l'entrée du **harnais gagnant** ; ils n'ont pas de précédence propre (ADR-0046).

- **Texte libre, pass-through, aucune validation** : alias ou id complet, transmis verbatim. Pas d'enum fermé qui périmerait à chaque sortie de modèle — *sharp tool* (ADR-0001).
- **Le mode d'échec d'un modèle appartient au harnais, pas à PDO** : `claude` sort non-zéro sur un id invalide, donc le designer le voit ; `opencode` **retombe en silence sur son défaut** quand le modèle demandé est injoignable, et le nœud tourne *vert* avec un autre modèle que celui écrit. L'asymétrie « le modèle échoue fort, l'effort échoue en silence » (#268) n'est donc pas une propriété du produit — elle se vérifie harnais par harnais (ADR-0045).
- **Un harnais peut n'avoir aucun axe d'effort** — `opencode` ne l'expose pas au lancement. L'UI grise alors le picker : c'est une absence déclarée, pas un défaut.
- **Catalogue déduit, jamais écrit en dur** *(terme, ADR-0053)* : les modèles et les niveaux d'effort **offerts** pour un harnais sont **déduits du binaire installé** par le daemon, puis publiés dans `GET /settings` ; le client les affiche au lieu de les connaître. Un catalogue en dur périme à chaque release du harnais et fait proposer le vocabulaire de `claude` à tous les autres. La **valeur** reste du texte libre pass-through (ci-dessus) : on déduit ce qu'on **offre**, on ne valide pas ce qu'on **reçoit**. _Éviter_ : « liste des modèles supportés » (c'est la liste offerte par ce binaire, à cette version) ; dupliquer le catalogue côté client.
- **Le catalogue se lit d'abord dans la source *générée*** *(terme, ADR-0056)* : un binaire n'énumère pas forcément là où on regarde. Le catalogue a trois sources, par préférence décroissante — le **script de complétion** (`<binaire> completion bash`, généré à partir des choix que le CLI déclare, donc préféré), le **sujet d'aide des réglages** (`<binaire> help config`), puis `--help`. Chaque axe (modèles, efforts) appartient à la **source la mieux placée qui répond pour lui**. `--help` tourne en premier quand même, parce que c'est là qu'un CLI **déclare ses sous-commandes** : on n'exécute que celles qu'il annonce (`claude` n'a pas de `completion` et lit l'argv comme un prompt — la lancer à l'aveugle coûterait un timeout de cinq secondes dans une réponse `/settings`). Mesuré : le `--help` de copilot 1.0.80 énumère ses efforts mais décrit `--model` en prose — le lire seul faisait conclure « copilot n'a pas de catalogue » alors qu'il en a un (#629). _Éviter_ : « la source du catalogue » au singulier ; choisir la source par nom de harnais (l'échelle est harnais-agnostique).
- **Effort demandé ≠ effort obtenu** : le flag exprime une **intention** — un niveau non supporté retombe en silence, un plafond d'organisation peut clamper, un skill/sous-agent peut surclasser le niveau de session. À lire comme un levier de déterminisme et de latence, pas un cadran de coût. _Éviter_ : « effort garanti », « effort du run », « mode économique », « modèle global », « modèle du run ».
- **Sémantique, pas layout** : la carte entre dans le **diff sémantique** et le `content_hash` de la bibliothèque.
- **S'applique aux nodes qui lancent un agent** (`agent`, `merge`), jamais à un node `script`.
- **Ce que la reprise conserve dépend du harnais** : sur `claude`, le modèle survit, l'effort non — PDO le re-pose depuis l'événement de démarrage, jamais depuis le YAML courant, qui a pu être édité entre-temps (#424, ADR-0007). Le YAML est la vérité au *spawn*, l'event log au *resume*.
- **Défaut d'instance** (#347) : un modèle par harnais peut être posé daemon-wide (Configuration d'instance, ADR-0015).

### Profil agentique

Un **profil agentique** est un réglage d'instance nommé et réutilisable qui associe un harnais agentique obligatoire à un modèle et un niveau d'effort facultatifs. Son identité ne dépend pas de son nom, qui peut changer sans casser ses référents.

Là où PDO choisit aujourd'hui un harnais, avec ou sans modèle et effort, l'utilisateur choisit **Inherit**, un profil agentique nommé, ou **Custom**. Inherit poursuit la précédence `Node → Run → Projet → Configuration d'instance → Default` ; un profil nommé fournit sa combinaison complète ; Custom porte à ce tier une combinaison complète non réutilisable. Le pipeline distingue visuellement les nodes qui suivent un profil de ceux qui utilisent Custom.

Le profil **Default** est le plancher modifiable et renommable de l'instance, avec une identité réservée et sans suppression possible ; il vaut initialement `claude`, sans modèle ni effort. La référence à tout profil reste vivante jusqu'au démarrage du node, qui en gèle alors les valeurs pour cette exécution. Un profil référencé puis supprimé produit un avertissement et reprend la précédence au tier suivant.

Les noms de profils sont uniques sans distinction de casse. Un picker affiche le nom puis la combinaison `harnais - modèle - effort`, en signalant les valeurs absentes. Avant une suppression, PDO liste toutes les références encore vivantes ; les NodeRuns déjà démarrés n'en font pas partie.

Modifier un profil ne modifie ni ne salit les pipelines qui le référencent ; leur affichage reflète sa valeur vivante. Un profil absent apparaît dans l'avertissement global du pipeline et dans le picker qui porte la référence cassée.
_Éviter_ : « profil » seul (ambigu avec le profil de staging), « preset », « template », « copie du profil », « override manuel », « ancien mode » pour Custom.

### Node `script` — exécution déterministe (ADR-0017)

Un node **`script`** exécute le bash de l'auteur au lieu de lancer Claude, dans une **session tmux** attachable comme tout NodeRun (ADR-0005) : exit 0 ⇒ `completed`, non-zéro ou timeout ⇒ `failed`. Il partage le worktree du Run par défaut et porte le même choix d'isolation qu'un `agent` (cf. *Isolation de Node*).

- **I/O par variables d'environnement** (`PDO_INPUT_<PORT>`, `PDO_OUTPUT_<PORT>`, `PDO_ARTIFACTS_DIR`, `PDO_VAR_<NAME>`…) : un script ne lit pas le préambule prose. Il écrit lui-même ses outputs ; la validation d'outputs s'applique en **fail-fast** (pas de retry interactif — la session a quitté). Contrat de refus → ADR-0035.
- **Corps** stocké dans le slot prompt du node. Un corps vide fait échouer le lancement (fail-loud).
- **Ni `model` ni `effort`** (aucun agent lancé).
- **Sharp tool** : même surface de confiance que le guard de Trigger et le bash d'un agent — le bash de l'auteur dans son propre pipeline. Un script non isolé qui commit laisse l'arbre propre : responsabilité de l'auteur.

## Dataflow

Modèle (A) — **document-first, code en side-channel** :

- Les arêtes du DAG transportent **uniquement des documents** (artefacts markdown).
- Le **code** vit dans la branche du Pipeline Run. Quand un NodeRun isolé finit, son sous-worktree est mergé dans cette branche. Quand un NodeRun non isolé finit, ses changements non ignorés y sont committés directement. Un NodeRun isolé lancé ensuite fork depuis cet état.
- Les wires de l'éditeur = dataflow documentaire intentionnel. L'état du code suit en arrière-plan.

---

## Edges conditionnelles — le routage vit sur l'arête

**La condition de routage vit directement sur l'edge**, attachée à l'output port qu'elle quitte (ADR-0011 ; les nœuds `Switch`/`Loop`/`ForEach` sont supprimés). Le `review` d'un Reviewer va directement vers `implementer` (`verdict=FAIL`) ou `end` (`verdict=PASS`), chaque arête gardée, sans nœud intermédiaire.

### Forme

Une edge porte une clause `when:` **optionnelle**. Sans clause, l'edge est inconditionnelle.

### Évaluation — multi-match, pas d'ordre

À l'arrivée d'un artefact sur un output port, **toutes** les edges sortantes dont la clause est satisfaite **firent** — le flux peut fan-out vers plusieurs nœuds simultanément. Pas de `first-match-wins`. Si deux conditions se chevauchent, les deux branches partent : c'est voulu (ADR-0001, *sharp tool*) — le designer écrit des conditions disjointes pour un XOR, ou converge un fan-out de Nodes isolés via un `Merge`. Une edge **`else`** fire **uniquement si aucune edge sœur** (même output port source) n'a matché.

Feedback runtime : un nœud qui a firé passe au vert ; les edges déclenchées sont marquées sur le canvas.

### Champs référençables

- Tout champ de frontmatter de l'artefact quittant le port source.
- Toute variable pipeline `$<name>`.
- **`iter`** — le compteur de la région englobante (cf. *Loops*). Sert notamment à câbler une sortie d'épuisement (`iter: { gte: $max }`).

Prédicats : `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `in`, `not_in`. Pas d'eval libre, jamais de LLM-router. Plusieurs prédicats dans une clause sont **AND'd** ; pour OR, `in: [...]` sur un champ, ou plusieurs edges sœurs vers la même target.

---

## Loops — boucles matérialisées, nommées

Une boucle est une **entrée nommée du bloc `loops:`** du YAML, qui référence un ensemble de nœuds membres. Le mot *région* désigne son **rendu** sur le canvas (boîte translucide autour des membres). Les edges restent uniformes : **aucune edge n'est marquée « back-edge »**, son rôle est *dérivé* de la boucle, jamais stocké.

Pourquoi une identité nommée plutôt qu'une détection de cycle pure : « quelle edge est *la* back-edge » est une propriété topologique globale qui bascule quand on édite le graphe ailleurs. Un **id stable** sort cette identité de la topologie, stabilise la persistance du bound, et ouvre les boucles imbriquées. Cf. ADR-0011.

### Forme

- `members` : **liste explicite d'ids de nœuds, ≥ 1** (jamais spatial — déplacer un nœud hors de la boîte ne le retire pas de la boucle). Une boucle n'est pas nécessairement un sous-graphe : un seul membre est légal et fréquent (`collection` à un membre = un fan-out par item ; `bounded` à un membre = self-edge jusqu'à `max_iter`).
- **Entrée** = le membre ayant une in-edge depuis un non-membre. **Re-entry** = une edge d'un membre vers cette entrée.
- **Rendu** : boîte englobante (≥ 2 membres) ou marqueur compact (1 membre), header `↻ X/Y` ou `⇉ laps/total`, **en lecture seule sur le canvas** — id et `max_iter` se consultent/s'éditent dans l'**inspecteur de région**.

### Deux drivers

- **`bounded`** — driver = compteur `max_iter`. **Naît par auto-détection d'un cycle** (self-edge incluse) : id généré + `max_iter` par défaut, pour qu'un cycle ne soit jamais accidentellement non-borné. L'auto-détection vit **à la frontière du modèle, pas dans le geste** (#396, addendum ADR-0011) : le parse matérialise les régions manquantes, donc tout lecteur — éditeur, snapshot de run, jumeau de bibliothèque — voit la même boucle. Dérivée, jamais écrite : ouvrir un pipeline ne salit pas l'onglet et ne réécrit pas le YAML.
- **`collection`** — driver = `over: <field>`, liste lue dans la frontmatter de l'artefact entrant. **Naît par geste explicite** (clic droit → « Fan out over "<field>" ») : un fan-out parallèle n'a aucune signature topologique à détecter. Câblé live (ADR-0026) — **v1 mono-membre**. **Les laps sont réellement concurrents** : c'est la seule exception à « un nœud a au plus une itération vive » (garde de transition #212), bornée par la projection de région (région ouverte, nœud gouverné, `iter` dans les bornes). Hors de ces bornes le refus tient. _Éviter_ : sérialiser le fan-out pour « respecter » #212, ou élargir le garde à tous les nœuds (mesuré en #453 : la barrière ne firait jamais).

### Compteur d'itération

- **Par-boucle**, keyé sur l'`id`. Tout nœud **membre** estampille ses artefacts avec l'`iter` courant. Un nœud hors boucle n'est **jamais re-spawné par un lap** (#195/#199).
- **Résolution d'inputs** (canonique, #194/#210) : un input se résout vers **la dernière itération complétée** du nœud source — jamais l'artefact d'une itération échouée, jamais un alignement positionnel sur l'`iter` du consommateur.
- `bounded` : le compteur **incrémente quand une re-entry fire**, et l'entrée est re-spawnée **une seule fois par lap** même si plusieurs re-entries firent (coalescées). La barrière de lap dans un body multi-nœuds est le fan-in naturel du nœud de jointure.
- Adressage : `reviewer/iter-2/review/output.md`. L'accumulation (`repeated: true`) suit la même quarantaine : un artefact par lap **complété** du nœud source, ordonné par N — la résolution passe par la projection, jamais par un glob disque brut (#353).

### Sortie de boucle

- **Succès anticipé** : une edge forward conditionnelle quittant un membre (`verdict=PASS → end`).
- **Épuisement** (`bounded`) : à `iter = max_iter`, la re-entry est plafonnée. Le designer **peut** câbler une sortie d'épuisement (`when: { iter: { gte: $max } }`). Sinon, la boucle entre dans un état **bloqué « exhausted — unrouted »** explicite (jamais de stall silencieux), routable par le Pipeline Manager. Pas d'auto-proceed implicite.
- **`collection`** : **barrière** — les edges quittant la boucle firent **une seule fois, quand tous les items sont terminés**. Liste vide → barrière immédiate. Items isolés → chacun son sous-worktree, convergence via `Merge`.

### Imbrication — différée

Le modèle à id autorise de *déclarer* `inner ⊂ outer`, mais la sémantique d'itération imbriquée est **différée** : v1 = itération plate, un seul niveau.

### Édition pendant un Run & intra-Run

Supprimer l'edge qui retire le **dernier cycle** d'une boucle `bounded` déclenche un popup de confirmation ; confirmé, l'entrée `loops:` part avec son état. L'interaction avec un Run actif est régie par ADR-0007. Les compteurs `iter` repartent de zéro à chaque Run.

---

## Edges — structure

Une edge câble un output port source vers un input port target, et porte une clause `when:` optionnelle. La terminaison du Run passe toujours par un edge vers le nœud `End` mandatoire (#39).

### Routage — `mode` + `waypoints` (#154)

Le tracé d'une edge est **orthogonal**, auto-routé par défaut. `mode: manual` épingle un tracé via des `waypoints` absolus ; « re-route automatically » les efface.

`mode` + `waypoints` (comme `view` sur les nœuds) sont du **layout, pas de la sémantique** : ils persistent **dans le fichier pipeline** (le routage voyage quand un workflow est partagé) mais sont **exclus du diff sémantique** — deux pipelines ne différant que par leur layout comparent **égaux**. Le partitionnement layout/sémantique a un propriétaire unique côté frontend, miroité côté daemon avec des gardes d'exhaustivité (#154, #355, #395).

### Ancrage de l'edge entrante — `target_side` (#168)

Les inputs étant émergents, une flèche entrante atterrit **sur le corps** du nœud cible. `target_side` mémorise de quel côté (le plus proche du point de dépôt). C'est du **layout** : persiste dans le fichier, exclu du diff sémantique. Les ports **déclarés** gardent leur côté fixe.

---

## Note (note de canvas)

Une **Note** est une annotation de documentation **inerte** posée sur le canvas : un texte libre épinglé près d'un groupe de nœuds pour expliquer une intention. Elle **n'est pas un Node** — aucun type, aucun port, aucune edge, aucune session : le runtime l'**ignore entièrement** (ADR-0018).

- **Persistée dans un bloc racine `notes:`** du YAML (sibling de `loops:`/`edges:`, jamais dans `nodes:`).
- **`content` = texte brut en v1** — pas de markdown, pour ne pas ouvrir une seconde surface de rendu (ADR-0013/0018).
- **`view` = layout, pas sémantique** : deux pipelines ne différant que par leurs notes comparent égaux.
- **Mutable pendant un Run** : inerte, aucune session à orphaner — jamais rejetée sur un Run actif (contraste avec la suppression d'un node non-`pending`, interdite par ADR-0007).

_Éviter_ : « commentaire » (évoque un commentaire YAML `#` ou d'issue), « placeholder annoté » (qui est un **vrai** nœud `agent` produit par l'import de workflow, ADR-0016).

---

## Blackboard

Le **Blackboard** est le store partagé où vivent tous les artefacts d'un Pipeline Run, persistés et adressés par chemin.

- **Localisation** : `<pipeline-worktree>/.pdo/artifacts/`. Suit la branche du Pipeline Run. Part au cleanup **du worktree** — mais est d'abord copié vers le *Blackboard archivé* (cf. §*Cleanup vs archive*, ADR-0020).
- **Format** : markdown brut avec **YAML frontmatter** pour les métadonnées structurées. Le corps reste lisible humainement, le frontmatter est parsable par le runtime.
- **Wires** : un wire de `Node A → Node B` n'est pas un transport ; c'est une **déclaration de dépendance**. Le runtime traduit en : *« avant de lancer B, attendre que A ait posé son artefact ; l'input port de B le lit depuis le Blackboard »*.
- **Cycles** : chaque tour écrit dans un sous-dossier `iter-<N>/`.

**Blackboard archivé** *(terme)* : copie **durable et lecture seule** du Blackboard d'un Run (plus son `pipeline.yaml` + prompts), écrite sous `~/.pdo/runs/<run-id>/` (store global) à l'archivage, **avant** la suppression du worktree. C'est ce qui permet de rouvrir un Run `archived` et d'accéder à ses outputs. Sa suppression relève du `forget` (ADR-0020, ADR-0024).

### Schéma d'adressage

```
<pipeline-worktree>/.pdo/artifacts/<node-id>/iter-<N>/<port-name>.md
```

Résolution des inputs : wire simple → dernière itération **complétée** du nœud source ; wire d'accumulation (`repeated`) → un artefact par itération complétée, ordonné par N. La résolution passe par la projection, pas par un glob disque (#353).

**html** *(type de port de sortie)* : un port dont l'artefact est un `output.html` **rendu** dans une iframe sandboxée — HTML + CSS statiques, **aucun JS exécuté**, jamais servi en `text/html` par le daemon (ADR-0028). Surface de relecture, non consommée en aval en v1. _Éviter_ : « aperçu HTML interactif ».

### Frontmatter — minimal

La frontmatter sert au *runtime* (parser un verdict, router) — **pas** à structurer le contenu. Tout ce qui est destiné à être lu par un autre LLM reste dans le **corps** markdown. Pas de structures imbriquées ni de listes lourdes en frontmatter.

### Schéma déclaratif par output port

Un Node peut **déclarer le schéma de frontmatter attendu** sur chacun de ses output ports. Le runtime l'utilise pour (a) injecter une description précise dans le préambule et (b) **valider à la complétion**.

Types supportés v1 : `enum` (avec `allowed`), `int`, `string`, `bool`, `list` (de strings). Si un cas concret force plus, on étend.

**Pas de typage côté input** — l'agent fait du best-effort sur ce qu'il reçoit. Asymétrie volontaire : l'output est un contrat de production mécaniquement vérifiable ; l'input est un contexte que l'agent interprète.

### Validation à la complétion + fallback tmux

Quand un NodeRun signale `pdo complete`, le runtime valide la frontmatter contre le schéma déclaré. Si mismatch : **fallback** — un message est envoyé dans la session tmux du NodeRun (« corrige et retry »), le nœud reste `running` ; **1 retry max**, puis `failed`. Ce mécanisme évite de fail loud sur une erreur que l'agent peut corriger seul, tout en bornant la dérive. Formes de refus exactes → ADR-0035.

### Contrat de refus de la complétion (ADR-0035)

Une tentative de complétion (`pdo complete` ou *Mark complete*) a **quatre issues** : **Completed** (2xx), **NoOp** (2xx — doublon légal sur un nœud déjà terminal, aucun événement), **Refused** (**jamais 2xx** — slug d'erreur stable + `recoverable` disant si c'est encore le tour de l'agent), panne/cible inconnue (404/500/410). Le point de discrimination est le **slug**, jamais le statut HTTP. Les codes de sortie de `pdo complete` (0/3/4/1) sont un **contrat public** : un refus terminal (exit 4) ne doit **pas** enchaîner sur `pdo fail` (l'échec est déjà enregistré). Détail complet → ADR-0035.

_Éviter_ : « erreur de complétion » pour un refus récupérable (rien n'est cassé, c'est encore le tour de l'agent) ; « échec de `pdo complete` » pour un noop (c'est un succès).

### Avance détachée après transition terminale (ADR-0023)

Le 2xx de `pdo complete` (et `fail`/`skip`) signifie « ton événement terminal est durablement enregistré et l'avance est planifiée », **pas** « le run a avancé » — et, depuis ADR-0035, aussi « ta complétion n'a pas été refusée ». L'avance s'exécute sur une tâche détachée de la requête HTTP (le reap tue la session du client `pdo` lui-même — inline, l'avance était silencieusement perdue). Les erreurs de validation restent renvoyées in-request ; les erreurs d'avance surfacent via `RunFailed` + logs.

---

## Variables pipeline

Une pipeline déclare au niveau racine un bloc `variables:` — paires nom/valeur typées référençables dans toute clause `when:` via `$<name>`.

**Override au lancement d'un Run** : `POST /runs` peut inclure un objet `variables:` qui écrase les valeurs déclarées. Pas d'expressions calculées — uniquement des littéraux ; la logique reste dans les `when:`.

---

## Prompt augmentation — déterministe

Chaque NodeRun voit son prompt construit en deux couches :

1. **Prompt utilisateur** — le rôle, écrit par le designer du pipeline.
2. **Préambule runtime** — généré déterministiquement à partir des ports configurés, écrit par PDO à chaque NodeRun.

Le préambule contient au minimum : les **inputs disponibles** (nom du port + chemin sur disque), les **outputs attendus** (chemin où écrire + schéma de frontmatter requis), les **capacités CLI** (`pdo complete`, `pdo fail --reason` — pas packagées en skills : 100 % systématiques, sans bénéfice de progressive disclosure), l'**itération courante**, et les **variables pipeline résolues**.

**Contenu attendu** *(consigne d'output)* : consigne optionnelle attachée à tout type d'output produit par un node agentique. Injectée dans le préambule pour guider la production, elle n'est pas validée mécaniquement : la présence de l'artefact et sa frontmatter restent les contrats vérifiables. _Éviter_ : « prompt d'output », qui se confond avec le prompt principal du Node.

Conséquence : le designer n'a pas à se soucier dans son prompt de « où écrire / quoi mettre en frontmatter / comment signaler la fin » — c'est imposé par le runtime. Il se concentre sur le *rôle*.

### Skills et extensions — délégués au harnais

PDO **ne gère pas** les skills, sous-agents, plugins ou MCP d'un harnais. Ce qui est disponible dans une session NodeRun est ce que le harnais charge naturellement depuis son propre home et le repo cible. Pas d'attachement par-Node, pas de mécanisme custom.

---

## Placement d'un NodeRun

Où travaille un NodeRun est écrit sur le Node (cf. *Isolation de Node*, ADR-0060), jamais déduit de son type. Un Node isolé reçoit un sous-worktree forké depuis la branche du Pipeline Run et le merge dans cette branche à sa complétion. Un Node non isolé travaille directement dans le worktree du Run.

Parallélisation : les Nodes non isolés sont gratis-parallèles et partagent leur arbre de travail ; les Nodes isolés parallèles voient leurs branches mergées séquentiellement à la fin (ordre de complétion).

Le choix est **gelé au spawn du NodeRun** : une édition du Document déplace le prochain lancement, jamais une exécution vivante (ADR-0007).

### Merge-back d'un sous-worktree (ADR-0036)

Le merge-back suppose que le tip de la branche pipeline reste un **ancêtre** de la branche du nœud (fast-forward). Un nœud qui **se rebase** casse l'invariant. Le garde est **structurel**, keyé sur la **base de spawn** (`base_sha`) : si la divergence est l'histoire du Run réécrite par le nœud lui-même, le merge-back se **résout en faveur du nœud** (commit de merge, rien ne devient inatteignable, événement nommé — jamais silencieux) ; si la base est périmée ou inconnue, conflit + échec comme avant (résoudre perdrait du travail arrivé entre-temps). Aucun garde de **contenu** ne marche — mesuré : blobs, chemins et tree-sémantique refusent tous les trois l'occurrence qu'ils devaient sauver (ADR-0036 §3).

**À ne pas confondre** : cette **base de spawn (`base_sha`)** est le fork *sous-worktree ← branche pipeline*, et elle **avance** au fil des merge-backs des nœuds sœurs. C'est un axe distinct du **point de fork du Run (`fork_sha`)** (§*Parallélisation entre Runs*), qui est le fork *branche du Run ← branche source*, **figé** à la création et jamais avancé — la base de la stat LOC / du diff de Run (#417). Réutiliser `base_sha` pour la stat surchargerait silencieusement un terme critique au merge-back : ce sont deux forks, deux durées de vie.

---

## Merge — nœud first-class

Le **`Merge`** est un nœud first-class du DAG, isolé d'office et sans réglage, à placer explicitement par le designer (ADR-0006). L'utilisateur dessine la convergence ; le runtime ne l'invente pas.

### Forme

- 1 input port `branches: repeated` — accumule les branches **réellement firées** qui convergent.
- 1 output port `merged` — artefact résumé avec frontmatter `conflict_count`, `branches: [...]`.

### Sémantique runtime

1. **Barrière edge-centrée** (addendum ADR-0006) : le Merge est prêt quand toutes ses edges entrantes sont résolues — chacune a soit **firé**, soit est **morte** (producteur complété sans firer, ou lui-même mort) — et qu'au moins une a firé. Il consomme uniquement les branches firées. Un Merge dont toutes les branches sont mortes est lui-même mort et sauté tant que `End` reste atteignable ; sinon le Run **halt explicitement** (« unrouted »), jamais de stall silencieux.
2. **Fork** d'un sous-worktree depuis la branche du Pipeline Run, **`git merge`** de chaque upstream isolé.
3. **Si conflit** → spawn Claude Code dans le sous-worktree, qui lit le Blackboard pour reconstituer les intentions, résout, commit, écrit le `merged.md`.
4. **Si pas de conflit** → `merged.md` trivial, commit, sans LLM.

### Lint info-only

Un fan-out de Nodes isolés sans `Merge` downstream affiche un diagnostic info-only sur le canvas (ADR-0001 : pas bloquant). Le canvas est l'unique surface des diagnostics pipeline-wide (#63).

---

## Principe — Sharp tool, not safe tool

L'outil ne contraint pas l'utilisateur à dessiner des pipelines « sains ». Pas de validation prescriptive du graphe, pas de warnings paternalistes. Si une pipeline est foireuse, c'est la responsabilité du designer. PDO fournit des primitives nettes ; l'usage est libre. (ADR-0001)

Conséquences :
- Schéma déclaratif côté output uniquement ; pas de typage côté input.
- Pas de « lint pipeline » bloquant. Au max, un lint info-only.
- L'éditeur permet des graphes exotiques. Le runtime se débrouille ou halt explicitement.

---

## Principe — Deliberate, then autonomous (trust-earned)

PDO ne **démarre** pas en *set it and forget it* : la valeur initiale est dans le **temps passé en conception**, et le défaut reste délibéré (humain dans la boucle). Mais l'autonomie est une **cible atteignable, pas un interdit** : une fois qu'un pipeline a gagné la confiance de l'utilisateur, celui-ci **peut** le laisser aller jusqu'au bout — pousser, ouvrir une PR, merger — sans intervention.

Point clé : **l'autonomie est une propriété du *pipeline*, jamais une faveur du runtime ni du Trigger.** Le tool ne court-circuite jamais l'humain de sa propre initiative ; c'est le *designer* qui inscrit les actions durables dans le graphe. Conséquence : un pipeline auto-shippant se comporte à l'identique lancé à la main ou par un Trigger. La confiance se construit et s'audite sur le *pipeline*, pas sur le déclencheur. (ADR-0012)

Conséquences :

- **Tout NodeRun est attachable** en tmux ; l'utilisateur peut intervenir, converser, corriger.
- **Un Node peut être marqué `interactive: true`** : son NodeRun attend que l'utilisateur attache la session et signale la complétion.
- **Le Pipeline Manager** est conversationnel et permet de débloquer des Runs — pas juste de lire l'état.
- **Aucune action durable auto par le runtime lui-même.** PDO ne merge, ne PR, ne cleanup **jamais de sa propre initiative**. Si ces effets se produisent, c'est qu'un **nœud du pipeline** les exécute — choix explicite du designer, versionné, auditable.
  - **« auto-cleanup » vs « reapable surfacing » (#128)** : faire supprimer worktrees/branches par le runtime de lui-même = interdit (ADR-0012). **Exposer** les candidats sans rien supprimer = autorisé : le runtime *liste* (`GET /runs/reapable`, lecture seule), la suppression reste au pipeline/humain via `cleanup_run`. La recette `docs/recipes/disk-janitor.md` câble ce surfacing à un Trigger cron — l'autonomie reste *dans le pipeline*.
  - **Reapable run** *(terme)* : un Run **terminal** pas encore `archived`, dont le(s) worktree(s) existent encore. Son disque est récupérable via `cleanup_run`.

À distinguer de *Sharp tool* (ADR-0001) : *Sharp tool* parle de l'**éditeur** (on ne contraint pas le design). *Deliberate, then autonomous* parle du **runtime** (on laisse l'utilisateur *choisir* d'inscrire l'autonomie dans son pipeline).

---

## Édition pendant un Run

Le canvas est **toujours interactif** (ADR-0007) — un seul mode d'édition, qui s'adapte selon que la pipeline tourne ou pas.

### Modèle de mutation

- **Aucun Run en cours** : l'édition modifie directement la template en bibliothèque.
- **Run en cours** : l'édition modifie le **snapshot run-scope** (`<repo>/.pdo/runs/<run-id>/pipeline.yaml`) ET propage vers la template d'origine (auto-sync montant). Le watcher émet `PipelineModified` ; le scheduler se réajuste au prochain tick.
- **`PipelineModified` est un signal passif** (#221) : il ne ré-ouvre **aucun** Run terminal (intégrité de l'état terminal). Reprendre un Run terminé est une opération **explicite** (`resume_run`), jamais un effet de bord du watcher.

### Politique de mutation pendant un Run

- **Suppression** : interdiction stricte de supprimer un node de status non-`pending`.
- **Modif config** : le `max_iter` d'une boucle live peut être modifié à chaud.
- **Ajout de node + edge** : libre. Le scheduler pickup au prochain tick ; les nodes completed/running ne re-tournent pas.

### Étanchéité

Modif d'un run-snapshot n'impacte aucun autre run ; modif d'une template hors-Run n'impacte aucun run en cours ; l'auto-sync ne va que du run-scope vers la template, jamais l'inverse.

---

## Pipeline Run — cycle de vie

### Input

Un Run prend un **input unique** : du free-text, une référence d'issue, ou un mélange. Le runtime ne distingue pas : il pose le contenu tel quel dans `<artifacts>/_input.md`. Le nœud d'entrée (un Claude Code avec tous ses tools) se débrouille à partir de là.

L'input peut aussi être **construit interactivement** via un nœud d'entrée `interactive: true` : le user écrit un prompt brut court, attache la session, l'agent grille jusqu'à un input structuré.

- **Saisie persistante de la modale New Run** : le contenu saisi survit à une fermeture/réouverture et n'est vidé qu'après un lancement réussi.
- **Images d'input** : téléversables à côté du prompt, stockées dans `_input/` du Blackboard, listées dans le préambule du nœud d'entrée, affichées sur la carte Start et dans son inspecteur.

### `prompt_required` — pipeline runnable sans prompt

Flag racine du YAML, défaut `true`. Mis à `false`, le pipeline est *self-sufficient* : son nœud d'entrée sait trouver son propre travail (backlog, `git diff`…). Le champ prompt du New Run devient optionnel ; un prompt fourni est passé comme *additional info*.

### Termination

À la fin d'un Run réussi, **niveau 0** par défaut : la branche `pdo/run-<run-id>` reste en l'état, le worktree reste sur disque. PDO ne fait **pas** de PR auto, **pas** d'auto-merge. Si un projet veut ce comportement, il ajoute un nœud « Shipper » dans son pipeline.

### Échec / blocage

**Le runtime ne déclare jamais forfait** (ADR-0032 amendé, ADR-0049) : un incident infra ou un give-up runtime (mort de session, boot recovery, spawn-abort, validation d'output, conflit de merge, `unrouted`) mène à un état **récupérable non terminal** — `Interrupted` au niveau node, `AwaitingUser` au niveau run, avec la **raison** portée dans l'état — jamais à `RunFailed`. `Failed` ne vient que d'un `pdo fail` délibéré d'un agent ou d'un abandon humain. La branche et les sous-worktrees restent vivants pour debug. **Pas d'auto-cleanup, jamais.** L'utilisateur peut : cleanup manuel, reprendre la main sur la branche, débloquer via le Pipeline Manager, éditer le graphe à chaud, ou automatiser la récupération *dans un pipeline* — `GET /runs/reapable` *surface* (lecture seule) les Runs terminaux au worktree résiduel, et le pipeline **`disk-janitor`** (livré) + un Trigger cron exécute `pdo reap` (politique TTL graduée pure, `reap_policy`, ne rate jamais son propre Run sur un lot partiel) via `cleanup_run`. L'origine de la suppression reste *dans le pipeline*, jamais le runtime. Recette : `docs/recipes/disk-janitor.md` (#128, #480).

### Résilience d'un run (retomber sur ses pattes)

**`Interrupted`** *(statut de node, ADR-0049)* : un incident infra (mort de session, boot recovery, spawn-abort) met le node ici — « la session est morte, pas le travail » — sans terminaliser le Run (qui passe `AwaitingUser`). **Pas d'auto-retry** : atteindre cet état réclame un humain. Récupération : reprise de la session dans le worktree (harness-spécifique, ADR-0045) ou restart avec les artefacts partiels fournis en input (défaut). _Éviter_ : « node failed » (une cause infra n'est pas un échec métier).

**`recover_node`** *(terme, ADR-0049 §3)* : la commande **ciblée** qui récupère un node `Interrupted`. Elle choisit son mécanisme sur le harnais **gelé** du node : **ré-attache** de la session en place (`claude --continue`) si le harnais le déclare (`can_resume()`, ADR-0045), sinon **repli automatique** sur le **restart-avec-artefacts** (agent frais, la sortie partielle survivante fournie en input, jamais réécrite). La mécanique de reprise est partagée avec `GET …/pane` (`reattach_node_session`). _Éviter_ : `restart_node` (qui, lui, respawn toujours frais, sans essayer la ré-attache).

**Ré-ouverture** *(terme, ADR-0032 amendé)* : `terminal ≠ verrouillé`. Tout Run terminal — `Completed`/`Skipped` inclus — se ré-ouvre par une **re-projection** qui gèle les `(node, iter)` satisfaits et interdit leur re-spawn (anti-#221). Geste explicite de l'humain, jamais du runtime ni du watcher (`PipelineModified` reste passif). Le label terminal précédent reste dans l'event log. _Éviter_ : « reprendre = relancer » (on ne refait jamais le satisfait).

**`reopen_run`** *(terme)* : la commande **globale** de ré-ouverture (« re-projette + drive le nouveau »), surfacée par un bouton Play dans la toolbar de niveau Run ; la seule qui ne cible aucun node (utile sur un Run halted-`unrouted`). Les commandes ciblées (retry/restart/start/complete/inject) embarquent leur propre ré-ouverture, **en un temps** (atomique, plus de course de re-fail). _Éviter_ : `resume_run` (renommé), `retry_all` (archive + Run neuf, run-id différent).

**Skip local** *(terme)* : sauter un node **sans terminer le Run** ; output vide par défaut, surchargeable via `overrides` ; compte comme satisfait pour la re-projection ; l'aval avance. _Éviter_ : `pdo skip` / `RunSkipped` (no-op **run-level** qui termine tout, #245).

**`force_route`** *(terme)* : sortie explicite d'une région ou d'un node vers une cible qui **court-circuite les `when:`** des edges — débloque un verdict non-`PASS` (CI verte, MR mergeable) qui n'atteindrait jamais `End`. _Éviter_ : `end_region` (qui, lui, respecte les `when:`).

**`auto_fail`** *(réglage, ADR-0015/0046, ADR-0032 amendé)* : opt-in résolu **global < projet < run < node** qui laisse un `pdo fail` d'agent terminaliser directement en `Failed`. Décoché (défaut), un `pdo fail` d'agent parke le Run en `AwaitingUser` (l'humain confirme l'échec). Ne concerne **que** le `pdo fail` d'agent ; les give-up runtime parkent toujours. _Éviter_ : « autonomie » tout court (c'est un axe précis, pas un mode global).

### Parallélisation entre Runs

Plusieurs Runs peuvent tourner simultanément sur le même repo. Conventions anti-collision :

- Branche : `pdo/run-<run-id>`.
- Worktree pipeline : `<repo>/.pdo/runs/<run-id>/worktree/`.
- Sous-worktrees : `<repo>/.pdo/runs/<run-id>/nodes/<node-id>/iter-<N>/`.
- `<run-id>` = slug `<timestamp>-<short-uuid>`, lisible et unique.

**Point de fork du Run (`fork_sha`)** *(terme, #417)* : le commit d'où `pdo/run-<run-id>` est **coupé** à la création, **gelé** dans l'événement `RunStarted` (résolu contre le ref local, sans fetch — même posture d'immuabilité que `source_branch`, le harnais gelé et `RepoPin.sha`, ADR-0042/0030). C'est la **base stable en trois-points** de la stat LOC et du diff de Run : figé à la création, il ne peut plus être déplacé par un `HEAD` du checkout partagé qui erre ensuite (le bug #417). Absent (Run d'avant #417) ⇒ repli sur `source_branch`, puis `HEAD`. _Éviter_ : le confondre avec **`base_sha`** (la base de spawn *sous-worktree ← branche pipeline*, ADR-0036 — un autre fork, qui **avance** au fil des merge-backs) ; « HEAD » comme base (c'est précisément l'ancre errante que #417 supprime) ; « checkout partagé » comme référence de diff ; le `source_branch` **vivant** comme cible de diff (même classe d'instabilité que le bug — l'opérateur peut le rebaser/supprimer).

**Nom placeholder** *(terme)* : nom lisible posé par le daemon au spawn, déterministe et immédiat, garanti présent même pour un Run prompt-less. _Éviter_ : nom temporaire, titre par défaut.

**Nom descriptif** *(terme)* : nom posé best-effort par le Pipeline Manager dans son propre tour, une fois qu'il sait ce que fait le Run ; remplace le placeholder s'il aboutit, sans jamais le supprimer (un Run a toujours un nom). **Désactivable par-Run et par-Trigger** (défaut d'instance `default_auto_name`, résolu `stored → env PDO_DEFAULT_AUTO_NAME → true`, ADR-0015, #338) : désactivé, le Run garde son nom placeholder et le manager n'est pas instruit de renommer. _Éviter_ : nom final, rename automatique.

### Statistiques de Run

Quatre métriques dans le panneau d'info (#100, #272) :

- **Durée** : wall-clock entre `started_at` et `completed_at`, dérivée à l'affichage (jamais persistée), live tant que le Run est vivant. Un Run `Paused` continue de compter.
- **Sessions de nœud lancées** : compte **cumulatif** des démarrages de session, re-spawns inclus, manager exclu. À distinguer de la gauge « sessions vivantes » du cap.
- **LOC** : `git diff --numstat` en **trois-points** depuis le **point de fork du Run (`fork_sha`)** — jamais depuis le `HEAD` du checkout partagé (#417) —, stable même si `main` avance **et** si le checkout est garé sur une branche divergente, **exclut `.pdo/`**. Live-only : « — » pour un Run archivé (branche supprimée), à distinguer de « 0 » (diff vide).
- **Coût (est.)** : **estimation** — pas une facture — dérivée à la lecture des transcripts Claude Code locaux, jamais persistée. Le même fold attribué alimente le total du Run, l'en-tête du Node et Stats ; sa déduplication `(message.id, requestId)` porte sur le Run entier (ADR-0058). Un modèle non tarifé ⇒ borne basse signalée « † » (`partial`), et `unpriced_models` (#425) nomme les familles concernées. Survit à l'archivage (les transcripts restent). Table de prix → ADR-0034.

**Coût dérivé / coût rapporté** *(termes, ADR-0052)* : un coût **dérivé** est recalculé par PDO (tokens × table de prix résolue) — c'est le chemin de `claude` et il porte `~`. Un coût **rapporté** est celui que le harnais **compte lui-même** dans son unité de facturation, que PDO convertit par une **constante publiée** et **sans passer par la table de prix** ; il ne porte donc pas `~`. Les deux formes s'additionnent en dollars, mais un total de Run est **ventilé par harnais**. Un total **indisponible** n'efface pas la ventilation : c'est la **somme** qui est refusée, pas la connaissance (ADR-0052 §3, FP #617). _Éviter_ : recalculer un coût rapporté depuis ses tokens ; convertir en euros ; sommer des unités natives entre harnais ; lire une tranche comme une fraction de total.
**Table de prix embarquée / fetchée / manuelle / résolue** *(termes, ADR-0034)* : l'**embarquée** est le plancher compilé dans le binaire ; la **fetchée** est écrite par le daemon seul depuis models.dev, hors du chemin de lecture ; la **manuelle** est écrite par l'humain seul ; la **résolue** est la fusion **par clé de famille** avec précédence `manuel → fetché → embarquée`. Un écrivain par fichier ; rien n'est jamais seedé. Depuis #528, la **résolue** est aussi **exposée en lecture** sur `GET /stats/cost` (tableau `resolved` : tier gagnant + `$/MTok` par famille, rendu dans l'onglet Stats → Cost à côté de « Sync costs ») — additif, lecture seule, même `PriceTable` que le fold de coût, donc jamais divergente du tarificateur (#373). _Éviter_ : « surcharge de prix », « merge des tables », « prix seedés », « prix live » (la lecture est toujours locale).

### Statistiques d'instance (cockpit, #377)

Surface **Stats** superposée en pleine fenêtre, avec navigation latérale et période commune aux sections : agrégats **transverses** filtrables par période (runs/erreurs/sessions, fires par pipeline, coûts), à distinguer des Statistiques de Run. Sessions et Cost ventilent les harnais dans des barres empilées par période et dans un tableau hiérarchique Pipeline → Nodes ; Cost affiche total et moyenne, tandis que Sessions porte les nombres d'exécutions. Les noms visibles suffisent, l'interface n'expose pas leurs identifiants techniques. Une lecture secondaire reprend la même hiérarchie par Projet → Pipelines → Nodes ; un Run sans Projet nommé porte le nom de son dépôt primaire. Les sessions propres au Run forment la ligne **Infrastructure**, tandis que les subagents appartiennent au Node parent (ADR-0058) ; un coût impossible à rattacher reste dans **Non attribué**, jamais dans une catégorie inventée. Une exécution de Node est un démarrage, donc chaque tour de boucle et chaque redémarrage compte séparément dans la moyenne. La période sélectionne les Runs démarrés dans sa fenêtre et inclut toutes leurs exécutions et leur coût complet. Une moyenne de coût ne porte que sur les exécutions au coût lisible ; elle garde le préfixe `~`, et son détail donne la couverture. Le marqueur `†` reste réservé à une borne basse incomplète. Un harnais n'est absent du tableau que s'il n'a aucune exécution sur la période ; un coût inconnu reste visible comme « — », jamais comme `$0`. Les événements antérieurs au multi-harnais sont attribués à `claude`, seul harnais agentique alors disponible ; les Nodes `script` restent hors compte. Les couleurs sont stables par nom de harnais. Le calcul ne se rafraîchit pas en arrière-plan : l'utilisateur déclenche **Refresh** et voit l'heure du dernier calcul. Les tarifs vivent dans le panneau secondaire **Pricing details**, avec leur synchronisation, la table résolue et ses avertissements. Les agrégats sont dérivés à la lecture, **jamais matérialisés** (ADR-0029), et restent exprimés en USD.

### Diff de Run (surface de relecture)

Section **Diff** repliable du panneau d'info (#116, #376) — distincte de la stat LOC et du diff sémantique. Trois portées : **diff de Run** (trois-points depuis le **point de fork du Run (`fork_sha`)**, jamais depuis le `HEAD` du checkout partagé — #417 —, exclut `.pdo/`, mêmes bornes que LOC pour que « compté » et « montré » coïncident), **diff de nœud** (deux-points, connu-imparfait — la base fidèle par nœud est différée), **Run archivé** (« Diff not preserved for archived runs », à distinguer de « No changes »).

### Contrôles de Run (niveau Run)

Trois commandes agissent sur le **Run entier** — `pause_run`, `resume_run`, `retry_all` — à ne pas confondre avec le niveau **nœud** (boutons Start/Stop/Retry du canvas, et commandes du manager).

> **Amendé (spec résilience, ADR-0032 amendé / ADR-0049).** Le verrou « refus `409` sur un Run non vivant » est levé : `terminal ≠ verrouillé`, une action de reprise **humaine** ré-ouvre le Run **en un temps** (re-projection sûre), y compris sur un `Completed`/`Skipped`. `resume_run` devient `reopen_run` (levier global « re-projette + drive le nouveau », bouton Play) ; les commandes ciblées embarquent leur ré-ouverture. Voir § *Résilience d'un run*. Les bullets ci-dessous décrivent l'état pré-résilience.

- `retry_node` (UI) et `restart_node` (manager) ne sont **pas** synonymes : seul `retry_node` invalide l'aval (et se ré-itère à `iter+1`, table-rase) ; `restart_node` re-spawne le seul nœud au **même `iter`**. `stop_node` (UI) laisse le nœud `stopped` ; `kill_node` (manager) le marque `failed`.
  - **`retry_node` est conscient des boucles.** Dans une boucle bornée, l'`iter` d'un membre EST son index de lap, donc la table-rase générique se scinde : (1) un membre se ré-exécute **au même lap** (`current_iter`), pas à `iter+1` — sinon il forge un lap fantôme qui pousse la région vers son `max_iter` ; (2) la marche aval **saute les arêtes de ré-entrée** (member → entrée de région), pour ne réinitialiser que la tranche *avant* du lap et ce qui sort réellement de la boucle, jamais un membre amont du **même lap** déjà validé. Un nœud hors boucle garde la sémantique `iter+1` / aval complet.
- **Les deux surfaces de spawn par nœud refusent en `409` sur un Run non vivant** (#487) : `retry_node` comme `force_spawn_node` (bouton Start) répondent « resume the run first » plutôt que de spawner sur un Run terminal/pausé — un bouton de nœud ne flippe jamais le `RunStatus` (ADR-0009), le levier Run-level reste `resume_run`. Le refus prend la forme ADR-0035 §3 (`error` = slug, `recoverable`, prose dans `message`, plus `session_killed`), et `retry_node` route désormais son (re)spawn par la primitive de référence `spawn_node` (addendum #236 d'ADR-0009), qui porte garde/ cap/ sandbox et rend un `SpawnOutcome` véridique — fini le `200 {"ok":true}` inconditionnel. La sonde de refus est le **premier geste** du handler, avant le stop et l'invalidation, pour ne laisser **aucun** effet de bord (sinon l'auto-invalidation gèle le nœud en `pending`).
- **Pause / Resume** : `pause_run` fait passer un Run vivant en `Paused` (aucun nouveau spawn, l'horloge continue). `resume_run` est **dual-purpose** : sur un Run `Halted`/`Failed` il **relance** depuis l'état courant. C'est le seul levier qui ré-ouvre un Run failed ; il ne réanime jamais un `completed`.
- **Retry-all** *(terme canonique)* : sur un Run terminal, archive l'original puis crée un Run **neuf** avec les mêmes paramètres — sans référence de filiation, indiscernable d'un lancement manuel. _Éviter_ : « retry » tout court (réservé au niveau nœud), « relancer le même Run » (le run-id change).

## Repo cible (`target_repo`)

Le **repo cible** d'un Run ou d'un Trigger est le dépôt git dans lequel il travaille. Chemin absolu, stocké **verbatim** — jamais canonicalisé.

- **Obligatoire à l'écriture, replié à la lecture** (ADR-0033) : requis à toutes les frontières d'écriture (400 nommant le champ, avant tout effet) ; à la lecture, les enregistrements historiques sans cible sont résolus vers le `repo_root` du daemon. **L'asymétrie est volontaire et permanente** — ne jamais « symétriser » le côté lecture.
- **Racine du daemon (`repo_root`)** *(terme)* : le répertoire de travail du daemon. Racine de **stockage** (bibliothèque, pipelines, `pdo.db`) et **valeur de repli à la lecture** — plus jamais une cible de Run implicite (ADR-0033).
- **Un Trigger à dépôt nul est une référence pendante, pas un défaut** : refus en amont du guard (jamais lancé), Trigger **dormant**, « Run now » répond 409 avec la raison.
- **`effective_repo` (résolu) ≠ `target_repo` (brut)** : le brut reste la valeur saisie (badge, détail, pré-remplissage) ; le résolu ne sert qu'à la clé de regroupement des listes. On ne réécrit jamais le brut côté serveur. Le regroupement par repo (listes Runs/Triggers) n'apparaît que si ≥ 2 repos distincts ; sinon liste plate.
- **Repos récents** : projection à la lecture des cibles portées par les Runs, comparaison verbatim.
- **Branche source** *(terme, #571)* : le point de coupe d'un Run — la réf de branche depuis laquelle sa branche `pdo/run-<id>` est créée. Une branche **locale** (`feature-x`) ou une **branche de suivi remote** (`origin/feature-x`), stockée **verbatim** — on ne réécrit jamais ce que l'utilisateur a choisi. Optionnelle : absente, le HEAD du repo cible fait défaut. Résolue **sans fetch** : une réf de suivi vaut ce que le dernier `git fetch` de l'opérateur en a fait (même contrat que la base des secondaires, ADR-0042) ; la fraîcheur appartient à l'opérateur, jamais au daemon. _Éviter_ : « branche distante » au sens « réf vivante côté serveur » (le daemon ne parle jamais au remote), « pull automatique ».

### Multi-repo par Run (#465, ADR-0042)

- **Dépôt primaire** *(terme)* — celui sur lequel le Run écrit et mène ses MR ; `target_repos[0]`, sémantique de l'ancien `target_repo` (ADR-0033), reste dans `target_repo` (pas matérialisé en `RepoPin`).
- **Dépôt secondaire** *(terme)* — dépôt associé au Run en plus du primaire, offert aux nœuds via un snapshot figé à un SHA **au moment de l'ajout**. **Modifiable par défaut** (ADR-0047) : un nœud peut y écrire, y committer et l'y livrer (`gh pr create`, `git merge`) exactement comme dans le primaire — PDO ne livre jamais lui-même, c'est le nœud (cf. *Nœud `Ship It`*, ADR-0036) qui ship chaque dépôt indépendamment. La liste des secondaires est un **ensemble par-Run éditable en cours de Run** (ajout/retrait ; le primaire reste immuable). Ni worktree pipeline, ni archive/coût côté PDO ; en list/cost, le Run se range sous le primaire.
- **Read-only opt-in** *(terme, ADR-0047)* — case à cocher **par dépôt secondaire** (défaut **décoché** ⇒ modifiable). Cochée, elle rétablit l'ancien comportement : le dépôt n'est que du **contexte en lecture**, et écrire un fichier *suivi* dedans fait échouer la complétion (garde `secondary_repo_dirtied`, 409). Porté par le flag `read_only` de `RepoPin`/`TargetRepoInput` (défaut `false`). _Éviter_ : « secondaire read-only » comme s'il n'y avait qu'un mode — c'est désormais un axe par dépôt.
- **Visibilité au spawn** *(terme)* — contrat d'une édition mid-run de la liste : elle affecte les nœuds lancés **après** elle ; les nœuds déjà vivants gardent leur contexte (préambule + `PDO_SECONDARY_REPOS`) figé à leur spawn. Un ajout matérialise le snapshot à l'édition ; un retrait le sort de la projection mais laisse le snapshot sur disque jusqu'au cleanup (ne casse pas un lecteur vivant).
- **Snapshot secondaire** *(terme)* — worktree détaché pinné au SHA de `RunStarted` (ou d'un `RunReposEdited` pour un ajout mid-run), sous `<primaire>/.pdo/runs/<id>/repos/<alias>/` (**3e frère** de `worktree/` et `nodes/`). Injecté aux nœuds par **chemin absolu** (préambule + env `PDO_SECONDARY_REPOS`). Sur un secondaire **read-only** (opt-in), la garde `secondary_repo_dirtied` (409 sur fichiers *suivis*) fait respecter la lecture seule ; sur un secondaire **modifiable**, la garde est passée et le `.git` du dépôt est monté rw en sandbox pour que `git` y fonctionne (ADR-0047, étend ADR-0030). Récupéré au teardown par un **balayage disque** de `repos/*` (`worktree remove --force` **+ `prune`** ; sans le prune : registration dangling, classe #498), couvrant aussi les snapshots retirés-mais-persistants et orphelins.

### Explorateur de fichiers (générique, `GET /fs/browse` + `FsExplorerModal`)

La brique **unique** de sélection de chemin à la souris : un listing à **un niveau**, un composant, plusieurs consommateurs (sélecteur de repo, sélecteur de Dockerfile). Jamais de récursion, jamais de **contenus** renvoyés (noms seulement) ; liens cassés et fichiers spéciaux invisibles ; mode fichier = select-then-confirm. Surface non authentifiée comme tout le HTTP du daemon — portée LAN assumée (#260 closed). _Éviter_ : « repo browser » (généralisé en #431, sans alias).

---

## Projet

Un **Projet** est un regroupement **nommé** de dépôts qui se travaillent ensemble (un front et un back, par exemple). Il porte un nom éditable et une liste de chemins **membres**, comparés verbatim comme partout ailleurs (cf. *Repo cible*).

- **Un chemin appartient à au plus un Projet**, et le Projet d'un Run est celui qui possède son **dépôt primaire**. Un secondaire membre d'un autre Projet n'y change rien : c'est un contexte read-only, pas une appartenance (ADR-0042).
- **Matérialisé à la demande, jamais seedé** : tant qu'aucun nom n'est donné ni aucun réglage attaché, il n'existe pas de Projet — les listes se groupent sur le libellé dérivé du chemin. Nommer un en-tête de groupe est ce qui crée l'entité.
- **Premier réglage porté** : le harnais agentique, dont il est le tier intermédiaire (ADR-0046).

_Éviter_ : « projet » pour un dépôt seul (c'est le *repo cible*) ou pour le `projects/` d'un home stagé.

---

## Trigger

Un **Trigger** est une liaison nommée et persistée entre une **condition de déclenchement** et un **template de Run**. Quand la condition se réalise, PDO crée un Pipeline Run *ordinaire*.

- **Template de Run** = exactement la charge utile d'un `POST /runs`.
- **Start-only.** Un Trigger sait *quand* déclencher et *quel input* passer — rien de plus. Il ne décide jamais de la terminaison du Run. L'autonomie de bout-en-bout est une propriété du **pipeline** visé, pas du Trigger (ADR-0012).
- **Provenance.** Un Run créé par un Trigger porte `triggered_by` ; à part ça c'est un Run ordinaire.
- **Pas de chaînage interne.** Un Trigger ne déclenche pas un autre Trigger. Les pipelines se couplent par le **monde extérieur** (labels GitHub, etc.), jamais par un wiring interne PDO.

### Condition de déclenchement

Un Trigger porte un **heartbeat cron** (obligatoire) et un **guard script optionnel** :

- **Sans guard** : à chaque tick cron, le Trigger fire.
- **Avec guard** : le script tourne d'abord (cheap, avant tout spawn). Contrat : **exit 0 ⇒ fire ; non-zéro ⇒ skip**. **Le stdout du guard devient l'input du Run.** Exécuté avec CWD = repo cible, timeout dur configurable, hors du thread de tick (un guard qui hang ne gèle jamais le scheduler).

**Un firing = un Run.** PDO ne fan-out jamais un Run par work-item : si le guard ramène N issues, c'est *un* Run dont l'input les liste ; la multiplicité est gérée *dans le pipeline* par une boucle `collection`.

**Références cassées** : un Trigger dont le pipeline ou le repo a disparu ne fire plus et affiche un `last_outcome` d'erreur — pas d'auto-suppression, pas de pourrissement silencieux.

**Résolution de l'input**, dans l'ordre : stdout du guard → `input_template` statique → rien. Input vide + pipeline `prompt_required` ⇒ Trigger **rejeté à la création** (échec loud au config-time plutôt qu'un nœud d'entrée paumé toutes les 15 min).

### Idempotence — déléguée au monde extérieur

**PDO ne tient aucun état de dedup.** L'idempotence est une responsabilité de l'utilisateur (*Sharp tool*), naturellement satisfaite quand le pipeline **mute l'état qu'il poll** (relabel/fermeture d'issue) : le label GitHub *est* le registre de dedup. Risque assumé : un pipeline qui ne mute pas l'état qu'il poll ⇒ Runs dupliqués, bornés seulement par la politique de recouvrement.

### Politique de recouvrement — skip

Un Trigger **ne fire pas** si son propre Run précédent est encore vivant : le tick est sauté, pas mis en file. Ferme la fenêtre de course du dedup-par-label. Surchargeable en `allow` par-Trigger, avec plafond optionnel `max_concurrent` (#239) : les trois modes se ramènent à un plafond effectif unique (`skip` = 1, `allow` = illimité ou m). Le guard ne tourne jamais sur un tick qu'on sauterait. **Pas d'empilement de Runs en attente** — la seule attente possible est au niveau nœud (cap de sessions), dans un Run déjà admis.

### Mécanisme cron & cycle de vie

- **Cron 5 champs, interprété en UTC** (#222). Presets UI + expression brute.
- **Fires manqués = forward-only, pas de backfill** : daemon down pendant 50 slots ⇒ `next_fire` recalculé depuis *now*. Correct par construction : le dedup étant externe, un seul poll forward voit tout le travail accumulé. La ré-activation d'un Trigger saute le slot manqué (#372) ; la dé-pause globale rattrape le slot courant une fois, comme le boot (#348).
- **Résilience** : un panic de tick est isolé, la boucle survit ; `GET /triggers/health` rend le scheduler observable (pause ≠ mort).
- **Daemon best-effort par défaut, persistant sur demande** : `pdo service install` installe une unité OS (ADR-0019) ; la status-bar signale un daemon éphémère.

### Persistence

Les Triggers vivent dans une table SQLite (`~/.pdo/pdo.db`), pas en YAML : un Trigger est de la **config + état de scheduling**, réécrit à chaque tick — mauvais fit YAML. Ne viole pas l'event-sourcing : l'event log reste la vérité du **Run** ; un Trigger *produit* des Runs.

**Table `trigger_fires`** (audit) : un enregistrement horodaté par tick significatif (`fired` / `skipped-overlap` / `guard-exit-nonzero` / `guard-error`), keyé par Trigger. Répond à la question #1 du debug : « pourquoi mon Trigger n'a pas firé cette nuit ? ». Sur un skip de guard, PDO conserve stdout/stderr/exit code (plafonnés, queue conservée — l'erreur s'imprime en dernier) : purement diagnostiques, ils n'altèrent pas le contrat d'input. La provenance (`manual` vs `cron`) est une dimension orthogonale à l'outcome (#341).

**Trois journaux, trois questions.** PDO tient trois journaux disjoints, chacun pour une question de debug distincte : l'**event log** (« qu'est-il arrivé *dans ce Run* ? » — vérité event-sourced du Run, `run_id` obligatoire) ; **`trigger_fires`** (« pourquoi mon Trigger a-t-il firé, ou pas ? » — les évaluations de scheduling) ; et l'**`audit_log`** (« qui a changé la config de l'app, et quand ? » — les mutations *hors-Run* : create/patch/delete de Trigger et pause globale). L'audit log est le seul sans `run_id` : un geste de config n'appartient à aucun Run. Origine best-effort (`actor_hint`, falsifiable, jamais un gate — bind 0.0.0.0 sans auth). Contrat : ADR-0044.
_Éviter_ : « le log » sans qualifier lequel des trois.

### UI — onglet Triggers

- **Run now** (ADR-0027) = un **fire de première classe** partageant verbatim le chemin cron : guard exécuté, overlap honoré, ligne d'audit écrite, `next_fire_at` intact (le heartbeat possède le planning). Un skip guard/overlap est un 200 honnête (ADR-0025).
- **Tester le guard (dry-run)** (#350/#351) : exécute la commande de guard **sans aucun effet de bord** — aucun Run, aucune ligne d'audit. À distinguer de Run now (fire réel).
- **Trigger désactivé** (par-ligne) : grisé, ne fire pas ; la ré-activation repart au prochain slot.
- **Pause globale** (#348, canal **daemon-wide**, à ne pas collapser avec le `disabled` par-ligne) : un master switch court-circuite le tick sans muter le `enabled` de personne — à la reprise chaque Trigger retrouve son état gratuitement. « Run now » manuel fire quand même pendant la pause (chemin disjoint, derrière confirmation). Kill-switch opérationnel : quota API, vacances, debug.

---

## Pipeline Manager

Agent conversationnel attaché à un Pipeline Run. Permet de **lire l'état** et d'**émettre des commandes** sur le Run.

### Cycle de vie

- **Un manager par Run**, session tmux dédiée `pdo-mgr-<run-id>`, spawn au démarrage du Run, persiste jusqu'au cleanup (interrogation post-mortem).
- **Pas de polling actif.** Le manager ne tourne que quand l'utilisateur lui parle.
- **Nommage descriptif dans son propre tour** : quand un Run porte un nom placeholder, le manager pose un nom descriptif best-effort — jamais réveillé par le daemon (#184).

### Implémentation

Le manager **est** une instance standard du harnais du Run, prompt augmenté par le runtime (identité du Run, endpoints HTTP documentés en clair + exemples curl). **Pas de MCP custom** : on possède le prompt de la session, autant documenter les endpoints. Pour la lecture brute, bash complet — tout l'état du Run est sur disque.

### Commandes disponibles (v1)

Exposées comme `POST /runs/<id>/commands` :

| Commande | Effet |
|---|---|
| `bump_region` | Accorde N itérations de plus à une région bornée et relance |
| `end_region` | Complète une région bornée sans itération supplémentaire |
| `extend_cycle` | (Legacy, cycles hors région) — refusé sur un membre de région : utiliser `bump_region` |
| `resume_run` | Relance le Run depuis l'état actuel (post-conflit résolu, etc.) |
| `kill_node` | Tue un NodeRun en cours (le marque `failed`) |
| `restart_node` | Re-spawn un NodeRun au **même `iter`** ; sur un nœud isolé, le sous-worktree est réutilisé en place — le travail non commité survit (#489). Préconditions et refus → ADR-0037 |
| `mark_node_done` | Force la complétion (nœud `interactive`, ou récupération d'un failed corrigé à la main). Même corps que `pdo complete` : refus 409 nommé → ADR-0035 |
| `inject_artifact` | Pose un artefact à la main dans le Blackboard |
| `cleanup_run` | Supprime branches, worktrees, artefacts (archive d'abord — ADR-0020) |
| `rename_run` | Donne au Run un nom descriptif |
| `start_node` | Spawne un NodeRun immédiatement, hors ordre de dépendance (force-spawn). Refus `409` sur un Run non vivant / au cap / sandbox non prête → forme ADR-0035 §3 |

> La route UI jumelle `POST /runs/<id>/nodes/<node>/retry` (bouton Retry/Play) n'est **pas** une commande `/commands`, mais partage la même discipline de refus (#487) : sonde de tête « resume the run first » (`409`), puis (re)spawn par `spawn_node` avec un `SpawnOutcome` véridique — cf. § *Contrôles de Run*. Sa résurrection sœur `GET …/pane` d'une session morte passe elle aussi désormais par la porte d'admission et laisse une trace `NodeStarted` (#487 §3) — un clic ne contourne plus le cap de sessions.

L'effet de la plupart des commandes est l'**append d'un événement**. Trois font davantage — `inject_artifact` écrit un fichier, `cleanup_run` démonte du disque, `retry_all` archive et **crée un Run neuf** : rejouer l'event log ne défait aucune des trois.

### Contrat de réponse des commandes (ADR-0025, ADR-0035, ADR-0037)

Les commandes disent **la vérité sur leur effet** : cible inconnue ⇒ 400 avant tout append ; mauvais mécanisme ⇒ 409 orientant vers le bon ; valide mais sans effet ⇒ `200 {noop: true, reason}` honnête. La convention noop ne couvre que le *sans-effet* : un **refus** de complétion n'est jamais un 2xx (ADR-0035), et un **spawn demandé mais non advenu** non plus (ADR-0037) — le throttle d'admission répond `waiting`, pas `noop`. _Éviter_ : discriminer sur le statut HTTP plutôt que sur le slug d'erreur.

**Opération git interrompue** *(terme, #516)* : les marqueurs qu'une session tuée laisse dans le gitdir d'un sous-worktree (`index.lock`, `MERGE_HEAD`, `rebase-merge/`, `rebase-apply/`). Sur une réutilisation, `restart_node` les **inventorie tous** dans `interrupted_git_ops` (corps de réponse **et** préambule du nœud re-spawné, consigne différenciée) et n'en supprime **aucun** — PDO ne peut pas prouver l'écrivain mort (#485), l'agent frais résout. _Éviter_ : « verrou git périmé » / `stale_git_lock` (faux pour 3 marqueurs sur 4, et le nom masquait le second marqueur → `pdo complete` prenait un merge à 2 parents silencieux).
### Ce que le manager ne peut **pas** faire

**Spawner des sous-agents ad hoc hors-DAG.** Pas d'orchestration probabiliste émergente. Il peut force-spawn un nœud **déjà déclaré** dans le DAG (`start_node`). Pour une investigation profonde, l'utilisateur attache directement la session du nœud.

---

## Architecture runtime — event-sourced

### Source de vérité = event log

Toutes les transitions d'état d'un Pipeline Run sont des **événements append-only** dans une SQLite locale (`~/.pdo/pdo.db`). L'état courant d'un Run = projection des événements. Pas de « state.yaml » stocké en plus.

### Daemon PDO

Process local qui héberge le **serveur HTTP** (REST + WebSocket), est l'**ordonnanceur** (lit l'event log, spawn les sessions tmux + sous-worktrees, écoute les complétions), et sert d'**API surface** unique pour le manager, l'UI et tout futur client.

### Conséquence pour la prompt augmentation

Le préambule inclut **l'URL de base du daemon** pour les nœuds qui en ont besoin. Cette URL **n'est pas une constante** : elle dépend du côté où l'agent s'exécute (hôte vs conteneur) et passe par le résolveur d'URL du daemon — cf. *Sandbox*.

---

## Configuration d'instance (instance-wide config)

Réglages **daemon-wide** (ADR-0015), à distinguer d'une variable *pipeline* ou d'un override de Run. _Éviter_ : « préférences globales », « config » tout court.

- **Store** : table SQLite singleton (même justification que les Triggers : config + état mutable, mauvais fit YAML).
- **Réglages** : cap de sessions, reaper TTL, timeout du guard de Trigger, `default_model`, `default_sandbox`.
- **Précédence `stored → env → default`** : la valeur **stockée (UI) gagne**, l'env est un bootstrap. _Éviter_ : « l'env gagne » (rendrait la page no-op pour ses propres opérateurs).
- **Prise d'effet sans redémarrage** : tous les réglages sont lus frais — aucun `PUT` n'est no-op jusqu'au redémarrage.
- **Frontière** : « le manager vérifie périodiquement le pipeline » reste exclu — réveiller le manager depuis le runtime renverse *Pas de polling actif* et l'origine-de-l'autonomie d'ADR-0012.

### Règles de provisionnement

Les **règles de provisionnement** sélectionnent, avec la syntaxe de patrons de `.gitignore`, les chemins du dépôt primaire à rendre présents lorsqu'ils manquent dans les worktrees d'un Run. Elles sont facultatives et additives selon la chaîne Configuration d'instance → Projet → Run → Node ; une règle plus précise remplace le mode hérité, et `!` exclut entièrement un patron hérité. Deux modes visant le même chemin au même niveau sont un conflit nommé, jamais une précédence implicite. Contrat → ADR-0061.

Trois listes distinctes rendent le mode explicite : **copie** indépendante, **lien physique** aux inodes de la source, ou **lien symbolique** vers la source. Un patron commençant par `/` est ancré à la racine du dépôt ; aucun chemin ne peut sortir du dépôt. Le worktree du Run reçoit les trois premiers niveaux et le sous-worktree d'un Node reçoit aussi le sien. Un patron sans correspondance est un résultat vide normal.

La recette Instance + Projet + Run est résolue et gelée à la création du Run ; celle du Node l'est pour son itération. Le niveau Node ne s'applique qu'à un Node isolé, qui possède son sous-worktree. Le provisionnement n'a lieu qu'à la création physique d'un worktree : une réutilisation préserve strictement son contenu. Il fusionne récursivement les dossiers mais n'écrase rien : seuls les chemins absents sont ajoutés, un chemin déjà matérialisé par Git ou par une règle précédente reste intact, et une exclusion n'enlève jamais un chemin versionné. PDO ne déduit aucune invalidation d'un changement du dépôt ; une erreur de copie ou de lien refuse le spawn avant toute session plutôt que de livrer un environnement partiel.

La recette est gelée, pas les octets : chaque worktree lit la source courante au moment de sa création. Les liens symboliques déjà présents dans la source sont reproduits sans jamais être déréférencés. Une prévisualisation sans effet résout les règles contre un dépôt explicitement choisi, montre leur niveau, leur mode et le nombre d'entrées, et signale exclusions, conflits et patrons sans correspondance ; elle ne se rabat jamais sur le cwd du daemon.

_Éviter_ : « fichiers obligatoires » ; « `.gitkeep` » (ce fichier conventionnel n'a aucune sémantique Git) ; « chemin absolu » pour un patron ancré au dépôt.

---

## Sessions tmux

### Modèle d'exécution

Chaque NodeRun = **une session tmux détachée** créée par le daemon, contenant le harnais du nœud en mode interactif avec le prompt augmenté. Nommage :

- NodeRun : `pdo-<run-id>-<node-id>-iter-<N>`.
- Manager : `pdo-mgr-<run-id>`.
- Shell de run : `pdo-shell-<run-id>`.
- Assistant de bibliothèque : `pdo-libassist-shared` (une seule pour tout le daemon).

Les sessions sont invisibles par défaut, survivent au crash de l'UI ou du daemon (récupération au redémarrage).

### Shell de run — « Open session » (ADR-0021)

**Shell de run** *(terme)* : un **bash interactif ad-hoc** (pas une REPL Claude Code) spawné à la demande dans `pdo-shell-<run-id>`, cwd = worktree pipeline. Sert à inspecter/déboguer un Run post-mortem. _Éviter_ : « session » tout court (= NodeRun), « manager » (= REPL conversationnelle), « terminal » (= le pont d'attache).

Visible uniquement sur les Runs terminaux non-archivés dont le worktree existe (*reapable*). **Un seul shell par Run**, create-if-absent, persistant (sans TTL), tué par `cleanup_run`. Exempt du cap. `resume_run` le tue best-effort avant de ré-armer le scheduler (un writer concurrent casserait le merge). Détails → ADR-0021.

### Assistant de bibliothèque — copilote d'authoring (ADR-0048, amendé ADR-0051)

**Assistant de bibliothèque** *(terme, #302)* : une **REPL `claude`** design-time, **unique pour tout le daemon**, spawnée à la demande dans `pdo-libassist-shared`. L'utilisateur **décrit** un changement en langage naturel ; l'agent produit/édite le YAML (+ `<id>.prompts/`), montre un diff, et **écrit au save** via `POST /sessions/libassist/save` — sans id ni scope, le focus désignant le fichier (validation `POST /nodes/parse`). _Éviter_ : « manager » (= REPL attachée à un **Run**, `POST /runs/<id>/commands`) — l'assistant n'est attaché à **aucun Run** et n'émet **aucune** commande ; son seul effet est d'écrire un fichier template.

Keyé sur rien (pas un Run, pas une pipeline) : il y en a **un seul**, partagé par toutes les templates, donc son historique survit à un aller-retour entre deux pipelines. Exempt du cap. Prompt système primé (format YAML + endpoints), sans id de pipeline. Détails → ADR-0048, amendé par ADR-0051.

**Focus de l'assistant** *(terme, #594)* : la template que l'UI est en train d'éditer (id + scope), déclarée en continu au daemon et horodatée. Elle sert deux fois : injectée dans le contexte de l'assistant à **chaque message** de l'utilisateur (il se resitue sans qu'on le lui rappelle), et lue par le sweep comme **preuve de présence** de l'humain. _Éviter_ : « pipeline courante » (ambigu avec le Run sélectionné), « session active ».

Cycle de vie : spawné à l'ouverture de l'onglet **Assistant**, reapé quand l'utilisateur quitte **toute** vue d'édition de pipeline — au sens strict : quand plus **aucun** onglet de template n'est ouvert, pas quand l'onglet actif cesse d'en être un (aller voir un Run ne coûte pas la conversation). Le reap vide le focus par le même geste. Trois garde-fous, dans l'ordre : une session **attachée** n'est jamais tuée ; un **focus frais** n'est jamais tué (on édite, même sans l'onglet affiché) ; sinon le sweep la tue sur une TTL d'inactivité courte. Un reload ou une fermeture d'onglet ne peut donc plus la laisser vivre indéfiniment.

Le save de l'assistant **ne nomme ni id ni scope** : le daemon écrit dans le fichier que le focus désigne, et diffuse lui-même le `pipeline_changed` qui fait relire le canvas. Le mot *scope* signifie deux choses dans le code — `repo`/`user` sur un onglet d'édition pointent `.pdo/pipelines/`, le même mot dans le *library store* pointe `.pdo/library/pipelines/` — et faire porter ce mot par l'assistant lui faisait écrire un doublon dans le mauvais arbre en annonçant « Sauvé » (FP-6 de #594).

### Cap de sessions concurrentes (admission control)

Borne globale sur le nombre de **sessions NodeRun vivantes** — la ressource qui s'effondre réellement (tmux-collapse, #77/#78).

- **« Session vivante »** (#215) : un nœud `Running`/`AwaitingUser` dans un **Run lui-même vivant**. Un nœud session-holding dans un Run terminal est un artefact de projection, pas une session.
- **Admission par spawn de nœud**, pas par Run : au cap, le nœud passe **`waiting`** jusqu'à libération d'un slot. Le Run est admis immédiatement ; ce sont les nœuds qui s'étranglent.
- **Les sessions Manager ne comptent pas** (légères ; les compter risquerait un soft-deadlock).
- **Configurable** (Configuration d'instance), gauge dans la status-bar (ambre à l'approche du cap) — à ne pas confondre avec la stat cumulative « sessions lancées » d'un Run.
- **Admission atomique** (check-and-reserve sous verrou, #213), et **le slot qu'un spawn reprend ne compte pas contre lui** (ADR-0037 §8 — sinon un `restart_node` à cap plein se throttlait contre lui-même, gel définitif). Libérer un slot (`kill_node`) réveille les `waiting`.

### Cycle de vie process — résilience (fail-fast)

Posture **fail-fast pour ce que le runtime sait** : toute divergence constatée est rendue visible (`Failed` avec cause lisible), et le runtime **refuse de conclure sur ce qu'il ne sait pas** (ADR-0032).

- **Le seul verdict terminal de liveness est la mort de session** (ADR-0032) : la session tmux d'un nœud `Running`/`AwaitingUser` n'existe plus ⇒ `Failed` avec cause nommant la session. Exact par construction (le process agent est le leader du pane : il sort, la session meurt). Il n'existe **plus aucun seuil d'idle** : le proxy « sans progrès » (mtime, seuil 120 s) tuait des agents sains dès qu'un appel d'outil dépassait deux minutes — un faux positif coûtait un Run entier, un slot occupé est un coût borné et visible. Conséquence assumée : un agent **vivant mais wedgé** (prompt interactif, menu de limite, retries épuisés) garde son slot indéfiniment ; les leviers sont humains (`kill_node`, `restart_node`, stop). Le mot « stale » ne décrit plus qu'un statut historique.
- **Complétion automatique sur fin de tour** (ADR-0032 §2, **opt-in, décoché par défaut**) : le seul chemin par lequel le runtime peut terminer l'itération d'un nœud *vivant*. Un agent qui a fini sans appeler `pdo complete` reste dans le REPL, vivant et immobile, avec une signature positive dans son transcript. Deux gardes obligatoires : tour terminé **et** outputs valides (couvre l'agent qui termine son tour pour *poser une question*). Signal illisible ⇒ « au travail », on ne touche à rien. Passe par le même chemin que `pdo complete` (merge du sous-worktree compris) ; l'événement dit que la complétion est automatique. Application directe d'ADR-0012 : une action durable initiée par le runtime se mérite. Livrée par **deux substrats** gatés sur le même réglage — un hook `Stop` côté agent (`pdo complete --auto`, primaire) et le balayage daemon (repli), idempotents entre eux (ADR-0043).
- **Reap sur état terminal** (#205) : à l'entrée d'un état terminal, un **snapshot du pane** est persisté (il survit à la suppression du sous-worktree), **puis** la session est tuée. Invariant : au plus une itération live par nœud côté tmux.
- **Balayage d'orphelins** (ADR-0038) : périodiquement et au boot, le daemon tue les sessions de son socket dont le Run est **absent** du log, **archivé**, ou dont l'itération a complété au-delà du reaper TTL (nœuds seulement : ni Manager ni shell n'ont de TTL). Le verdict « absent » est rendu sur un log lu **après** l'inventaire — l'ordre inverse a tué des sessions fraîchement spawnées sous un `session_died` crédible. L'ordre repose sur un invariant qui appartient au **spawn** : aucune session n'existe avant que l'événement qui la réserve soit durablement enregistré ; tout nouveau chemin de spawn doit réserver avant de spawner. Le sous-worktree d'une session tuée **survit** (collision au respawn → #498).
- **Recovery au boot** : réconciliation de l'état persisté avec le monde process réel — nœud vivant sans session ⇒ `Failed` avec cause ; branche mergée sans complétion correspondante ⇒ divergence signalée, jamais complétée en silence ; nœud session-holding dans un Run terminal ⇒ réconcilié (#215).
- **Réconciliation run-level** (#214) : un Run `Running` sans aucun nœud vivant ni action possible est un **stall silencieux**, réconcilié vers `Failed` avec une cause nommant le(s) nœud(s) bloquant(s) — au boot et à chaque balayage. Garde-fous : une région de boucle ouverte n'est jamais auto-failée ; `AwaitingUser` attend un humain. **Nuance à ne jamais collapser** : le « nœud vivant » du *stall* inclut `Waiting` (un throttlé avancera) ; la « session vivante » de l'*admission* l'exclut (un `Waiting` ne tient pas de session). Deux prédicats distincts : « tient une session » ≠ « peut encore progresser ».
- **Blocage sur menu de limite d'usage** (#290) : la session est vivante mais coincée sur un menu interactif — invisible pour la sonde de liveness. Le détecteur lit le pane et émet un événement **informationnel** (le nœud reste `Running`), best-effort (l'ancre textuelle dérive avec les versions de CC). La récupération automatique reste une décision humaine différée (ADR-0012).

### Pont UI ↔ tmux : terminal inline xterm.js

ADR-0005. **Terminal interactif inline** dans le panneau de détail du nœud (WebSocket ↔ PTY ↔ `tmux attach`), bidirectionnel, temps réel. **Détacher** vers une fenêtre OS native reste un fallback opt-in (escape hatch), jamais le chemin principal.

- **« Agrandir » est toujours un geste utilisateur explicite** (#270) : ni la sélection d'un nœud ni l'auto-snap n'agrandit d'eux-mêmes.
- **Trois états d'affichage** (#346) : `split` (défaut d'une node vivante), `agrandi` (geste), `réduit` (défaut au clic sur une node à session terminée — les outputs priment). Défaut au **montage**, pas de repli réactif.
- **Une itération reapée se lit, elle ne s'attache pas** (#617) : sur une itération terminale (`completed` / `failed` / `stopped` / `stale` — le même ensemble que le `iter_is_terminal` du daemon), le terminal **n'ouvre aucun socket**. Il lit `GET …/pane`, écrit le **snapshot** figé au reap dans le même xterm, l'annonce comme tel et retire « détacher » (il n'y a plus rien à quoi s'attacher). Attacher un PTY sur une session déjà reapée, c'était mettre le `can't find session:` de tmux en surface principale de toute node terminée, pendant que l'endpoint servait le snapshot à personne. L'itération lue est **celle affichée** par l'IterSelector, pas le rollup du nœud : une vieille itération d'un nœud reparti est reapée elle aussi. Décidé au montage et à chaque **changement de nom de session** — une node qui se termine sous les yeux garde son buffer live (le snapshot peut n'être pas encore figé), un retry rebascule sur le chemin live.

Multi-client par session : gratuit côté tmux. Sécurité : un contrôle d'`Origin` garde **les deux** WebSockets (le terminal PTY *et* le flux d'événements du dashboard) contre le DNS-rebinding / CSWSH. L'allowlist par défaut est loopback (`localhost`/`127.0.0.1`) et **s'étend** par configuration pour un déploiement derrière reverse-proxy / domaine public — posture « Mono-user, local » inchangée (le daemon reste sans auth ni TLS ; le proxy les porte).

### Nœuds interactifs — signal de complétion

Un Node `interactive: true` spawn une session normale et **n'auto-complète jamais**. La complétion est signalée **depuis l'UI** par un bouton « Mark complete » (pas de slash-command in-session : le bouton reste accessible sans être attaché). Les artefacts présents sur disque sont alors pris tels quels — le préambule le dit à l'agent et au user.

Le bouton **n'est pas une garantie** : il est gaté sur le seul statut, et le garde autorise explicitement « mark complete sur un nœud failed corrigé à la main » comme chemin de récupération. Le clic peut donc être **refusé** (409 nommé, affiché au niveau du bouton) — cf. ADR-0035.

---

## Sandbox (exécution isolée d'un Run)

Modèle d'exécution (conteneur, mounts, réseau, uid) → **ADR-0030**. Profils de staging et garanties → **ADR-0031**. Ici : le vocabulaire.

**Sandbox** :
Propriété **par Run**, **immuable après création**, gelée dans l'événement de création. Valeur `off` **ou un nom de profil de staging** (`minimal` et `full` en sont les deux défauts virtuels) : `off` = sessions sur l'hôte (défaut) ; tout autre nom ⇒ toutes les tails du Run s'exécutent dans un conteneur dédié. C'est une propriété de l'**environnement d'exécution**, jamais de la sémantique du pipeline (le YAML reste intouché). Un nom de profil inconnu **échoue fort** partout, jamais de retombée silencieuse. La source du mode suit la précédence **choix explicite du Run → défaut par-Trigger → défaut d'instance** ; `off` reste le plancher.
_Éviter_ : « mode conteneur », « isolation » seul ; « copy »/« pure » (vocabulaire mort, aucun alias) ; confondre avec l'attribut `sandbox=""` de l'iframe du port `html` (ADR-0028) — sans rapport.

**Staging dir** :
Répertoire par Run sous `~/.pdo/sandbox/<run-id>/`, créé au démarrage d'un Run sandboxé, purgé à `cleanup_run`. Héberge le *staged Claude home*, un `.claude.json` sibling et les exceptions `$HOME` du profil. Le vrai `~/.claude` n'est **jamais** monté — ce sont toujours des **copies** qui sont montées. _Éviter_ : « home copié », « sandbox dir » (collision avec la racine `~/.pdo/sandbox/`).

**Staged Claude home** :
Le sous-répertoire du staging dir qui tient lieu de `.claude` aux sessions du Run. _Éviter_ : « fake home », « home miroir ».

**Profil de staging** (ADR-0031) :
Liste **nommée** de ce qu'un Run sandboxé stage dans son home — plus son **env** et sa **source d'image** : un profil décrit le home, l'env **et** le conteneur. Ce qui est stocké est un **diff** d'intention (`disabled`/`extras`), jamais un instantané — sinon une install ne verrait plus les évolutions futures du défaut. Le nom **et** la liste résolue sont **gelés au lancement du Run** : la préparation lit l'état du Run, jamais le réglage vivant. Pas de rename en v1 (le nom est aussi la valeur stockée par ses trois consommateurs). _Éviter_ : « allowlist » seul, « preset », « template » ; croire que le profil ne décrit que le home.

**Plancher de staging** :
Les **garanties** tenues quel que soit le profil : credentials valides, managed settings de l'org consentis, bypass permissions accepté, confiance pré-accordée à la racine du Run, `projects/` vide. Chaque garantie est satisfaite par une **copie** de l'hôte ou une **synthèse de repli** — c'est ce qui rend le décochage d'une entrée sûr sans l'interdire. Trois garanties ne sont pas des entrées du tout : affichées en lecture seule, refusées même en extra. _Éviter_ : « fichiers obligatoires », « liste verrouillée » (formulé en fichiers verrouillés, le plancher se contredirait).

**Entrée de profil / exception `$HOME`** :
Un chemin **relatif à `$HOME`** (`.claude/skills`, `.gitconfig`, `.config/gh`). Une entrée hors `.claude` est **copiée puis montée** — jamais un bind direct du fichier hôte (un agent ferait `git config --global` et réécrirait le fichier de l'utilisateur). Une entrée absente de l'hôte est loggée et sautée : l'échec dur ferait dépendre la politique de qui a tapé le chemin, et sur une instance à Triggers horaires, désinstaller un outil tuerait chaque tir. _Éviter_ : « fichier monté » (c'est une copie qui est montée), « exclusion » pour un décochage.

**Cycle de vie du staging** :
`prepare` seede le staged home depuis la liste gelée du profil (plancher tenu dans tous les cas ; symlinks échappants déréférencés ; walk best-effort) → les sessions tournent → **merge-back** : à la transition terminale du Run **et** à `cleanup_run`, seuls les **transcripts** (`*.jsonl` de `projects/`) sont recopiés vers `~/.claude/projects/`, sous le même dirname encodé, de façon **idempotente** — aucune autre écriture ne revient vers l'hôte → `teardown` purge le staging dir. Le coût et la veille lisent les transcripts **du staging** tant qu'un Run sandboxé est vivant. Le staging n'est purgé qu'au `cleanup_run` (dette disque à surveiller). _Éviter_ : « sync », « flush » pour le merge-back ; « cleanup » pour le teardown (réservé au niveau Run).

**Image sandbox (`pdo-sandbox:h-<hash>`)** :
L'image Docker d'un Run sandboxé. Identité **adressée par contenu** : le tag est le hash du contenu du Dockerfile (pas une version), le nom celui de la **variante**. C'est ce qui rend une image tirée d'un registry et une image buildée localement **interchangeables sous le même nom**. Le provisionnement est **build-si-absent, pull-d'abord** quand la source est le registry (fallback build si le pull rate). _Éviter_ : « image latest », « tag de version », « image du conteneur » (l'image n'est pas le conteneur).

**Variante d'image** :
Un nom d'image distinct pour un Dockerfile outillé différemment, dérivé de son nom de fichier (`Dockerfile.chrome-dev` → `pdo-sandbox-chrome-dev`). Une variante est **autonome** (steps dupliqués, jamais un `FROM` de la base — le tag doit rester le hash de ses propres octets) ; l'image de base reste minimale ; la sélection se fait **par profil**. _Éviter_ : « image chrome » (c'est une variante nommée), ajouter du tooling à la base.

**Dockerfile embarqué / seedé / résolu** :
L'**embarqué** vit dans le binaire ; il est **seedé** au premier usage à `~/.pdo/sandbox/Dockerfile` puis **jamais écrasé** (copie de référence éditable) ; le **résolu** est celui réellement hashé et buildé — le seedé par défaut, ou celui qu'un profil (ou l'env) désigne. Un Dockerfile pointé doit être **auto-porteur, sans `COPY`/`ADD`** (contexte de build vide). _Éviter_ : confondre les trois.

**Source d'image d'un profil** (ADR-0031 §9) :
Ce qu'un profil peut poser pour décider quel conteneur ses Runs obtiennent : un **Dockerfile choisi** (adressage par contenu) ou un **ref registry libre** tiré tel quel. Un ref explicite sort de l'adressage par contenu : pas de repli build, un pull raté = erreur dure nommant le ref. Gelée par Run. PDO **ne vérifie pas** que l'image contient `claude` : responsabilité de qui la fournit. Ne rien poser ⇒ le défaut (registry sur le Dockerfile seedé), l'env pouvant l'override (échappatoire headless). _Éviter_ : « override d'image » (c'est un tier de précédence), « le réglage d'image » (retiré de l'écran Settings — un axe par écran).

**Conteneur sandbox (`pdo-sbx-<run-id>`)** :
Le conteneur **unique et long-vécu** d'un Run sandboxé, nommé d'après le run-id, dormant ; toutes les tails y entrent par `docker exec`. Naît au premier nœud, meurt au `cleanup_run` — un nom par-Run rend kill et destruction ciblés. _Éviter_ : « la sandbox » (= la feature), « VM ».

**Identity mounts** :
Les bind-mounts qui **répliquent l'identité de l'hôte** : repo cible monté rw à son chemin absolu hôte, staged home → `$HOME/.claude`, `.claude.json` sibling, binaire `pdo` ro dans le PATH — plus l'uid/gid hôte adoptés par le process. C'est ce qui garantit le **même chemin de travail des deux côtés**, donc le même dirname de transcripts pour le merge-back. _Éviter_ : « volumes » seul.

**Injection d'identité** :
Le geste post-démarrage qui donne à l'uid hôte une **identité nommée** dans le conteneur (append à `/etc/passwd`/`/etc/group` derrière une garde, idempotent, best-effort). Ferme le défaut mesuré : en `--user` numérique inconnu de l'image, `sudo` abandonne avant NOPASSWD (« you do not exist in the passwd database ») et l'agent perd `apt install`. La prescription initiale (bind-monter des fichiers passwd générés) est **mesurée cassante** en `:ro` comme en `:rw` (ADR-0030). _Éviter_ : « créer un utilisateur » (le conteneur tourne toujours en `--user` numérique), « identity mount » (pas un mount).

**Résolveur d'URL du daemon** :
La fonction **unique** qui rend l'URL du daemon telle qu'elle est joignable **depuis le côté où l'agent s'exécute** (`localhost` côté hôte, la gateway côté conteneur). Ses deux consommateurs sont l'env du conteneur **et** le texte du préambule manager — c'est ce qui interdit la dérive « env résolu, prose en dur » (un manager sandboxé qui déclare le daemon mort, #447). _Éviter_ : « unifier les occurrences de localhost » (certaines sont légitimes côté hôte).

**Marqueur de session (`PDO_SBX_SESSION`)** :
Variable d'environnement posée sur chaque `docker exec`, héritée par toute la descendance : la clé du **kill ciblé** — arrêt du seul arbre de process porteur du marqueur, dans le conteneur partagé ; les sessions sœurs du même Run survivent. Nécessaire car tuer le client `docker exec` côté tmux ne tue pas le process conteneur. _Éviter_ : « tag » (réservé à l'image), « label ».

**Précédence du mode (`effective_sandbox`)** :
Résolution **pure**, une fois, au point où tous les chemins de création de Run convergent : choix explicite du Run → défaut par-Trigger → défaut d'instance → `off`. Le défaut par-Trigger est clearable (revient à l'héritage). Le sélecteur du New Run propose « Use instance default » qui **nomme** le défaut résolu au lieu de le recopier dans le champ (un prefill async ratait sa fenêtre et posait un `off` explicite jamais choisi, #452). _Éviter_ : « merge des modes » ; dire que le dialogue « choisit toujours » un mode.

**Sonde Docker** :
Check de disponibilité Docker côté hôte, TTL-caché, **advisory** : grise les options de profil et refuse le Launch quand le mode effectif en demande un — mais ne **clampe jamais** la valeur (écrire un verdict métier dans le champ le rendait indistinguable d'un choix utilisateur). Le fail-fast du run-advance reste le gate autoritaire. _Éviter_ : « sandbox available » (ambigu avec le mode).

**Préparation du sandbox (`sandbox_prep`) et précondition de spawn** :
État additif projeté sur le Run (`pending`|`ready`) rendant visible la fenêtre de prep eager (bannière + badge). Porte la **précondition de spawn** : un Run sandboxé dont la prep n'est pas `ready` n'est **pas schedulable** — le refus n'écrit rien, ce qui permet le **rejeu** par l'avance qui suit la fin de prep. Sans elle, tout chemin d'avance concurrent lançait un `docker exec` sur un conteneur inexistant → faux `session_died` (#445). Une grâce couvre la durée de prep dans la détection de stall ; une prep dont le Run est devenu terminal est abandonnée. _Éviter_ : « statut preparing » (pas un état de la machine à statut) ; « événement purement informationnel ».

### Ambiguïté signalée

« sandbox » désigne deux choses : (1) cette feature (exécution conteneurisée d'un Run) ; (2) l'attribut `sandbox=""` de l'iframe du port `html` (ADR-0028). Le contexte tranche ; ne jamais fusionner.

---

## UX — un seul mode d'édition unifié

PDO est un **atelier de production de code** ; la conception de pipelines est un *moyen*, pas le centre de gravité. Un seul mode, le canvas est toujours interactif (ADR-0007). Source visuelle de référence : [`docs/design/`](./docs/design/).

### Layout 3 panneaux

Liste (Runs / Triggers / Library) à gauche, canvas au centre, détail du nœud sélectionné à droite. La section « Archived » de la liste des Runs est un regroupement de **vue**, jamais un delete.

### Toolbar — bouton info pipeline

L'icône `i` ouvre un panneau **info pipeline** : nom, statut, variables, bouton favoriter. Si la pipeline tourne, le terminal manager y prend la place dominante ; sur une **template** de bibliothèque, un onglet **Assistant** héberge le copilote d'authoring (ADR-0048 / ADR-0051), et un glyphe « agent » dans la toolbar y saute directement (côté run, le même chemin mène au Manager — #302). Realtime via WebSocket : chaque événement de l'event log push une update vers l'UI.

### Status icon par Run

Le point ne peut pas être **tout** le signal (#503) : un Run non vert projette la **raison** de son événement terminal, qui titre le point dans la liste et s'affiche dans le panneau du Run — le premier endroit où l'on arrive en cliquant un point rouge.

### Cleanup vs archive

Le bouton « Cleanup » sur un Run terminé supprime la branche et les worktrees, **après copie** des sorties vers le *Blackboard archivé* (ADR-0020) — c'est ce qui rend un Run `archived` consultable (canvas en lecture seule + outputs). **L'event log n'est pas touché** : le Run reste interrogeable post-mortem. Pas d'auto-cleanup, jamais. Le seul reclaim du Blackboard archivé est le `forget`.

### Forget durable

Le `forget` (`DELETE /runs/<id>`, autorisé sur un Run `archived`) est **durable** (ADR-0024) : tombstone + purge des events en une transaction. Un écrivain tardif ne peut plus ressusciter le run ; les commandes sur un run oublié répondent **410 Gone** ; un run_id oublié n'est jamais réutilisable.

### Notifications

Pas de notifications système v1. Le status icon suffit. Si ça manque, opt-in plus tard.

---

## Stack technique

Choix et pourquoi → ADR-0003. Daemon **Rust**, frontend **React + Vite** (canvas **xyflow**) **embarqué dans le binaire du daemon**.

### Service unit persistant (ADR-0019)

**Service unit** : l'unité OS qui fait démarrer le daemon au boot et survivre au logout (systemd `--user` sous Linux, LaunchAgent best-effort sous macOS) — la différence entre « les Triggers ne tournent que tant que tu es loggé » et un orchestrateur autonome fiable. CLI `pdo service {install|uninstall|status}`. Garde de conflit de port à l'install (deux daemons ne partagent jamais un port). La status-bar affiche une pastille `ephemeral` quand le daemon ne survit pas au reboot — le seul signal que le dot de connexion ne peut pas exprimer (joignable ≠ persistant). Lignes load-bearing de l'unité et pourquoi → ADR-0019.

### Versioning (#139)

**Source de vérité unique : le `version` du `Cargo.toml` workspace.** `frontend/package.json` reste à `0.0.0` en permanence — intentionnel. Le daemon expose sa version compilée via `GET /sessions` (l'endpoint de la status-bar — pas de route dédiée : un champ JSON additionnel est rétro-compatible et évite une entrée de whitelist proxy). En prod le binaire embarque le frontend, donc daemon et UI ne divergent pas.

### Mono-user, local

Le daemon bind **`0.0.0.0:<port>`** — joignable depuis le LAN, c'est **délibéré** (#260 est closed, pas différée). Pas d'auth, pas de TLS, pas de multi-user : single-user local par design, sur un réseau de confiance.

**Le chemin de lecture ne dépend jamais d'Internet.** Les egress du produit sont tous **opt-in et tolérants à l'échec** : `docker pull`, guards de Trigger shellés, sync de la table de prix. Chaque nœud est par ailleurs une session `claude`, donc le produit ne fonctionne pas hors ligne — « pas de dépendance réseau » n'a jamais été littéral.

### Persistance et hot-reload

- **Save explicite** (#35) : bouton Save, Cmd/Ctrl+S, et flush automatique au lancement d'un Run. Pas d'auto-save debounced. Le canvas EST le fichier YAML + les prompts.
- **Hot-reload bidirectionnel** : édition externe (Vim, VS Code) → re-parse et re-render. Last-write-wins.
- **Undo/redo** (#226, ADR-0014) : pile **par onglet**, scopée à l'**édition** (exclut l'état de Run et les prompts), in-memory, plafonnée.
- **Pas de git intégration v1.**

### Onglets de pipeline (canvas)

Un **onglet de pipeline** = un document ouvert dans la zone centrale (un `PipelineDef` + prompts + historique d'undo). À ne pas confondre avec les onglets de **liste** du panneau gauche ni l'onglet **info** de la toolbar.

- **Onglet de run** : contexte = un Run, édite le snapshot run-scope. Un onglet de **pipeline** édite le registre d'instance.
- **Mode mono-onglet** *(préférence UI, #342)* : ouvrir une pipeline **remplace** l'onglet courant. Préférence par-poste (`localStorage`), hors Configuration d'instance (ADR-0015 ne couvre pas la présentation locale). _Éviter_ : « mode document » (collision avec *document* = artefact).

### Création d'un nouveau nœud

La création **depuis un YAML** (#345) est le round-trip natif de l'*Export as YAML…* — à ne pas confondre avec l'*Import de workflow*, format étranger avec perte (ADR-0016). **Pas de library de templates PDO-shipped** (ADR-0001 : pas d'opinion vendor sur « à quoi ressemble un Implementer »).

---

## Bibliothèque

La **bibliothèque de nodes** contient les nodes réutilisables qu'un utilisateur peut ajouter à une Pipeline. La réutilisation reste une aide à l'édition : un Document de pipeline transportable développe chaque node partagé en node ordinaire, sans recréer son appartenance à la bibliothèque sur l'instance cible.

Une entrée `agent` ou `script` **énonce son isolation** (cf. *Isolation de Node*) comme elle énonce son harnais et ses ports : l'entrée porte le choix, l'instanciation le restitue tel quel, et une entrée écrite avant que le champ existe se lit au défaut de son type — jamais comme un silence. L'isolation entre donc dans l'**identité sémantique** de l'entrée : deux Nodes identiques au worktree près ne sont pas le même contenu, et l'étoile de synchronisation le dit. Une entrée `merge`, `start` ou `end` ne porte aucune ligne, un Merge étant isolé par construction.

_Éviter_ : « pipeline de bibliothèque » — les Pipelines appartiennent au registre d'instance, pas à la bibliothèque.

### Registre de pipelines

Le **registre de pipelines** est l'ensemble des Pipelines possédées par une instance PDO. Une Pipeline n'appartient ni à un dépôt ni à une bibliothèque ; le dépôt est choisi comme contexte d'un Run.

**Duplicate (pipeline)** — clone indépendant dans le registre : identité fraîche, nom suffixé `(copy)` puis `(copy N)`, contenu possédé par la Pipeline conservé au maximum. À distinguer du duplicate de node sur canvas.

### Document de pipeline transportable

Un **Document de pipeline transportable** est la représentation YAML versionnée et interprétable qui permet de copier une Pipeline entre instances. Il conserve au maximum le graphe, les boucles, le routage, la présentation, les notes, les prompts, les déclarations et valeurs par défaut de variables, et développe les nodes partagés en nodes ordinaires.

Il ne transporte ni secrets, ni environnement, ni valeurs d'exécution, ni configuration d'instance. Un choix agentique lié à un profil d'instance redevient **Inherit** ; l'identité de la Pipeline est recréée à l'import. L'import est atomique, et un document invalide ou d'une version non prise en charge est refusé avec diagnostics.

**Un document produit par PDO est importable par PDO** : l'export ne peut pas émettre ce que l'import refuse, sinon la panne tombe sur la machine de destination — celle qui ne peut rien corriger. Les prompts vivent en fichiers annexes nommés par identifiant de node, et un node supprimé y laissait le sien : la clé morte rendait la Pipeline non transportable. L'invariant *clés ⊆ nodes* se tient donc partout où les prompts franchissent une frontière — sauvegarde, export, écriture au registre — et, à l'import, un prompt sans node est un **reliquat** qu'on écarte avec un avertissement, jamais un motif de refus. Reste fatale la seule clé qui ne peut pas être un nom de fichier : elle désigne un chemin, pas un node.
_Éviter_ : « YAML canonique » — le format interne peut séparer des contenus que le document rassemble ; le contrat est la fidélité maximale du round-trip, pas l'identité avec le stockage interne. « Document corrompu » pour un reliquat de prompt — un reliquat se jette, une corruption se refuse.

---

## Import de workflow (Claude Code → pipeline)

**Import de workflow** :
Décompilation **avec perte** d'un workflow Claude Code (`.claude/workflows/*.js`) en un **brouillon de Pipeline** déposé dans le registre (jamais lancé). But = **onboarding**, pas fidélité — « importe le câblage, signale le reste ». Parsing par AST statique, **jamais d'exécution du `.js`** (ADR-0016 — le daemon bind `0.0.0.0`, exécuter du JS étranger serait un RCE).
_Éviter_ : « conversion », « migration » — la **migration** réécrit du YAML PDO d'un ancien schéma vers le courant (même format) ; l'**import** traduit un format étranger.

**Placeholder annoté** :
Nœud `agent` dont le corps explique un idiome de workflow que l'import v1 ne matérialise pas. L'annotation **est** le tutoriel d'onboarding : elle nomme ce qu'un utilisateur PDO n'écrirait jamais à la main (gestion worktrees, boucle budgétaire — remplacés par des features plateforme) et le traduit en interaction délibérée. Distinct du *nom placeholder* d'un Run.

**Extraction verbatim** :
Règle de récupération des prompts : string-literal sans interpolation → verbatim ; template-literal avec `${…}` → texte statique verbatim + marqueurs câblables ; prompt sans texte statique → placeholder annoté.

### Relations

- Un **Import de workflow** produit un **brouillon de Pipeline** dans le registre.
- L'**import de Pipeline PDO** interprète un Document de pipeline transportable et crée une Pipeline indépendante ; il partage la même modale que l'Import de workflow, mais constitue un mode distinct et fidèle.
- Idiomes mappés : `agent()` → **Node**, `pipeline()` → boucle **`collection`**, `for`/`while` autour d'un `agent()` → boucle **`bounded`**, `if`/`return` gardé → **edge conditionnelle**, schémas JSON → **frontmatter de port de sortie**.
- Un idiome hors sous-ensemble → **placeholder annoté**. Un `git merge` scripté → Node `agent` annoté, **pas** le Merge first-class (dont il excède le contrat).
- Tout rôle importé — placeholder annoté compris — devient un Node `agent` **isolé**, et le brouillon écrit la ligne. L'import ne déduit jamais l'isolation du prompt, du nom du rôle, de ses sorties ni de son appartenance à une région `collection` : un workflow étranger n'a pas d'avis sur les worktrees, et en inventer un est précisément la devinette qu'ADR-0060 supprime. L'auteur arbitre ensuite sur le canvas.
