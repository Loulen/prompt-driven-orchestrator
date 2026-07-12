# Prompt Driven Orchestrator (PDO) — Glossaire métier

Glossaire vivant. Mis à jour au fil des décisions, lazy.

---

## Pipeline

Un **Pipeline** est un DAG nommé, à **orchestration déterministe**, qui décrit l'enchaînement de rôles d'agents pour accomplir une tâche d'ingénierie.

- **Orchestration déterministe** : aucun *LLM-router*. Le routage entre nœuds suit des prédicats mécaniques portés par `Switch` et `Loop` (ADR-0002). Aucun LLM ne décide à l'exécution quel nœud activer.
- **Pas de routage probabiliste** : le déterminisme porte sur la *structure d'orchestration* (qui appelle qui dans quel ordre), pas sur le contenu produit par chaque nœud (les LLM aux feuilles restent stochastiques).
- **Graphe modifiable pendant l'exécution** : la topologie n'est pas immuable. L'utilisateur peut éditer le graphe pendant qu'un Run tourne (ADR-0007) — ajouter un nœud, créer une edge, etc. — et le scheduler se réajuste au prochain tick. Les nœuds en cours d'exécution restent immutables (cf. *Édition pendant un Run* ci-dessous).
- **Multiples pipelines plutôt qu'embranchements** : pour gérer des trade-offs coût/complexité (ex. *quick-fix* vs *feature-with-adversarial-review*), on définit plusieurs pipelines distincts. Pas un seul pipeline avec des branches.

Contrairement à : Liza (pipelines YAML), Langgraph (conditional edges + LLM-router), TPM workflow (orchestrateur LLM qui décide quand spawner).

---

## Node

Unité atomique d'un Pipeline. Un **Node** représente un rôle. La plupart des nodes lancent une instance de Claude Code à laquelle on confie un prompt système qui définit sa mission (Implementer, Planner, Reviewer, etc.). Un node **`script`** fait exception : il exécute du **bash déterministe fourni par l'auteur**, sans LLM (cf. *Node `script`* ci-dessous, ADR-0017).

Un Node se définit par :

- **Nom** — identifiant lisible affiché dans le canvas.
- **Prompt système** — le rôle, écrit dans la zone de texte qui s'ouvre à l'édition.
- **Ports de sortie — déclarés.** Un ou plusieurs documents produits, chacun un port nommé : c'est le **contrat de production** du Node (avec son schéma de frontmatter optionnel, cf. *Blackboard*). Multi-fan-out supporté (le Debugger sort `repro_steps` + `screenshots`). Rendu : **un dot vert par document**, drag-source des edges, librement placé.
- **Ports d'entrée — émergents.** Un Node ne **déclare pas** ses entrées : elles sont *dérivées des edges entrantes*. Connecter `debugger.repro_steps` vers un Node y crée de facto une entrée `repro_steps` (le nom suit le document amont). Plusieurs edges de même nom **poolent** dans une seule entrée-liste — pooling **sémantique**, jamais un groupement visuel des flèches : chaque flèche atterrit où le designer veut sur le Node, **sans dot d'entrée**. Sur collision de noms *distincts*, on qualifie par source. L'accumulation cross-itérations (glob `iter-*`) est un flag `repeated` porté par l'**edge**, pas par l'entrée.

Asymétrie assumée (et déjà présente dans le typage : output = contrat vérifié, input = best-effort) : le Node *connaît* ses sorties, *découvre* ses entrées au câblage. Conséquence sur la bibliothèque : un Node réutilisable porte ses **outputs + rôle + type**, pas ses inputs (purement pipeline-spécifiques).

Distinct de :

- **NodeRun** *(à valider)* — l'exécution d'un Node au sein d'un Pipeline Run précis. Un NodeRun = une session tmux Claude Code dans un sous-worktree dédié, avec un statut (pending/running/done/failed).

### Modèle (par node)

Chaque Node peut porter un **modèle** optionnel (`model: Option<String>`) : l'identifiant du modèle Claude avec lequel sa session est lancée (`claude --model <x>`). Absent ⇒ le node utilise le modèle par défaut du compte (aucun `--model` n'est passé). Permet de payer un modèle capable là où le raisonnement est dur (Planner, Reviewer) et un modèle économique sur les nodes mécaniques.

- **Texte libre, pass-through, aucune validation** : la valeur est un alias (`opus`, `sonnet`, `haiku`, `opusplan`, `fable`…) ou un id complet (`claude-opus-4-8`), transmise verbatim à `claude`. Un id invalide fait échouer `claude` au démarrage — *sharp tool*, responsabilité du designer (ADR-0001). Pas d'enum fermé qui périmerait à chaque sortie de modèle.
- **Sémantique, pas layout** : le modèle fait partie de l'identité du pipeline (il change *quel agent* tourne). Il entre donc dans le **diff sémantique**, contrairement à `view`/`mode`/`waypoints`/`target_side`. Deux pipelines ne différant que par le modèle d'un node comparent **différent**.
- **S'applique aux nodes qui lancent un agent** : `doc-only`, `code-mutating`, `merge`. Les nodes structurels (`start`, `end`) ne lancent pas de session → pas de modèle.
- **Resume** : une session reprise (`claude --continue`) **conserve son modèle** d'origine (garanti par la doc Claude Code), donc pas besoin de re-passer `--model` au resume.
- **Défaut daemon-wide hors-scope** : un `default_model` d'instance viendra plus tard via `instance_config` (ADR-0015). _Éviter_ : « modèle global », « modèle du run » (le modèle est *par node*, jamais par run).

### Node `script` — exécution déterministe (ADR-0017)

Un node **`script`** exécute le bash de l'auteur au lieu de lancer Claude. Il tourne dans une **session tmux** (attachable comme tout NodeRun, ADR-0005) dont le tail est `timeout N bash <corps>` : **exit 0 ⇒ node `completed`**, non-zéro ou timeout ⇒ `failed`. En v1 il est d'**effet doc-only** (pas de sous-worktree ; tourne dans le worktree du Run ; doit le laisser propre).

- **I/O par variables d'environnement** (un script ne lit pas le préambule prose) : `PDO_INPUT_<PORT>`, `PDO_OUTPUT_<PORT>`, `PDO_ARTIFACTS_DIR`, `PDO_VAR_<NAME>`, plus les `PDO_RUN_ID/NODE_ID/NODE_ITER/DAEMON_URL` habituels. Le script écrit lui-même `output.md` à `$PDO_OUTPUT_<port>` ; pour piloter une edge `when:`, il y écrit sa propre frontmatter YAML. `outputs_validator` s'applique en **fail-fast** (pas de retry interactif — la session a quitté).
- **Corps** stocké dans le slot prompt du node (`<pipeline>.prompts/<node>.md`). Un corps vide fait échouer le lancement (fail-loud, pas de no-op silencieux).
- **Pas de `model`** (aucun agent lancé). Le seam de test `tmux_cmd_override` ne s'applique pas à un script (le bash *est* déterministe, donc testable sans stub).
- **Sécurité** : équivalent au guard de Trigger et au bash d'un agent — le bash de l'auteur dans son propre pipeline, aucune nouvelle frontière de confiance (#260 reste le contrôle réel).
- **Sharp tool** : un script doc-only qui fait `git commit` laisse l'arbre propre et passe le garde d'immutabilité — c'est la responsabilité de l'auteur.

## Dataflow

Modèle (A) — **document-first, code en side-channel** :

- Les arêtes du DAG transportent **uniquement des documents** (artefacts markdown).
- Le **code** vit dans la branche du Pipeline Run. Quand un NodeRun finit, son sous-worktree est mergé dans la branche du Pipeline Run. Le NodeRun suivant fork un nouveau sous-worktree depuis cet état.
- Les wires de l'éditeur = dataflow documentaire intentionnel. L'état du code suit en arrière-plan.

À traiter plus tard : conflit potentiel quand deux NodeRuns parallèles modifient le code → stratégie de waves / disjoint-files (cf. Liza/TPM).

---

## Edges conditionnelles — le routage vit sur l'arête

Le nœud **`Switch`** est **supprimé** (obsolète), ainsi que le pattern "clause `when:` portée par les ports de sortie d'un Switch". **La condition de routage vit désormais directement sur l'edge**, attachée à l'output port qu'elle quitte. Un Switch n'était qu'un pass-through ré-émettant son input sur un port gardé — un hop fantôme dans le dataflow que l'edge conditionnelle élimine : le `review` d'un Reviewer va directement vers `implementer` (`verdict=FAIL`) ou `end` (`verdict=PASS`), chaque arête gardée, sans nœud intermédiaire. Cf. ADR-0011 (supersede le placement décidé en ADR-0002).

### Forme

Une edge porte une clause `when:` **optionnelle** (même grammaire de prédicats qu'avant). Sans clause, l'edge est inconditionnelle (fire toujours).

```yaml
edges:
  - source: { node: reviewer, port: review }
    target: { node: implementer, port: task }
    when: { verdict: { in: [FAIL, NEEDS_WORK] } }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
    when: { verdict: { eq: PASS } }
```

### Évaluation — multi-match, pas d'ordre

À l'arrivée d'un artefact sur un output port, **toutes** les edges sortantes dont la clause est satisfaite **firent** — le flux peut fan-out vers plusieurs nœuds simultanément. Pas de `first-match-wins`, aucun ordre déclaré ne compte (supersede la sémantique first-match-wins du Switch). Si deux conditions se chevauchent, les deux branches partent : c'est voulu (ADR-0001, *sharp tool*) — le designer écrit des conditions disjointes pour un XOR, ou converge un fan-out `code-mutating` via un `Merge`. Une edge **`else`** (clause vide marquée `else: true`) fire **uniquement si aucune edge sœur** (même output port source) n'a matché.

Feedback runtime : un nœud qui a firé passe au vert ; les edges déclenchées sont marquées d'un indicateur sur le canvas, pour rendre le fan-out lisible.

### Champs référençables

- Tout champ de frontmatter de l'artefact quittant le port source.
- Toute variable pipeline `$<name>`.
- **`iter`** — le compteur de la région englobante (cf. *Loop regions*). Re-autorisé comme champ de `when:` : il n'avait été retiré que parce que le nœud `Loop` portait le compteur ; le nœud disparaissant, le compteur redevient adressable. Sert notamment à câbler une sortie d'épuisement (`iter: { gte: $max }`).

Prédicats : `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `in`, `not_in`. Pas d'eval libre, jamais de LLM-router — le principe mécanique d'ADR-0002 tient. Plusieurs prédicats dans une clause sont **AND'd** ; pour OR, `in: [...]` sur un champ, ou plusieurs edges sœurs vers la même target.

---

## Loops — boucles matérialisées, nommées

Les nœuds **`Loop`** et **`ForEach`** sont **supprimés** (obsolètes). Une boucle n'est plus un nœud à ports `body`/`done`/`break` : c'est une **entrée nommée du bloc `loops:`** du YAML, qui référence un ensemble de nœuds membres. Le mot *région* désigne son **rendu** sur le canvas (boîte translucide autour des membres ; simple marqueur si la boucle n'a qu'un membre). Les edges restent uniformes : **aucune edge n'est marquée "back-edge"**, son rôle est *dérivé* de la boucle, jamais stocké.

Pourquoi une identité nommée plutôt qu'une détection de cycle pure : "quelle edge est *la* back-edge" est une propriété topologique globale qui bascule quand on édite le graphe ailleurs. Un **id stable** sort cette identité de la topologie, stabilise la persistance du bound, et ouvre les boucles imbriquées. Cf. ADR-0011.

### Forme

```yaml
loops:
  - id: review-loop
    kind: bounded          # compteur borné, séquentiel
    members: [implementer, reviewer]
    max_iter: 3
  - id: per-issue
    kind: collection       # fan-out parallèle data-driven
    members: [fixer]       # ≥ 1 membre — souvent un seul nœud
    over: issues           # champ liste dans l'artefact entrant
```

- `members` : **liste explicite d'ids de nœuds, ≥ 1** (jamais spatial — déplacer un nœud hors de la boîte ne le retire pas de la boucle). Une boucle **n'est pas nécessairement un sous-graphe** : un seul membre est légal et fréquent.
  - `collection` à un membre = fan-out d'**un** nœud par item (le foreach le plus courant : « un fixer par issue »).
  - `bounded` à un membre = un nœud qui **se relance** (self-edge) jusqu'à `max_iter`.
- **Entrée** = le membre ayant une in-edge depuis un non-membre (membre unique : ce nœud). **Re-entry** = une edge d'un membre vers cette entrée (membre unique : sa self-edge).
- **Rendu** : ≥ 2 membres → boîte englobante ; 1 membre → marqueur compact sur le nœud. Header : `↻ X/Y` (bounded) ou `⇉ N items` (collection), **en lecture seule sur le canvas** — ni id ni éditeur inline (règle slim card #149). Le `max_iter` et l'id se consultent/s'éditent dans l'**inspecteur de région** (clic sur le header).

### Deux drivers

- **`bounded`** — driver = compteur `max_iter`. **Naît par auto-détection** : câbler une edge qui ferme un cycle (self-edge incluse) matérialise une boucle (id généré + `max_iter` par défaut), pour qu'un cycle ne soit jamais accidentellement non-borné.
- **`collection`** (ex-ForEach) — driver = `over: <field>`, liste lue dans la frontmatter de l'artefact entrant. **Naît par geste explicite** (sélection du/des membre(s) → « fan out sur une collection » → choix du champ) : un fan-out parallèle n'a aucune signature topologique à détecter. Un output typé `list` câblé en aval peut *suggérer* le geste, sans l'imposer.

### Compteur d'itération

- **Par-boucle**, keyé sur l'`id` : une boucle = un `iter`. Tout nœud **membre** estampille ses artefacts avec l'`iter` courant. Un nœud hors boucle garde l'`iter` de ses propres runs (1 s'il n'a couru qu'une fois) : il n'est **jamais re-spawné par un lap** (#195/#199 — seul un vrai cycle émergent ou le moteur de région peut re-lancer un nœud déjà complété ; un membre n'est jamais spawné au-delà de `max_iter`).
- **Résolution d'inputs** (canonique, #194/#210 — module `input_resolution`) : un input se résout vers **la dernière itération complétée** du nœud source — jamais l'artefact d'une itération échouée, jamais un alignement positionnel sur l'`iter` du consommateur. Un feeder externe à une boucle continue de servir son artefact complété à n'importe quel lap.
- `bounded` : le compteur **incrémente quand une re-entry fire**, et l'entrée est re-spawnée **une seule fois par lap** même si plusieurs re-entries firent (coalescées — absorbe le double-spawn iter+1 de #108). La barrière de lap dans un body multi-nœuds est le fan-in naturel du nœud de jointure, pas une machinerie dédiée.
- Adressage et accumulation inchangés : `reviewer/iter-2/review/output.md` ; un input `repeated: true` glob `iter-*/<port>` → un artefact par lap, ordonné.

### Sortie de boucle

- **Succès anticipé** : une edge forward conditionnelle quittant un membre (`verdict=PASS → end`). Remplace l'ancien `break`.
- **Épuisement** (`bounded`) : à `iter = max_iter` avec la condition de continuation encore vraie, la re-entry est plafonnée. Le designer **peut** câbler une sortie d'épuisement (`when: { iter: { gte: $max } }`) vers où il veut. Sinon, et si aucune edge forward ne matche, la boucle entre dans un état **bloqué "exhausted — unrouted"** explicite (jamais de stall silencieux), routable par le Pipeline Manager (#126). Pas d'auto-proceed implicite.
- **`collection`** : **barrière** — les edges quittant la boucle firent **une seule fois, quand tous les items sont terminés** (préserve `done → Merge`, ADR-0006). Liste vide → barrière immédiate, edges sortantes firent une fois sans item-artefact. Items `code-mutating` → chacun son sous-worktree, convergence via `Merge`.

### Imbrication — différée

Le modèle à id autorise de *déclarer* `inner ⊂ outer`, mais la **sémantique d'itération imbriquée** (coordonnée composite `outer-2/inner-3`, accumulation scopée au lap parent) est **différée** : v1 = itération plate, un seul niveau.

### Édition pendant un Run & intra-Run

Supprimer l'edge qui retire le **dernier cycle** des membres d'une boucle `bounded` déclenche un **popup de confirmation** (« ceci détruira la boucle <id> ») ; confirmé, l'entrée `loops:` est retirée, bound et état d'itération partent avec. L'interaction avec un Run actif est régie par ADR-0007 (nœuds running immuables, edges libres). Les compteurs `iter` repartent de zéro à chaque Run — pas de mémoire d'itérations entre Runs.

---

## Edges — structure

Une edge câble un output port source vers un input port target, et porte une clause `when:` **optionnelle** (sémantique multi-match : cf. *Edges conditionnelles*). La terminaison du Run passe toujours par un edge vers le nœud `End` mandatoire (#39) ; le pattern halt-edge des versions antérieures reste déprécié.

### Routage — `mode` + `waypoints` (#154)

Le tracé d'une edge est **orthogonal** (connecteur à angle droit) : l'auto-routage évite les autres nœuds et se recalcule quand un nœud bouge. Le routage vit sur l'edge via deux champs :

- **`mode`** : `auto` (défaut, absent) ou `manual`. Une edge `auto` ne stocke **aucun** waypoint — son chemin est recalculé déterministiquement à chaque rendu (re-route gratuit au déplacement d'un nœud). Une edge `manual` épingle son tracé.
- **`waypoints`** : liste de points **absolus** `{ x, y }`, significative seulement en `manual`. Le premier drag d'une poignée de segment épingle la route (`mode: manual`) ; l'action par-edge « re-route automatically » (panneau de détail d'edge) les efface et repasse en `auto`. Les waypoints absolus acceptent le drift quand un nœud bouge : le reset est l'échappatoire.

`mode` + `waypoints` (comme `view` sur les nœuds) sont du **layout, pas de la sémantique** : ils persistent **dans le fichier pipeline** (le routage voyage quand un workflow est partagé) mais sont **exclus du diff sémantique** — deux pipelines ne différant que par leur routage ou les positions de leurs nœuds comparent **égaux** (déplacer un nœud ou bouger un waypoint ne marque jamais le pipeline « modifié »). Cf. #154, design screen 14.

### Ancrage de l'edge entrante — `target_side` (#168)

Les inputs sont **émergents** (#149) : une flèche entrante n'atterrit pas sur un dot d'input déclaré mais **sur le corps** du nœud cible. `target_side` mémorise **de quel côté** (`left` / `right` / `top` / `bottom`) la flèche s'ancre : la règle décidée est *le côté de la carte cible le plus proche du point de dépôt*. Le routage orthogonal arrive alors par ce côté (plus de gauche-vers-droite forcé). Absent ⇒ `left` (ancrage historique), jamais écrit.

`target_side` est du **layout, pas de la sémantique** (au même titre que `mode`/`waypoints`/`view`) : il persiste dans le fichier (l'arrivée des flèches voyage avec un workflow partagé) mais est exclu du diff sémantique. Les ports **déclarés** (l'input `result` du nœud `End`, les ports des nœuds structurels `merge` / `loop` / `for-each`) gardent leur côté fixe et **ne sont pas** affectés par l'ancrage au dépôt.

---

## Note (note de canvas)

Une **Note** est une annotation de documentation **inerte** posée sur le canvas : un texte libre que le designer épingle près d'un groupe de nœuds pour expliquer une intention (« ce loop est borné à 3 exprès », « TODO câbler l'edge d'épuisement »). Elle **n'est pas un Node** — aucun titre/`name`, aucun type (`doc-only`/`code-mutating`/`merge`/`script`), aucun port, aucune edge, aucune session, aucune place dans le dataflow ni l'ordonnancement : le runtime l'**ignore entièrement**.

- **Persistée dans un bloc racine `notes:`** du YAML (sibling de `loops:`/`edges:`, jamais dans `nodes:`), chaque entrée = `{ id, content, view }`. C'est la forme éprouvée de `loops:` — une entité nommée de premier niveau, rendue sur le canvas, qui n'est délibérément **pas** un type de nœud (cf. ADR-0018).
- **`content` = texte brut en v1** — pas de markdown, pour ne pas ouvrir une 2ᵉ surface `react-markdown` ni le sink `dangerouslySetInnerHTML` d'ADR-0013. Édité dans l'inspecteur (clic sur la note), pas inline sur la carte.
- **`view` = layout, pas sémantique** — même classe que `view`/`mode`/`waypoints`/`target_side` : persiste **dans le fichier** (une note partagée suit le workflow) mais est **exclu du diff sémantique** ; deux pipelines ne différant que par leurs notes comparent **égaux** (le star « synced/diverged » ne bouge pas). La puce « non sauvegardé » (dirty), elle, s'allume — normal. Taille pilotée par le contenu en v1 ; redimensionnement différé.
- **Mutable pendant un Run** : inerte, aucune session à orphaner — `mutation_validator` ne doit jamais rejeter l'ajout/édition/suppression d'une note sur un Run actif (contraste avec la suppression d'un node non-`pending`, interdite par ADR-0007).

_Éviter_ : « commentaire » (évoque un commentaire YAML `#` ou un commentaire d'issue GitHub), et « placeholder annoté » (qui est, lui, un **vrai** nœud `doc-only` produit par l'import de workflow, ADR-0016).

---

## Blackboard

Le **Blackboard** est le store partagé où vivent tous les artefacts d'un Pipeline Run. Toutes les sorties documentaires de tous les NodeRuns y sont persistées et adressées par chemin.

- **Localisation** : `<pipeline-worktree>/.pdo/artifacts/`. Suit la branche du Pipeline Run. Part au cleanup **du worktree** — mais est d'abord copié vers le *Blackboard archivé* global (`~/.pdo/runs/<run-id>/artifacts/`, lecture seule, survit au cleanup ; cf. §*Cleanup vs archive* et ADR-0020).
- **Format** : markdown brut (`.md`) avec **YAML frontmatter** pour les métadonnées structurées (verdict, statut, références, etc.). Le corps reste lisible humainement, le frontmatter est parsable par le runtime.
- **Wires** : dans l'éditeur, un wire de `Node A → Node B` n'est pas un transport ; c'est une **déclaration de dépendance**. Le runtime traduit en : *"avant de lancer B, attendre que A ait posé son artefact ; l'input port de B le lit depuis le Blackboard"*.
- **Cycles + accumulation** : chaque tour de cycle écrit dans un sous-dossier `iter-<N>/`. Les ports d'entrée qui veulent accumuler (ex. `reviews_bloquantes`) lisent un glob `iter-*/review.md` → liste naturellement ordonnée.

**Blackboard archivé** *(terme)* : copie **durable et lecture seule** du Blackboard d'un Run (plus son `pipeline.yaml` + `pipeline.prompts/`), écrite sous `~/.pdo/runs/<run-id>/` (store **global**, hors du `run_dir` repo-local) au moment de l'archivage, **avant** la suppression du worktree. Contrairement au Blackboard vif (qui *part au cleanup*), il **survit** au cleanup — c'est ce qui permet de rouvrir un Run `archived` et d'accéder à ses outputs (canvas réhydraté en lecture seule via `GET /runs/<id>/pipeline`). **N'est pas** récupéré par `cleanup_run` (repo-local) ; sa suppression relève du `forget` (cf. §*Cleanup vs archive*, ADR-0020).

### Schéma d'adressage

Chaque artefact produit par un NodeRun a un chemin canonique :

```
<pipeline-worktree>/.pdo/artifacts/<node-id>/iter-<N>/<port-name>.md
```

- `<node-id>` : slug stable du Node dans le pipeline (assigné à l'édition, ex. `implementer-1`).
- `<N>` : compteur d'itération du NodeRun. Vaut `1` pour les nœuds non-cycliques.
- `<port-name>` : nom du port de sortie (ex. `summary`, `review`, `plan`).

**Résolution des inputs** :
- Wire simple → input port lit `<artifacts>/<source-node>/iter-<latest>/<port>.md`.
- Wire d'accumulation (port marqué `repeated`, typiquement le port `reviews_bloquantes` côté Implementer dans un cycle) → input port lit le glob `<artifacts>/<source>/iter-*/<port>.md`, ordonné par N.

### Frontmatter — minimal

Les artefacts sont des `.md` avec **frontmatter YAML minimale**. La frontmatter sert au *runtime* (parser un verdict, savoir quoi router) — **pas** à structurer le contenu. Tout ce qui est destiné à être lu par un autre LLM (issues bloquantes, justifications, recommandations) reste dans le **corps** markdown.

Exemple :

```markdown
---
verdict: FAIL
---

## Blocking issues

- error_handling_missing_in_foo
- test_coverage_below_threshold

## Detailed review

Le code de `foo()` ne gère pas le cas où...
```

Pas de structures imbriquées, pas de listes lourdes en frontmatter. Si on a besoin de structure exploitable par le runtime, on l'ajoute champ par champ et on documente.

### Schéma déclaratif par output port

Un Node peut **déclarer le schéma de frontmatter attendu** sur chacun de ses output ports. Le runtime utilise ce schéma pour (a) injecter une description précise dans le préambule (l'agent sait quels champs écrire avec quelles contraintes) et (b) **valider à la complétion du NodeRun** que la frontmatter écrite respecte le schéma.

Types supportés en v1 : `enum` (avec liste `allowed`), `int`, `string`, `bool`, `list` (de strings). Pas de `float`, pas de `date`, pas de nested — si un cas concret le force, on étend.

YAML :

```yaml
outputs:
  - name: review
    frontmatter:
      verdict:
        type: enum
        allowed: [PASS, FAIL]
      score:
        type: int
      issues:
        type: list
```

**Pas de typage côté input** — l'agent fait du best-effort sur ce qu'il reçoit (un wire vers un upstream typé donne malgré tout un format lisible dans le préambule, mais aucune validation runtime ni lint d'incompatibilité). Asymétrique volontaire : l'output est un contrat de production qu'on peut mécaniquement vérifier ; l'input est un contexte que l'agent interprète.

### Validation à la complétion + fallback tmux

Quand un NodeRun signale `pdo complete`, le runtime parse la frontmatter de chaque output produit et la matche contre le schéma déclaré. Si **mismatch** :

1. **Fallback** : le runtime envoie un message dans la session tmux du NodeRun (*"Ton frontmatter ne respecte pas le schéma : <champ X manquant / valeur Y hors enum>. Corrige et retry."*). Le NodeRun reste en status `running` (pas marqué failed).
2. L'agent corrige et appelle à nouveau `pdo complete`. Le runtime re-valide.
3. Si la 2e tentative échoue (limite : **1 retry max**, 2 tentatives au total), le NodeRun est marqué `failed` avec raison *"output frontmatter mismatch après retry"*.

Ce mécanisme évite de fail loud sur une erreur que l'agent peut typiquement corriger seul, tout en bornant la dérive (un agent qui boucle dans le mismatch finit failé en deux tours).

### Avance détachée après transition terminale (#304, ADR-0023)

Le 2xx de `pdo complete` (et `fail`/`skip`) signifie « ton événement terminal est durablement enregistré et l'avance est planifiée », **pas** « le run a avancé ». Après l'append de l'événement terminal (`NodeCompleted`/`NodeFailed`/marqueur skip), la queue du handler — reap de la session tmux + avance du run (spawn du successeur, finalisation du port `end`, `RunFailed`/`RunSkipped`, `retry_waiting_nodes`) — s'exécute sur une tâche `tokio::spawn` **détachée** de la requête HTTP. Raison : le reap tue la session tmux du client `pdo` lui-même ; inline, hyper annulait la future de la requête à la fermeture de la socket et l'avance était silencieusement perdue (run coincé `running`). Les erreurs de validation (guard, merge conflict, outputs) restent renvoyées in-request ; les erreurs d'avance surfacent via `RunFailed` + logs, jamais via la réponse HTTP. Un panic dans la queue détachée est isolé (`catch_unwind`) et émet un `RunFailed` explicite.

---

## Variables pipeline

Une pipeline déclare au niveau racine un block `variables:` — paires nom/valeur typées (entiers, floats, strings, listes, booléens) qui peuvent être référencées dans n'importe quelle clause `when:` via `$<name>`.

```yaml
variables:
  max_iter_review: 5
  max_iter_plan: 3
  min_quality_score: 7
```

**Override au lancement d'un Run** : le payload `POST /runs` peut inclure un objet `variables: { ... }` qui écrase les valeurs déclarées. Permet de relancer une même pipeline avec une config différente sans toucher au YAML. Les variables non-overridées gardent leur valeur de la pipeline.

Pas d'expressions calculées dans la déclaration des variables — uniquement des littéraux. La logique reste dans les `when:`.

---

## Prompt augmentation — déterministe

Chaque NodeRun voit son prompt construit en deux couches :

1. **Prompt utilisateur** — la zone de texte que le designer du pipeline a remplie à l'édition (le "rôle" du nœud : *"Tu es un Reviewer. Tu lis le code, tu identifies les blocking issues..."*).
2. **Préambule runtime** — généré déterministiquement à partir des ports configurés. Ne dépend pas du LLM, écrit par PDO à chaque NodeRun.

Le préambule contient au minimum :

- **Inputs disponibles** :
  - Pour chaque port d'entrée : nom du port + chemin absolu sur disque + (optionnel) inline du contenu si court.
  - Ex. *"Tu as accès à : `plan` (lis `<artifacts>/planner-1/iter-1/plan.md`), `task` (lis `<artifacts>/planner-1/iter-1/task.md`), `reviews_bloquantes` (lis tous les fichiers `<artifacts>/reviewer-1/iter-*/review.md`)."*
- **Outputs attendus** :
  - Pour chaque port de sortie : chemin où écrire + schéma de frontmatter requis.
  - Ex. *"Tu dois produire à `<artifacts>/reviewer-1/iter-2/review.md` un fichier markdown avec frontmatter YAML contenant le champ `verdict: PASS | FAIL`. Le contenu détaillé (blocking issues, justifications) va dans le corps."*
- **Capacités PDO-specific (CLI)** :
  - `pdo complete` — à appeler via Bash quand le NodeRun est terminé (cf. signal de complétion, Q10).
  - `pdo fail --reason "..."` — à appeler en cas d'incapacité à finir.
  - Ces commandes ne sont **pas** packagées comme skills Claude Code — elles sont 100% systématiques, sans bénéfice de progressive disclosure.
- **Itération courante** : *"Tu es à l'itération {iter} de ce nœud."* Permet à l'agent d'adapter son comportement au tour de boucle (par exemple : Implementer en iter 1 implémente from scratch ; en iter 2+ il itère sur les reviews).
- **Variables pipeline résolues** : injecte les valeurs des variables référencées dans le préambule (utile si l'agent doit savoir le `max_iter_review` pour adapter son verbosité, par exemple).

Conséquence : le designer du pipeline n'a pas à se soucier dans son prompt utilisateur de *"où écrire / quoi mettre en frontmatter / comment signaler la fin"* — c'est imposé par le runtime. Il se concentre sur le *rôle*.

### Skills Claude Code — délégué

PDO **ne gère pas** les skills. Les skills disponibles dans une session NodeRun sont ceux que Claude Code charge naturellement : `~/.claude/skills/`, `<target-repo>/.claude/skills/`, `<sub-worktree>/.claude/skills/`. Pas d'attachement par-Node, pas de symlink, pas de mécanisme custom. Si le user veut une capacité spécifique, il l'exprime soit dans le prompt du nœud, soit en modifiant la pipeline elle-même.

---

## `code-mutating` vs `doc-only`

Chaque Node est typé par son **effet sur le code** :

- **`code-mutating`** — Implementer, Refactorer, Migrator, Merge. Reçoit un sous-worktree forké depuis la branche du Pipeline Run. Peut éditer/commit/merger. À la fin du NodeRun, son sous-worktree est mergé dans la branche du Pipeline Run.
- **`doc-only`** — Planner, Reviewer, Architect, PRD-writer. Pas de sous-worktree. Lit la branche du Pipeline Run en read-only (`git show`, `git diff`, `git log`). Écrit uniquement dans le Blackboard.

Garde-fou : à la fin d'un NodeRun `doc-only`, la branche du Pipeline Run doit rester intacte (pas de commit). Si une violation est détectée, le NodeRun échoue.

Conséquence sur la parallélisation : les `doc-only` sont gratis-parallèles (pas de merge possible). Les `code-mutating` parallèles voient leurs branches mergées séquentiellement à la fin (ordre de complétion).

---

## Merge — nœud first-class

Le **`Merge`** est un nœud first-class du DAG, type `code-mutating` toujours, à placer explicitement par le designer (ADR-0006). Il remplace l'ancien Merge Resolver auto-spawné, dont la formulation est désormais **obsolète** (auto-spawn supprimé, toggle `auto_merge_resolver` supprimé). L'utilisateur dessine la convergence ; le runtime ne l'invente pas.

### Forme

- 1 input port `branches: repeated` — accumule les branches **réellement firées** qui convergent (compte dynamique : une branche routée ailleurs ou supprimée par un `else` n'y entre pas — cf. addendum ADR-0006).
- 1 output port `merged` — artefact résumé du merge avec frontmatter `conflict_count`, `branches: [...]`, et corps narratif.

### Sémantique runtime

1. **Barrière (edge-centrée, addendum ADR-0006)** : le Merge est prêt quand **toutes ses edges entrantes sont résolues** — chacune a soit **firé** (producteur `Completed` + garde satisfaite / edge inconditionnelle), soit est **morte** (producteur `Completed` mais l'edge n'a pas firé, ou producteur lui-même mort) — et qu'**au moins une a firé**. Il spawne en consommant **uniquement les branches firées**, ignorant les mortes. (L'ancienne formulation node-centrée « attend que tous les upstream soient `Completed` » est superseded par les edges conditionnels d'ADR-0011 : une branche non-routée ne devient jamais `Completed` et bloquerait le Merge — stall silencieux.) Un Merge dont **toutes** les branches sont mortes est lui-même mort et sauté tant que `End` reste atteignable ; si la cascade de mort rend `End` inatteignable, le Run **halt explicitement** (« unrouted »), jamais de stall silencieux.
2. **Fork** : forke un sous-worktree depuis la branche du Pipeline Run.
3. **`git merge`** : tente le merge automatique sur chaque upstream qui a une branche dédiée (= les `code-mutating`). Les `doc-only` upstream n'ont pas de branche, leurs artefacts sont consommés via le Blackboard pour le summary.
4. **Si conflit** → spawn Claude Code dans le sous-worktree, qui lit les artefacts du Blackboard pour reconstituer les intentions, résout, commit, écrit le `merged.md` avec frontmatter et résumé narratif.
5. **Si pas de conflit** → écrit un `merged.md` trivial (frontmatter `conflict_count: 0`), commit le merge, sans LLM.
6. À la fin : son sous-worktree est mergé dans la branche du Pipeline Run.

### Lint info-only

Si le designer dessine un fan-out `code-mutating` sans `Merge` downstream, l'éditeur affiche un diagnostic info-only sur le canvas (cf. ADR-0001 : pas bloquant, juste lisible). Pas de blocage à la sauvegarde. Le canvas est l'unique surface de ces diagnostics pipeline-wide : un overlay flottant, jamais dupliqué dans l'inspecteur (qui reste scopé au nœud et aux métadonnées de pipeline). Cf. #63.

---

## Principe — Sharp tool, not safe tool

L'outil ne contraint pas l'utilisateur à dessiner des pipelines "sains". Pas de validation prescriptive du graphe (genre *"interdit fan-out `code-mutating` sans Reviewer downstream"*), pas de warnings paternalistes. Si une pipeline est foireuse — fan-out non revu, accumulation infinie, deadlock conceptuel — c'est la responsabilité du designer du pipeline. PDO fournit des primitives nettes ; l'usage est libre.

Conséquences à anticiper sur les décisions futures :
- Schéma déclaratif côté output uniquement (cf. *Frontmatter — Schéma déclaratif par output port*) ; pas de typage côté input — l'agent fait du best-effort.
- Pas de "lint pipeline" bloquant. Au max, un lint info-only (ex. fan-out `code-mutating` sans Merge downstream).
- L'éditeur permet des graphes "exotiques" (cycles, fan-out `code-mutating` sans Merge explicite, ports déconnectés). Le runtime se débrouille ou halt explicitement.

---

## Principe — Deliberate, then autonomous (trust-earned)

PDO ne **démarre** pas en *"set it and forget it"* : la valeur initiale est dans le **temps passé en conception**, et le défaut reste délibéré (humain dans la boucle). Mais l'autonomie est une **cible atteignable, pas un interdit** : une fois qu'un pipeline a gagné la confiance de l'utilisateur sur une classe de tâches, celui-ci **peut** le laisser aller jusqu'au bout — pousser, ouvrir une PR, merger — sans intervention.

Point clé : **l'autonomie est une propriété du *pipeline*, jamais une faveur du runtime ni du Trigger.** Le tool ne court-circuite jamais l'humain de sa propre initiative ; c'est le *designer* qui inscrit les actions durables dans le graphe (nœud Shipper avec `gh pr create`, nœud de merge vers main, etc.). Conséquence directe : un pipeline auto-shippant se comporte **à l'identique** qu'il soit lancé à la main ou par un Trigger — aucune divergence manuel/automatique. La confiance se construit et s'audite sur le *pipeline*, pas sur le déclencheur.

Conséquences :

- **Tout NodeRun est attachable** en tmux à n'importe quel moment ; l'utilisateur peut intervenir, converser, corriger.
- **Un Node peut être marqué `interactive: true`** à l'édition. Quand son NodeRun spawn, il s'arrête en attente que l'utilisateur attache la session et signale la complétion (slash command, fichier sentinelle, ou autre — TBD). Cas typique : nœud d'entrée qui grille l'utilisateur pour construire l'input du pipeline (à la `grill-with-docs`).
- **Le Pipeline Manager** est conversationnel et permet de débloquer des Runs (relancer un cycle pour N itérations de plus, etc.) — pas juste de lire l'état. Il vit dans l'onglet info de la toolbar (cf. *UX — un seul mode d'édition unifié*).
- **Aucune action durable auto par le runtime lui-même.** PDO ne merge, ne PR, ne cleanup **jamais de sa propre initiative**. Si ces effets se produisent, c'est qu'un **nœud du pipeline** les exécute — choix explicite du designer, versionné dans le graphe, auditable. (Révise l'ancien « pas d'auto-merge, jamais » : l'interdit ne porte plus sur l'*effet* mais sur son *origine* — jamais le runtime, toujours le pipeline.)
  - **« auto-cleanup » vs « reapable surfacing » (#128).** Faire **supprimer** worktrees/branches par le runtime de lui-même = `auto-cleanup` = **interdit** (ADR-0012(a) ; un `git branch -D` est irréversible, même classe d'effet que merge/PR). **Exposer** les candidats sans rien supprimer = `reapable surfacing` = **autorisé** : le runtime *liste*, la suppression reste au pipeline/humain via `cleanup_run`. C'est ce que fait `GET /runs/reapable` (lecture seule). La recette `docs/recipes/disk-janitor.md` ferme le disk-fill non-surveillé en câblant ce surfacing à un Trigger cron qui appelle `cleanup_run` — l'autonomie reste *dans le pipeline*.
  - **Reapable run** *(terme)* : un Run **terminal** (`completed`/`failed`/`halted`/`skipped`) et **pas encore `archived`**, dont le(s) worktree(s) sur disque existent encore. Son disque est récupérable via `cleanup_run`. Le *surfacer* est lecture seule et permis au runtime ; *exécuter* la récupération est une action pipeline/humaine (ADR-0012).

À distinguer de *Sharp tool* (ADR-0001) : *Sharp tool* parle de l'**éditeur** (on ne contraint pas le design). *Deliberate, then autonomous* parle du **runtime** (on ne court-circuite pas l'humain de force ; on lui laisse *choisir* d'inscrire l'autonomie dans son pipeline).

---

## Édition pendant un Run

Le canvas est **toujours interactif** (ADR-0007). L'ancienne dichotomie "mode Edit" vs "mode Run" avec toggle global est **obsolète** — un seul mode d'édition, qui s'adapte selon que la pipeline tourne ou pas.

### Modèle de mutation

- **Quand aucun Run ne tourne** sur une pipeline : l'édition modifie directement la template en bibliothèque (`~/.pdo/library/pipelines/<id>.yaml`).
- **Quand un Run tourne** : l'édition modifie le **snapshot run-scope** (`<repo>/.pdo/runs/<run-id>/pipeline.yaml`) ET propage la même modif vers la template d'origine en bibliothèque (auto-sync montant). Le pipeline_watcher observe le snapshot run-scope et émet un event `PipelineModified` à chaque mutation ; le scheduler se réajuste au prochain tick (la fonction est pure, pas de cache à invalider).
- **Contrat de l'event `PipelineModified`** : `payload.kind` vaut `"yaml"` (mutation de `pipeline.yaml`) ou `"prompt"` (mutation d'un fichier sous `pipeline.prompts/`), exclusivement — la décision est **par chemin** (`detect_run_scoped_change`). Les events ne sont **jamais coalescés** entre fichiers : une édition YAML et une édition prompt rapprochées produisent deux events distincts. Un même run peut légitimement émettre les deux kinds presque simultanément (la copie initiale du snapshot déclenche le watcher) ; tout consommateur qui attend un kind précis doit donc **filtrer sur `payload.kind`** plutôt que prendre le premier event venu (#182).
- **`PipelineModified` n'altère jamais le statut du Run (#221)** : dans la projection, c'est un signal **passif** — il peut provenir d'une écriture parasite ou étrangère (y compris pour un node absent du DAG du Run). Il ne ré-ouvre donc **aucun** Run terminal : un Run qui a atteint `RunCompleted` reste `Completed`, exactement comme `Failed`/`Halted` (intégrité de l'état terminal). Pour un Run **vivant** (`Running`/`AwaitingUser`), aucun changement de statut n'est nécessaire : `spawn_ready_after_event` relit le fichier au tick suivant et amorce les nodes nouvellement ajoutés. Reprendre un Run **terminé** pour y consommer du nouveau travail est une opération **explicite** (`resume_run`), pas un effet de bord du watcher. (Avant le fix, ré-ouvrir un `Completed` le laissait *phantom-`running`* indéfiniment — sans chemin de re-complétion fiable —, retenait sa session manager + worktree, faisait *skip* tout déclenchement de trigger en overlap-`skip`, et laissait un `resume_run` ultérieur re-spawner une boucle déjà satisfaite.)

### Politique de mutation pendant un Run

- **Suppression** : interdiction stricte de supprimer un node de status non-`pending`. Les nodes `running` ou `completed` restent dans le graphe (le designer peut juste déconnecter leurs edges s'il veut les neutraliser).
- **Modif config** : le `max_iter` d'un Loop live peut être modifié à chaud — équivaut à la commande `extend_cycle` du Pipeline Manager, qui devient redondante.
- **Ajout de node + edge** : libre. Si la nouvelle edge active un node non-encore-spawné, le scheduler le pickup au prochain tick. Les nodes already-completed/running ne re-tournent pas — modif sans effet sur leur iter en cours, mais visible à l'iter suivante (Loop).

### Étanchéité

- Modif d'un run-snapshot n'impacte aucun autre run en cours (chaque run a son propre snapshot).
- Modif d'une template hors-Run n'impacte aucun run en cours (qui ont déjà leur propre snapshot).
- L'auto-sync montant ne va que du run-scope vers la template, jamais l'inverse.

---

## Pipeline Run — cycle de vie

### Input

Un Run prend un **input unique**, qui est soit :

- du **free-text prompt** (description en texte libre),
- une **référence d'issue** GitHub (URL ou `#123` — résolue via `gh issue view`),
- un mélange des deux dans le free-text (l'utilisateur colle un lien d'issue dans son prompt — le nœud d'entrée, qui est un Claude Code avec accès à tous ses tools/MCP, va lui-même chercher l'info).

Le runtime ne distingue pas (i) de (ii) : il pose le contenu utilisateur tel quel dans un artefact `<artifacts>/_input.md` du Blackboard. Le nœud d'entrée se débrouille à partir de là.

L'input peut aussi être **construit interactivement** via un nœud d'entrée marqué `interactive: true` (cf. principe *Deliberate over autonomous*). Pattern typique : le user écrit un prompt brut court, attache la session du nœud d'entrée, l'agent grille jusqu'à un input structuré, le user "submit", le pipeline démarre vraiment.

#### Images d'input

Le user peut **téléverser des images** à côté du prompt texte (New Run modal). Le runtime les stocke dans `_input/` du Blackboard, à côté de `output.md`, et le préambule du nœud d'entrée les liste. Ces images sont **portées par le nœud Start** : le `StartNodeInfo` projeté expose `input_images` (les noms de fichiers, dans l'ordre de téléversement). L'UI les affiche en bandeau de vignettes sur la carte Start du canvas et en pleine taille (cliquables, lightbox) dans le StartInspector, aux côtés du prompt. Un Run sans image rend le nœud Start et le StartInspector à l'identique (prompt seul).

### `prompt_required` — pipeline runnable sans prompt

Flag racine du pipeline YAML (à côté de `variables:`), **défaut `true`** (préserve le comportement actuel). Mis à `false`, le pipeline est *self-sufficient* : son nœud d'entrée sait trouver son propre travail (lire le backlog, `git diff main`, etc.). Rendu UI : case « Prompt required » cochée par défaut, décochable.

- **New Run modal** : si `prompt_required: false`, le champ prompt devient optionnel (`canLaunch` ne l'exige plus). Un prompt fourni est passé comme *« additional info »*, pas comme tâche principale.
- **Runtime** : un input vide n'est légal que si `prompt_required: false`. Le préambule du nœud d'entrée s'adapte — avec input : « additional info : … » ; sans : le nœud source son travail lui-même.

### Termination

À la fin d'un Run réussi, **niveau 0** par défaut : la branche `pdo/run-<run-id>` reste en l'état, le worktree reste sur disque, l'utilisateur fait ce qu'il veut. PDO ne fait **pas** de PR auto, **pas** de commentaire d'issue, **pas** d'auto-merge. Si un projet veut ce comportement, il l'exprime en ajoutant un nœud "Shipper" dans son pipeline (un Claude Code avec `gh pr create` dans son prompt).

### Échec / blocage

NodeRun en échec, halt déclenché par une `when:` clause (`run_halted` event), Merge node foiré, etc. → le Run passe en status `BLOCKED` ou `FAILED`. La branche pipeline et les sous-worktrees restent vivants pour debug. **Pas d'auto-cleanup, jamais.** L'utilisateur peut :

- Cleanup manuel intégral (suppression branches/worktrees).
- Reprendre la main directement sur la branche.
- Débloquer via le **Pipeline Manager** : conversation au cours de laquelle le user peut, par exemple, demander *"continue le cycle pour 3 itérations de plus"*. Le manager dispose des commandes pour modifier l'état runtime.
- Éditer le graphe à chaud (ADR-0007) — ajouter un Reviewer, déconnecter une edge bloquante, etc.
- Automatiser le cleanup **sans réintroduire l'auto-cleanup runtime** : `GET /runs/reapable` *surface* (lecture seule) les Runs terminaux dont le worktree traîne encore ; une pipeline janitor + Trigger cron exécute la récupération via `cleanup_run`. Recette : `docs/recipes/disk-janitor.md` (#128, Track A). L'origine de la suppression reste *dans le pipeline*, jamais le runtime.

### Parallélisation entre Runs

Plusieurs Runs du même pipeline (ou de pipelines différents) peuvent tourner simultanément sur le même repo target. Convention de nommage qui garantit l'absence de collision :

- Branche : `pdo/run-<run-id>` (ex. `pdo/run-2026-05-05-1430-a3f`).
- Worktree pipeline : `<repo>/.pdo/runs/<run-id>/worktree/`.
- Sous-worktrees `code-mutating` : `<repo>/.pdo/runs/<run-id>/nodes/<node-id>/iter-<N>/`.
- Blackboard : `<pipeline-worktree>/.pdo/artifacts/...` (déjà défini).

`<run-id>` = slug `<timestamp>-<short-uuid>` pour rester lisible humainement et garanti unique.

**Nom placeholder (placeholder name)** :
Nom lisible posé par le daemon au spawn du Run, déterministe et immédiat (dérivé du timestamp du
run-id), garanti présent même pour un Run prompt-less ou déclenché par Trigger.
_Éviter_ : nom temporaire, titre par défaut.

**Nom descriptif (descriptive name)** :
Nom lisible posé best-effort par le Pipeline Manager dans son propre tour, une fois qu'il sait ce que
fait le Run ; remplace le placeholder s'il aboutit, sans jamais le supprimer (un Run a toujours un nom).
_Éviter_ : nom final, nom auto, rename automatique.

### Statistiques de Run

Le panneau d'info d'un Run expose un petit bloc de stats (cf. #100, #272). **Quatre métriques** ; le **coût** est une **estimation** — pas une facture — dérivée des transcripts Claude Code locaux (`~/.claude/projects/<cwd-encodé>/*.jsonl`) : somme des `usage` par message (dédupliqués par `(message.id, requestId)`) × table de prix publics par modèle (cache dérivé 1.25×/2×/0.1× de l'input). Reversé #272 (2026-07-06, ratifié par le propriétaire) : le blocage historique (« aucune télémétrie fiable ») était faux — la télémétrie n'est pas requise, les transcripts portent l'usage. Modèle inconnu → $0 + drapeau « borne basse » ; aucun `costUSD` dans les transcripts (mode *calculate*). Voir ADR-0022.

- **Durée** : temps écoulé entre `started_at` et `completed_at`. **Dérivée à l'affichage** (frontend) à partir des deux timestamps déjà projetés depuis l'event log — pas de `duration_ms` backend (qui figerait un Run vivant). Horloge **live** (tick) tant que le Run est vivant (`Running`/`AwaitingUser`/`Paused`) ; figée à `completed_at` à l'entrée terminale. La durée est du **wall-clock** : un Run `Paused` continue de compter (le temps de pause est inclus).
- **Sessions de nœud lancées** (*node sessions started*) : **compte cumulatif** des événements `NodeStarted` sur tout le Run. Mesure les sessions tmux NodeRun réellement spawnées — **y compris** les re-spawns au **même** `(node, iter)` (restart/recovery), donc ≥ le nombre de `(node, iter)` distincts (une projection dédupliquée par `(node, iter)` *sous-compterait*). Le **Pipeline Manager** n'émet pas `NodeStarted` → exclu par construction (« exclure le manager » est un no-op). Distinct de la gauge « sessions vivantes » du cap (cf. *Cap de sessions concurrentes*).
- **LOC** (lignes changées par le Run) : `git diff --numstat` en **trois-points** (`HEAD...pdo/run-<run-id>`) — la base est le **point de fork** (merge-base), donc stable même si `main` avance (un diff deux-points dériverait). **Exclut `.pdo/`** (artefacts/prompts générés ne sont pas du code produit ; protégé par `.gitignore` mais un pathspec `:(exclude).pdo/` défensif couvre les repos cibles externes). **Dérivé du git, live-only** : la branche `pdo/run-<run-id>` est supprimée au cleanup → la stat affiche **« — »** pour un Run archivé/nettoyé (branche absente = `None`), à distinguer de **« 0 »** (diff réellement vide). Même schéma que le snapshot de pane qui survit au reap.
- **Coût (est.)** : `Some { usd, partial }` **dérivé à la lecture**, jamais persisté (comme LOC), dans `run_cost::compute_run_cost`. Agrège TOUTES les sessions du Run (nœuds, manager, merge-resolver, subagents) via prefix-glob sur `~/.claude/projects/`. `None` → « — » quand aucun transcript n'est trouvé. **Plus durable que LOC** : le cleanup supprime la branche (LOC → « — ») mais **pas** `~/.claude/projects/`, donc un Run archivé garde son coût. `partial: true` (un modèle non tarifé a contribué, donc exclu) → borne basse, signalée par un « † » dans l'UI. Encodeur de chemin **propre** (`cc_project_dirname`), volontairement distinct de `stale_detector::encode_working_dir` (bogué, à corriger séparément — cf. ADR-0022 et le doc-comment).

### Contrôles de Run (niveau Run)

Trois commandes agissent sur le **Run entier**, à ne pas confondre avec les commandes niveau-nœud (`retry_node`, `restart_node`, `stop_node`). L'UI (liste des Runs) les expose en actions de ligne gatées par status — c'est la couche 3 d'ADR-0009. Le gating client est un sous-ensemble strict des status acceptés par le daemon : aucun bouton affiché ne peut donc produire un 409.

- **Pause / Resume** — `pause_run` fait passer un Run **vivant** (`Running`/`AwaitingUser`) en `Paused` : aucun nouveau nœud n'est spawné, mais l'horloge de durée continue de tourner (le temps de pause est inclus, cf. *Statistiques de Run*). `pause_run` est refusé (409) sur tout autre status, y compris un Run déjà `Paused`. `resume_run` ramène un `Paused` en `Running`. `resume_run` est **dual-purpose** : sur un Run `Halted`/`Failed` il **relance** depuis l'état courant (re-drive post-conflit résolu, cf. *Échec / blocage*), ce qui n'est pas une simple levée de pause — l'UI ne propose toutefois Resume que sur `Paused`.
- **Retry-all** *(terme canonique)* — `retry_all` sur un Run **terminal** (`Completed`/`Failed`/`Skipped`/`Halted` ; **jamais** `Archived`) **archive le Run d'origine** (via `cleanup_run`) puis **crée un Run neuf** avec les mêmes pipeline / input / variables / repo / branche et un nouveau `run-id` (réponse `201 Created`). Le nouveau Run **ne porte aucune référence de filiation** vers l'ancien : il est indiscernable d'un lancement manuel (pas de `retry_of`). L'UI, derrière une confirmation, sélectionne d'office ce Run *offspring* au retour.
  _Éviter_ : « retry » tout court (réservé au niveau **nœud** — `retry_node` ré-exécute un seul nœud et invalide son aval, cf. ADR-0009), « relancer le même Run » (le `run-id` change et l'ancien est archivé).

## Repo cible (`target_repo`)

Le **repo cible** d'un Run ou d'un Trigger est le dépôt git dans lequel il travaille (worktrees, artefacts, exécution du guard). Chemin absolu, stocké **verbatim** — jamais canonicalisé (`validate_target_repo`).

- **Absent ⇒ `repo_root` du daemon.** Un Run/Trigger sans `target_repo` explicite s'exécute contre le dépôt racine du daemon. La résolution est **côté serveur** (`effective_repo_root`) : tout point de lecture qui a besoin d'un chemin concret (détail de Run, et désormais les listes Runs/Triggers) substitue `repo_root`. Conséquence : **pas de bucket « Unassigned »** — un Run sans cible et un Run ciblant explicitement le `repo_root` sont le *même* projet (≈ 46/101 runs de dev n'ont pas de `target_repo`).
- **Clé de regroupement des listes (« par projet »).** Les listes Runs et Triggers se regroupent par repo cible résolu. Regroupement **conditionnel** : un en-tête par repo n'apparaît que si la liste contient **≥ 2 repos distincts** ; sinon (cas mono-repo courant) la liste reste **plate, identique à avant** — aucun en-tête, aucun badge ajouté. Seuil calculé **par liste** (l'onglet Runs et l'onglet Triggers sont indépendants) et sur les **lignes actives** de chaque liste. Côté Runs, les Runs `archived` sont **exclus du seuil** : ils sont extraits vers une section « Archived » repliable et plate, sous les groupes actifs (#136) — le regroupement par repo ne s'applique donc qu'aux Runs actifs. Côté Triggers, il n'existe pas de notion d'archive : toutes les lignes comptent. Clé = chemin complet (deux repos de même basename ⇒ deux groupes distincts) ; libellé = basename, chemin complet au survol, **suffixe discriminant minimal** en cas de collision de basename (`/a/foo` + `/b/foo` ⇒ « a/foo » + « b/foo »). Tri des groupes : alphabétique par chemin complet (déterministe) ; ordre intra-groupe = ordre serveur préservé (Runs `run_id DESC`, Triggers `created_at DESC`).
- **`effective_repo` (résolu) ≠ `target_repo` (brut).** Le champ brut `target_repo` (nullable) reste la valeur saisie par l'utilisateur — il pilote le badge repo de la ligne Trigger, le panneau détail, le pré-remplissage Run-now. Le champ résolu `effective_repo` (toujours concret, exposé par les *endpoints de liste* uniquement) ne sert qu'à la clé de regroupement. **On ne réécrit jamais `target_repo` côté serveur** : sinon badge/détail/pré-remplissage afficheraient un repo jamais saisi en mono-repo (régression). Le regroupement vit **côté client** (UI réversible) ; le serveur se contente de résoudre la clé.
- **Repos récents (`GET /repos/recent`).** Projection *à la lecture* des `target_repo` portés par les événements `RunStarted` : jusqu'à 5 chemins distincts, plus récent d'abord. Comparaison **verbatim** (cohérent avec la règle jamais-canonicaliser ci-dessus : `/a/repo` et `/a/repo/` comptent comme deux entrées). Les Runs lancés sans `target_repo` explicite ne contribuent pas aux récents.

---

## Trigger

Un **Trigger** est une liaison nommée et persistée entre une **condition de déclenchement** et un **template de Run**. Quand la condition se réalise, PDO crée un Pipeline Run *ordinaire* à partir du template.

- **Template de Run** = exactement la charge utile d'un `POST /runs` : pipeline (depuis la bibliothèque) + repo cible + source branch + input + overrides de variables.
- **Start-only.** Un Trigger sait *quand* déclencher et *quel input* passer — rien de plus. Il ne décide jamais de la terminaison du Run (pas de policy de finish côté Trigger, cf. *Deliberate, then autonomous*). L'autonomie de bout-en-bout (push/PR/merge) est une propriété du **pipeline** visé (nœud Shipper), pas du Trigger.
- **Provenance.** Un Run créé par un Trigger porte une référence `triggered_by: <trigger-id>` ; à part ça c'est un Run ordinaire, indistinguable dans son cycle de vie.
- **Pas de chaînage interne.** Un Trigger ne déclenche pas un autre Trigger. Les pipelines se couplent par le **monde extérieur** (ex. un pipeline auditeur écrit des issues GitHub `ready-for-agent` ; un Trigger de polling les ramasse), jamais par un wiring interne PDO — cohérent avec « les Runs ne partagent pas de blackboard ».

### Condition de déclenchement

Un Trigger porte un **heartbeat cron** (obligatoire) et un **guard script optionnel** :

- **Sans guard** : à chaque tick cron, le Trigger fire — un Run est spawné. (Le pipeline visé est typiquement *self-sufficient*, cf. #137 : pas d'input requis.)
- **Avec guard** : à chaque tick, PDO exécute d'abord le script (cheap, avant tout spawn). Contrat : **exit 0 ⇒ fire ; exit non-zéro ⇒ skip** (aucun Run spawné, pas de pollution de la liste). **Le `stdout` du guard devient l'input du Run** (stdout vide ⇒ pas d'input). Exemple issue-polling : `gh issue list --label ready-for-agent --json number,title` → vide ⇒ exit 1 (skip) ; non-vide ⇒ exit 0, la liste sert d'input.

**Un firing = un Run.** PDO ne fan-out jamais un Run par work-item. Si le guard ramène N issues, c'est *un* Run dont l'input liste les N issues ; la multiplicité est gérée *dans le pipeline* par une boucle `collection` (ex-ForEach, « un fixer par issue »). Le Trigger reste bête : il démarre un Run.

**Exécution du guard** : lancé `sh -c "<command>"` avec **CWD = `target_repo`** (pour que `gh issue list` / `git log` marchent dans le contexte du repo sans chemins en dur), héritant l'environnement du daemon (auth `gh`, PATH). Variable `PDO_TARGET_REPO` injectée. **Timeout dur 60 s** (configurable via la *Configuration d'instance*, en secondes ; #129, ADR-0015 — l'env `PDO_GUARD_TIMEOUT_MS` reste le seam de test), exécuté **hors du thread de tick** (task spawnée) : un guard qui hang ne doit jamais geler le scheduler — dépassement ⇒ kill, `guard-error (timeout)` dans `trigger_fires`, fire sauté, le tick reste réactif.

**Références cassées** : si le pipeline (library) ou le repo cible d'un Trigger a été supprimé/renommé depuis la création, le Trigger **ne fire plus et affiche un `last_outcome` d'erreur** (« pipeline not found ») dans l'onglet — pas d'auto-suppression, pas de pourrissement silencieux (*Sharp tool* : on surface, on ne masque pas).

**Résolution de l'input du Run déclenché**, dans l'ordre : `stdout` du guard (s'il existe et non-vide) → `input_template` statique du Trigger → rien. Si l'input résolu est vide *et* que le pipeline a `prompt_required: true`, le Trigger est **rejeté à la création** (erreur claire : « ce pipeline exige un prompt ; ajoute un guard, un input template, ou passe le pipeline en prompt-not-required »). Échec loud au config-time plutôt qu'un nœud d'entrée paumé toutes les 15 min. (Le guard prime quand présent ; pas de merge template+stdout en v1 — `echo` ton texte statique dans le guard si tu veux les deux.)

### Idempotence — déléguée au monde extérieur

**PDO ne tient aucun état de dedup.** Pas de `fired_keys`, pas de mémoire « ai-je déjà traité ce work-item ». L'idempotence est une responsabilité de l'utilisateur (*Sharp tool*), naturellement satisfaite quand le pipeline **mute l'état qu'il poll** : le nœud Shipper du fixer relabel/ferme l'issue (`ready-for-agent` → `in-progress`/closed), donc le prochain `gh issue list --label ready-for-agent` du guard ne la voit plus. Le label GitHub *est* le registre de dedup.

Risque assumé : un guard qui renvoie toujours le même work + un pipeline qui ne mute pas l'état ⇒ Runs dupliqués en boucle, bornés seulement par la politique de recouvrement. Choix v1 délibéré : pas de moteur de dedup qui *devine* l'intention ; la boucle est à l'utilisateur de la fermer.

### Politique de recouvrement — skip

Un Trigger **ne fire pas** si **son propre** Run précédent est encore vivant (`running`/`awaiting_user`/`blocked`). Le tick est sauté (loggué « skipped — previous run still active »), pas mis en file. Justification : ferme la fenêtre de course du dedup-par-label (Q4) — tant que le fixer-run-1 travaille l'issue #42, l'issue reste `ready-for-agent`, donc sans skip les polls suivants re-firent sur #42. Le skip est le défaut, surchargeable en `allow` par-Trigger pour qui veut des fires concurrents.

**Recouvrement borné — `allow` + `max_concurrent`** (#239) : `allow` accepte un plafond optionnel `max_concurrent` (entier ≥ 1, nullable). `None`/vide ⇒ concurrence illimitée (compat ascendante des `allow` existants) ; `Some(m)` ⇒ fire tant que le Trigger a `< m` Runs vivants *à lui*, puis skip. Les trois modes se ramènent à un **plafond effectif** unique : `skip` ⇒ plafond 1 (jamais d'empilement sur son propre Run vivant), `allow+None` ⇒ illimité, `allow+Some(m)` ⇒ m. La décision (`fire_decision::overlap_ceiling`) et la gate du guard passent toutes deux par ce même plafond, donc le guard ne tourne jamais sur un tick qu'on sauterait. Le décompte est dérivé de la provenance `triggered_by` (Runs vivants), pas d'un état neuf : un Run qui se termine libère un slot.

**Pas d'empilement de Runs en attente.** On ne crée jamais un Run entièrement « en attente ». La seule attente possible est *au niveau nœud* (back-pressure du cap de sessions, ci-dessous), à l'intérieur d'un Run déjà admis.

### Mécanisme cron & cycle de vie

- **Format** : expression cron 5 champs (crate cron + `chrono`), interprétée en **UTC** (cohérente entre première planification et recalculs). L'UI propose des presets (toutes les 15 min / horaire / quotidien 09:00) compilés en cron + une échappatoire expression brute. (Cron en heure *locale* = enhancement séparé ; cf. #222, hors scope.)
- **Scheduler** : nouvelle task background (`tokio::time::interval`, sœur du reaper/stale) qui tick ~toutes les 30 s ; résolution cron à la minute. À chaque tick : pour chaque Trigger activé dont `next_fire ≤ now`, applique le skip de recouvrement, exécute le guard, spawn le Run, recalcule `next_fire`.
- **Invariant `next_fire_at` = UTC canonique (`…Z`)** (#222) : tout writer (création/édition/scheduler) stocke en UTC, et la requête « quels Triggers sont dûs » compare/ordonne via `julianday()` (tz-normalisé), donc une ligne à offset local (donnée legacy ou régression `Local::now()`) ne peut plus se mettre en dormance silencieuse pendant des heures.
- **Résilience du tick** (#222) : un panic pendant un tick est **isolé** (frontière `tokio::spawn`) — la boucle survit et le tick suivant rattrape les Triggers non firés (forward-only). `GET /triggers/health` expose `last_tick_at` + l'intervalle, pour qu'un scheduler mort/bloqué soit observable plutôt que silencieux.
- **Fires manqués = forward-only, pas de backfill.** Daemon down pendant 50 slots ⇒ au redémarrage `next_fire` est recalculé depuis *now*, les slots manqués ne sont pas rejoués. Correct *par construction* : le dedup étant externe, un seul poll forward voit *tout* le travail accumulé (`gh issue list` ramène toutes les issues en attente d'un coup).
- **Daemon best-effort par défaut, persistant sur demande (#156)** : un `pdo daemon` lancé à la main ne fire que tant que le process vit (survit à la fermeture de l'UI, meurt au reboot/logout). `pdo service install` le rend **persistant** — installé comme **service unit** (systemd `--user` sous Linux, LaunchAgent launchd best-effort sous macOS), il démarre au boot et survit au logout, résolvant la limitation v1 que l'onglet Triggers signalait. La status-bar distingue désormais un daemon persistant (silencieux) d'un daemon **éphémère** (pastille ambre `ephemeral` pointant sur `pdo service install`). Détails : *Service unit persistant (#156)* + ADR-0019.

### Persistence — table SQLite

Les Triggers vivent dans une **nouvelle table `triggers`** de `~/.pdo/pdo.db`, *pas* en YAML sur disque. Un Trigger est de la **config + état de scheduling** (créé via modale, pas un artefact canvas-backed comme un pipeline), et son état mutable (`enabled`, `next_fire_at`, `last_fired_at`, `last_outcome`) serait réécrit à chaque tick — mauvais fit YAML. La requête centrale du scheduler (« quels Triggers sont dûs ») est une requête indexée triviale.

Ligne : `id, name, pipeline_id, target_repo, source_branch, input_template, variables(JSON), cron, guard_command(nullable), overlap_policy, max_concurrent(nullable), enabled, next_fire_at, last_fired_at, last_outcome`. (Pas de migration runner : la colonne `max_concurrent` est ajoutée à `init` via un `ALTER TABLE … ADD COLUMN` idempotent, gardé par un `pragma_table_info`, pour migrer les `pdo.db` antérieurs à #239. Même précédent pour les colonnes `guard_stdout`/`guard_stderr`/`guard_exit_code` de `trigger_fires`, ajoutées à `init` pour migrer les bases antérieures à #244.)

Ne viole pas l'event-sourcing : l'event log reste la vérité du **Run** (keyé `run_id`) ; un Trigger *produit* des Runs (eux event-sourcés normalement, avec provenance `triggered_by`).

**Table `trigger_fires`** (audit) : un enregistrement horodaté par tick significatif — `fired→run_id` / `skipped-overlap` / `guard-exit-nonzero` / `guard-error`. Répond à la question #1 du debug (« pourquoi mon Trigger n'a pas firé cette nuit ? »). L'onglet Triggers la lit pour « last fired / last skipped + raison ». Un skip dû au plafond borné (#239) **réutilise l'outcome `skipped-overlap`** (pas de nouveau statut à apprendre à l'UI), le plafond étant porté dans la *raison* (« max concurrent runs reached (2/2) ») — la colonne raison du panneau d'historique répond au « pourquoi » précisément.

**Capture de la sortie du guard (#244)** : sur une ligne `guard-exit-nonzero`, PDO conserve désormais ce que le guard a imprimé — colonnes additives `guard_stdout`, `guard_stderr`, `guard_exit_code` (exit code `NULL` si tué par signal). Chaque flux est **plafonné à 16 KB, queue conservée** (un marqueur de troncature préfixe le reste, car l'erreur s'imprime en général en dernier). Indispensable pour les guards type grep (`gh issue list … | grep .`) : sur « rien à faire » le stdout est vide, le *pourquoi* vit sur stderr + l'exit code. Le panneau détail révèle ces champs derrière une **disclosure par-ligne « Guard output »** (repliée par défaut), uniquement sur les lignes `guard-exit-nonzero` ; un `guard-error` (spawn/timeout) n'a pas de flux capturé (son détail reste dans `reason`). **Côté skip ces flux sont purement diagnostiques** : ils n'altèrent pas le contrat d'input (cf. *Résolution de l'input* ci-dessus — seul le stdout d'un `Pass` devient l'input du Run, et lui n'est jamais plafonné). En capturant stderr, on draine désormais ce flux en continu, ce qui corrige au passage un deadlock latent (un guard inondant stderr > ~64 KB bloquait jusqu'au timeout et était mal classé `guard-error`).

### UI — onglet Triggers

- **Ligne** : status dot (depuis `last_outcome`, tooltip au survol « last run date : XXX, result : YYY ») · nom · pipeline · badge repo · planning lisible (« every 15 min ») · toggle enable/disable · « next fire in … / last fired … ». Actions au survol : run-now, edit, delete. Langage visuel calqué sur les lignes Runs.
- **Sélection → panneau détail droit** : config complète + **historique des fires** (table `trigger_fires` : horodatage · outcome · lien run-id). C'est là qu'on répond à « pourquoi pas firé cette nuit » (skips et guard-errors listés avec raison).
- **Run now** = ouvre `NewRunModal` en mode Run-now pré-rempli depuis le Trigger (pipeline, repo, branch, variables, input_template→prompt) ; lancement manuel. **Le guard n'est pas exécuté** (c'est une gate de polling, pas une partie du template). Sidestep l'ambiguïté guard/overlap, et sert de « tester ce que fait ce Trigger ».
- **Trigger désactivé** : reste dans la liste, grisé, ne fire pas — le contrôle « pause de la boucle d'audit ».

---

## Pipeline Manager

Agent conversationnel attaché à un Pipeline Run. Permet à l'utilisateur de **lire l'état** et **émettre des commandes** sur le Run.

### Cycle de vie

- **Un manager par Run.** Spawn automatique au démarrage du Run dans une session tmux dédiée nommée `pdo-mgr-<run-id>`. Persiste tant que le Run n'est pas cleanup (donc aussi après success/failed/blocked, pour interrogation post-mortem).
- **Pas de polling actif.** Le manager ne tourne effectivement que quand l'utilisateur lui parle. Quand attaché, il lit l'état frais à la demande.
- **Nommage descriptif dans son propre tour.** Quand un Run porte un *nom placeholder* (posé par le daemon au spawn, cf. *cycle de vie*), le manager lui donne un *nom descriptif* via `rename_run` **best-effort, à l'intérieur de son propre tour** — une fois qu'il sait ce que fait le Run, jamais réveillé par le daemon ni par un nœud (cohérent avec « Pas de polling actif »). Pour un Run prompt-less purement non-attendu, le manager n'a souvent jamais le contexte : le placeholder persiste, et c'est le comportement voulu (#184).

### Implémentation

- Le manager **est** une instance Claude Code standard, pas un agent custom.
- Son **prompt système est augmenté** par le runtime avec :
  - L'identité du Run qu'il gère (`<run-id>`).
  - La liste des **endpoints HTTP** du daemon PDO accessibles (URL de base, schéma, exemples d'invocation curl).
  - La liste des **commandes** disponibles avec leur payload attendu.
- **Pas de MCP custom.** L'agent appelle les endpoints via `bash` + `curl`. Justification : MCP est utile pour des clients agentiques distants/inconnus ; ici on possède le prompt de la session, autant documenter les endpoints en clair.
- Pour la lecture brute (sans passer par les endpoints), le manager a accès à `bash` complet : `ls`, `cat`, `git log`, `tmux capture-pane`, etc. Tout l'état du Run est sur disque, donc grep-able.

### Commandes disponibles (v1)

Toutes exposées comme endpoints `POST /runs/<id>/commands` du daemon :

| Commande | Effet |
|---|---|
| `bump_region` | Accorde N itérations supplémentaires à une région de boucle bornée (`region_id` = id de la région `loops:`) et relance |
| `end_region` | Déclenche la complétion d'une région de boucle bornée sans itération supplémentaire |
| `extend_cycle` | (Legacy, cycles `$var` hors région) Augmente le `max_iter` d'un cycle bloqué de N et relance. Cible = le nœud porteur de l'arête de cycle sortante (nœud de condition de sortie), jamais la tête. Refusé (`409`) si le nœud est membre d'une région `loops:` bornée — utiliser `bump_region` |
| `resume_run` | Relance le Run depuis l'état actuel (utile post-conflit résolu manuellement) |
| `kill_node` | Tue un NodeRun en cours |
| `restart_node` | Re-spawn un NodeRun (perd l'état tmux courant) |
| `mark_node_done` | Force la complétion d'un NodeRun (cas typique : nœud `interactive: true` que l'utilisateur signale fini) |
| `inject_artifact` | Pose un artefact à la main dans le Blackboard |
| `cleanup_run` | Supprime branches, worktrees, artefacts, événements |
| `rename_run` | Donne au Run un nom descriptif (remplace le nom placeholder posé au spawn) |
| `start_node` | Spawne un NodeRun immédiatement, sans attendre la complétion amont (force-spawn) ; inputs résolus best-effort ; refusé (`409`) si le Run n'accepte pas de spawn ou si le cap de sessions est atteint |

L'effet de chaque commande est l'**append d'un événement** dans l'event log. Le runtime consomme ces événements et agit en conséquence.

### Contrat de réponse des commandes (ADR-0025)

Les commandes de pilotage de boucle (`extend_cycle`, `bump_region`, `end_region`, `resume_run`) disent la vérité sur leur effet :

- **Cible inconnue** (`node_id` ou `region_id` absent du pipeline du Run — snapshot du Run, pas la bibliothèque) → `400` `{"error":"node '<id>' not found in pipeline"}` (resp. `region '<id>'`). La validation précède l'append du `CommandIssued` **et** la levée du `Halted` : une commande rejetée ne modifie pas l'event log.
- **Mauvais mécanisme** (`extend_cycle` sur un membre de région bornée) → `409` `{"error":"node '<id>' is a member of loop region '<region>'; use bump_region with region_id '<region>'"}`.
- **Valide mais sans effet immédiat** (itération encore vivante, région déjà complétée, throttle d'admission) → `200` `{"ok":true,"noop":true,"reason":"..."}` (même convention que `mark_node_done`).
- **Effet réel** → `200` `{"ok":true,"spawned":[{"node_id":...,"iter":...}]}`.

### Ce que le manager ne peut **pas** faire

- **Spawner des sous-agents ad hoc hors-DAG.** Pas d'orchestration probabiliste émergente. Le manager parle, lit, et exécute des commandes prédéfinies. Il peut en revanche force-spawn un nœud **déjà déclaré** dans le DAG via `start_node` (hors ordre de dépendance). Si l'utilisateur veut une investigation profonde, il attache directement la session tmux du nœud concerné.

---

## Architecture runtime — event-sourced

### Source de vérité = event log

Toutes les transitions d'état d'un Pipeline Run sont enregistrées comme **événements append-only** dans une **SQLite locale** (`~/.pdo/pdo.db`). L'état courant d'un Run = projection des événements de ce Run.

Pas de "state.yaml" ou "STATE.md" stocké en plus. Seul l'event log persiste.

Schéma indicatif (à raffiner) :

```sql
CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  ts TEXT NOT NULL,
  kind TEXT NOT NULL,        -- 'run_started', 'node_started', 'node_completed',
                             -- 'cycle_iteration', 'merge_conflict', 'command_issued', etc.
  node_id TEXT,
  iter INTEGER,
  payload JSON               -- métadonnées arbitraires : artefacts produits, session tmux, exit code, etc.
);
```

### Daemon PDO

Process local toujours-actif (lazy start) qui :

- Héberge le **serveur HTTP** (REST + WebSocket).
- Est l'**ordonnanceur** : il lit l'event log, détermine quels NodeRuns sont prêts, spawn les sessions tmux + sous-worktrees, écoute leur complétion, append les événements correspondants.
- Sert d'**API surface** unique pour : la session manager, l'UI web/desktop, et tout futur client.

### Endpoints (esquisse, à raffiner)

```
GET    /pipelines                          — liste des définitions de pipelines
GET    /pipelines/<id>                     — définition d'un pipeline

POST   /runs                               — démarre un nouveau Run (body: pipeline-id + input)
GET    /runs                               — liste des Runs (filtrable par statut)
GET    /runs/<id>                          — projection d'état courante d'un Run
GET    /runs/<id>/events                   — historique brut
GET    /runs/<id>/events?subscribe         — WebSocket push des nouveaux events
GET    /runs/<id>/nodes                    — état de tous les NodeRuns
GET    /runs/<id>/nodes/<node-id>          — détail d'un NodeRun (statut, iter, session tmux, artefacts)
POST   /runs/<id>/commands                 — émet une commande (body: { kind, payload })

GET    /repos/validate?path=               — valide un repo cible (chemin absolu + is_dir + `git rev-parse`)
GET    /repos/branches?path=               — liste les branches locales du repo donné
GET    /repos/recent                       — jusqu'à 5 `target_repo` distincts, projetés des événements `run_started`, plus récent d'abord
GET    /repos/browse?path=                 — listing filesystem à un niveau (explorateur du New-Run modal, #131 ; durcissement d'exposition fs différé à #260)
```

### Conséquence pour la prompt augmentation

Le préambule runtime injecté dans chaque NodeRun (cf. section *Prompt augmentation*) inclut, en plus des chemins d'inputs/outputs, **l'URL de base du daemon** (`http://localhost:<port>`) pour les nœuds qui en ont besoin (typiquement le manager, mais aussi un nœud "Shipper" qui voudrait poster un commentaire sur l'issue source via les endpoints, etc.).

---

## Configuration d'instance (instance-wide config)

Réglages **daemon-wide** — ils s'appliquent à *toutes* les Runs/Triggers d'une instance PDO, à distinguer d'une variable *pipeline* (scopée à un pipeline) ou d'un override de Run. Livrés par la **page de réglages instance-wide** (#129, ADR-0015). _Éviter_ : « préférences globales », « config » tout court (ambigu avec la config pipeline).

- **Store** : table SQLite **singleton** `instance_config` de `pdo.db` (une seule ligne, `id = 1`, seedée à l'`init` avec les défauts). Même justification que les Triggers (config + état mutable, pas un artefact canvas-backed → mauvais fit YAML, cf. *Persistence — table SQLite*). Nouveau réglage = colonne `ALTER TABLE … ADD COLUMN` idempotente (précédent `max_concurrent` #239), jamais de migration runner.
- **Réglages v1** : (1) **cap de sessions** (cf. *Cap de sessions concurrentes*) ; (2) **reaper TTL** (cf. *Reap sur état terminal*) ; (3) **timeout du guard de Trigger** (cf. *Trigger* — exposé en **secondes**, l'env `PDO_GUARD_TIMEOUT_MS` reste le seam de test en ms). Le troisième est *sécable* : le tracer minimal = cap + reaper TTL.
- **Précédence `stored → env → default`** (ADR-0015) : la valeur **stockée (UI) gagne**, l'env est un bootstrap consulté quand le stored est `NULL`, le défaut est le plancher. _Éviter_ : « l'env gagne » (rendrait la page no-op pour les opérateurs qui l'utilisent).
- **API** : `GET /settings` renvoie par champ `{ effective, source, stored, env, default }` (`source ∈ {stored, env, default}`) — assez riche pour que l'UI **révèle** un env masqué (« `PDO_SESSION_CAP=10` positionné mais surclassé par 30 »). `PUT /settings` écrit le seul tier `stored`, valide **fail-fast** (rejet `400` : cap `< 1`, TTL `< 1`, timeout hors `[1, 600]` s — pas de retombée silencieuse sur le défaut comme le parseur d'env). `GET /sessions` (barre de statut) reste inchangé.
- **Prise d'effet sans redémarrage** : le reaper TTL est lu **une fois au boot** et figé dans la closure de la boucle de balayage — un `PUT` reste un no-op tant que la lecture n'est pas déplacée *dans* le corps de la boucle. Le cap (lu frais par admission) et le timeout guard (lu frais par tick) n'ont pas ce défaut.
- **Hors scope (frontière ADR)** : « le manager vérifie périodiquement le pipeline » reste **exclu** — réveiller le manager depuis le runtime renverse *Pas de polling actif* (cf. *Pipeline Manager*) et touche l'origine-de-l'autonomie d'ADR-0012 ; décision humaine/ADR séparée.

---

## Sessions tmux

### Modèle d'exécution

Chaque NodeRun = **une session tmux détachée** créée par le daemon (`tmux new-session -d -s <name>`). Le contenu de la session est Claude Code en mode interactif, lancé avec le prompt augmenté du Node. Conventions de nommage :

- NodeRun : `pdo-<run-id>-<node-id>-iter-<N>`.
- Manager : `pdo-mgr-<run-id>` (cf. section *Pipeline Manager*).
- Shell de run : `pdo-shell-<run-id>` (cf. sous-section *Shell de run*, #316).

Les sessions sont **invisibles à l'utilisateur** par défaut — pas de fenêtre OS qui s'ouvre. Elles tournent en arrière-plan et survivent au crash de l'UI ou du daemon (le runtime peut récupérer leur état au redémarrage).

### Shell de run — « Open session » (#316, ADR-0021)

**Shell de run** *(terme)* : un **bash interactif ad-hoc** (`bash -i`, pas une REPL Claude Code) spawné à la demande dans une session tmux dédiée `pdo-shell-<run-id>`, cwd = le **worktree pipeline** du Run (`<repo>/.pdo/runs/<run-id>/worktree/`). Sert à inspecter/déboguer un Run post-mortem (lire les fichiers, `git log`/`git diff`, relancer un test). _Éviter_ : « session » tout court (= session tmux NodeRun), « manager » (= REPL conversationnelle attachée au Run), « terminal » (= le pont xterm.js d'attache).

- **Action « Open session »** dans la liste de Runs (à gauche), visible uniquement sur les Runs **terminaux non-archivés** dont le worktree existe encore (= un *Reapable run*). Gate serveur : `is_terminal() && ≠ Archived && worktree_dir_for_run(...).exists()` (source de vérité — le client n'a pas le chemin worktree, il gate sur le seul `status`). Les Runs **live** (`Running`/`AwaitingUser`/`Paused`) sont exclus au MVP : un edit concurrent dans le worktree pipeline casserait le `git merge` d'un `node_done` en vol.
- **Un seul shell par Run**, create-if-absent (`pdo-shell-<run-id>` fixe ; re-clic = ré-attache). Endpoint `POST /sessions/{run_id}/shell` → `{ ok, session, created }` ; **ne crée pas d'événement, ne mute pas la projection** (donc pas un `run_command` kind — une opération side-band comme `session_attach`). L'attache se fait par le `WS /sessions/<session>/pty` existant + le composant `TmuxTerminal` (inline xterm.js, ADR-0005 ; le spawn OS reste l'escape hatch « détacher »).
- **Persistant**, comme le Manager : survit à la fermeture du terminal, reapé **uniquement** si le Run est absent ou archivé (arm `Shell` de `sweep_orphans`, miroir de l'arm Manager, **sans TTL**), tué par `cleanup_run` à l'archivage. Marker reaper `__shell__`, iter 0. **Le tail est une boucle de respawn `while true; do bash -i; sleep 0.2; done`, pas un `exec bash -i` nu** : un `bash -i` sort sur EOF (Ctrl-D, `exit`, ou un EOF poussé dans le pane à la coupure du WS) et, étant la seule fenêtre, emporte toute la session — le bug de persistance de l'itération 1 (contrairement à `claude`/`sleep` qui ne sortent pas sur EOF). La boucle rend le pane indépendant d'un bash donné (respawn dans le même pane, scrollback conservé) ; corollaire : `exit`/Ctrl-D ne ferme pas la session, il ouvre un shell frais. Garde : `tests/run_shell.rs::shell_survives_eof_and_exit` (ADR-0021 #4).
- **Exempt du cap** (§ *Cap de sessions concurrentes*), par construction : pas un nœud projeté, n'appelle pas la gate d'admission — exactement comme le Manager.
- **Sûreté env** : le shell passe par `wrap_with_env` → exporte `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`. Sans ça, un `claude` tapé dedans 409/SIGKILL les sessions vivantes du compte (cf. commentaire `wrap_with_env`). Le spawn **ignore** `tmux_cmd_override` (bash déterministe, comme un node `script`).
- **Interlock resume** : `resume_run` (Halted/Failed résumables) tue le shell best-effort **avant** de ré-armer le scheduler — un writer concurrent dans le worktree pipeline casserait le merge / la garde doc-only. Refuser (409) déadlockerait (le shell ne meurt que sur archive).

### Cap de sessions concurrentes (admission control)

Borne globale, daemon-wide, sur le nombre de **sessions NodeRun (Claude Code)** vivantes simultanément — la ressource qui s'effondre réellement (cf. tmux-collapse, #77/#78). S'applique à *tous* les Runs, manuels comme déclenchés (les Triggers ne font qu'exposer le besoin).

- **Définition de « session vivante » (#215)** : un nœud `Running`/`AwaitingUser` ne compte que s'il appartient à un **Run lui-même vivant** (`Running`/`AwaitingUser`/`Paused`). Un nœud resté `Running`/`AwaitingUser` dans un Run **terminal** (`Completed`/`Failed`/`Halted`, ou `Archived`) ne tient plus de session par construction (les sessions sont reapées à l'entrée terminale, #205) : le compter serait un artefact de projection qui ampute le cap d'un slot fantôme à vie. Le compteur (`count_live_node_sessions`) filtre donc sur la *liveness du Run*, pas seulement sur `≠ Archived`. (Le `RunStatus` terminal est `Completed`/`Failed`/`Halted` — il n'existe pas de `RunStatus::Stopped` ; `Stopped` est un statut de *nœud*.)
- **Admission par spawn de nœud**, pas par Run : quand le scheduler veut spawner un NodeRun et que `live_sessions + 1 > cap`, le nœud passe en état **`waiting`** jusqu'à libération d'un slot, puis spawn. Le Run est admis immédiatement ; ce sont les *nœuds* qui s'étranglent.
- **Les sessions Pipeline Manager ne comptent pas** dans le cap (légères, 1/Run ; les compter risquerait un soft-deadlock où N managers saturent le budget sans laisser de slot au travail réel).
- **Valeur configurable** via la *Configuration d'instance* (page de réglages instance-wide ; #129, ADR-0015). Précédence `stored → env → default` : la valeur UI surclasse `PDO_SESSION_CAP`, lue frais à chaque décision d'admission (prend effet sans redémarrage).
- **Compteur de sessions** dans la **barre de statut basse** (avec les autres infos techniques), ex. « 7/10 », vire à l'ambre à l'approche du cap pour rendre le throttling lisible avant qu'il morde. C'est une **gauge instantanée** (sessions *vivantes* à l'instant T, au plus une par nœud) — **à ne pas confondre** avec la stat **« Sessions de nœud lancées »** d'un Run (total *cumulatif* des `NodeStarted`, cf. *Statistiques de Run* dans le cycle de vie).
- **Admission atomique (check-and-reserve, #213)** : la décision d'admission (compter les sessions vivantes → décider → réserver le slot en appendant `NodeStarted`/`NodeWaiting`) est sérialisée par un verrou (`admission_lock`). Sans lui, des spawns concurrents (retries des nœuds `waiting` sur plusieurs Runs) observent tous le même slot libre et dépassent le cap.

### Cycle de vie process — résilience (fail-fast, #213)

Posture **fail-fast** partout : jamais d'auto-réparation silencieuse, toute divergence est rendue visible (état `Failed` avec cause lisible). Toutes les transitions vers `Failed` émises ci-dessous passent par la **garde de transitions** (#212).

- **Résilience du balayage (#251)** : les trois boucles background (scheduler de Triggers, stale detector, reaper) partagent une frontière d'isolation commune (`run_isolated`) : un panic pendant un balayage est **contenu** (frontière `tokio::spawn`) — la boucle survit et le balayage suivant rattrape. Sans cela, un seul panic dans le stale detector (un `unwrap` égaré, un cas limite de `project`, un chemin mal formé) tuait silencieusement *toute* la détection (stall/idle/mort de session) pour le reste de la vie du daemon : c'était la cause-racine du stall silencieux où le daemon affichait `running` indéfiniment sans rien détecter. `GET /stale/health` expose `last_tick_at` + l'intervalle (sœur de `GET /triggers/health`, #222), pour qu'un stale detector mort/bloqué soit observable — et pour distinguer, à la prochaine récidive, un balayage mort (heartbeat figé) d'un ratage de sonde par nœud (heartbeat qui avance).
- **Détection de mort de session (liveness sweep)** : le stale detector sonde, à chaque tick, chaque nœud `Running`/`AwaitingUser` ; si sa session tmux n'existe plus, le nœud passe `Failed` avec une cause **nommant la session** (`session_died: tmux session pdo-… no longer exists`). Plus de nœud zombie qui brûle un slot d'admission indéfiniment (#202).
- **Reap sur état terminal (#205)** : à l'entrée d'un état terminal (`completed`/`failed`/`stopped`), un **snapshot du pane** est persisté sous `…/runs/<run>/nodes/<node>/pane-iter-<N>.snapshot` (hors du sous-worktree, donc il survit à sa suppression), **puis** la session est tuée. Invariant : **au plus une itération live par nœud** côté tmux. `GET …/pane` sert le snapshot quand la session est partie et l'indique via `source: "snapshot"` (vs `"live"` / `"resumed"`). Plus de sessions qui s'accumulent vers le point d'effondrement (#77/#78).
- **Recovery au boot** : au démarrage, le daemon réconcilie l'état persisté avec le monde process réel. Un nœud `Running`/`AwaitingUser` sans session vivante → `Failed` avec cause. Une branche de sous-worktree mergée dans la branche pipeline sans `NodeCompleted` correspondant → divergence **détectée et signalée** (jamais complétée en silence). De même, un nœud resté `Running`/`AwaitingUser` dans un Run **déjà terminal** (le boot recovery « live-run » ne le couvrait pas) est réconcilié vers `Failed` avec une cause nommant la situation (`run terminal: node left session-holding`), de sorte que la projection soit cohérente et qu'aucun slot fantôme ne subsiste après redémarrage (#215).
- **Réconciliation au niveau Run (#214)** : la recovery par nœud ci-dessus ne couvre pas le cas **run-level**. Un Run resté `Running` mais **sans aucun nœud vivant** (`Running`/`Waiting`/`AwaitingUser`), **sans merge resolver actif**, et où l'ordonnanceur ne peut produire **aucune action** (aucun nœud `ready`, aucune boucle à amorcer) est un **stall silencieux** — typiquement coincé derrière un nœud terminal non-`Completed` (`Failed`/`Stale`/`Stopped`) dont l'aval ne pourra jamais être schedulé. Il est réconcilié vers `Failed` avec une cause `run_stalled: …` nommant le(s) nœud(s) bloquant(s), **au boot ET à chaque balayage périodique** du stale detector, au lieu de rester `Running` pour toujours. Garde-fous (jamais de faux positif) : tout statut ≠ `Running` est ignoré (`AwaitingUser` attend un humain, `Halted`/`Paused`/terminal n'ont rien à réconcilier) ; une **région de boucle/foreach ouverte** (non-`done`) n'est jamais auto-failée — un état « exhausted — unrouted » est routé par le Pipeline Manager, pas un fail-fast ; le cas « tous les nœuds `Completed` » reste géré par la complétion normale.
  - **Nuance de vocabulaire à ne jamais collapser** : le « nœud vivant » du *stall* inclut `Waiting` (`Running`/`Waiting`/`AwaitingUser`) — un nœud throttlé avancera dès qu'un slot se libère, donc le compter évite un faux stall ; la « session vivante » de l'*admission* (§ Cap) l'exclut (`Running`/`AwaitingUser` seulement — un `Waiting` ne tient pas encore de session tmux). Ce sont **deux prédicats distincts, jamais un seul** : « tient une session » (admission) ≠ « peut encore progresser » (stall). Les unifier re-créerait un faux positif (un Run `Waiting`-derrière-blocage serait faussement `run_stalled`→`Failed`).
- **Blocage sur menu de limite d'usage (#290)** : un troisième état « vivant mais sans progrès », distinct de la mort de session et du stall run-level. Quand la session Claude Code d'un nœud atteint la limite 5 h **en cours de tour**, CC affiche un **menu interactif bloquant** dans le pane (« Stop and wait for limit to reset / Switch to usage credits ») et attend une frappe qui ne vient jamais. La session tmux reste vivante (donc ni `session_died` ni, faute de progrès, une transition terminale) : ni la sonde de liveness ni le proxy de fraîcheur (mtime du `.jsonl`) ne le voient. Le stale detector **lit donc le contenu du pane** (`tmux capture-pane`) pour les nœuds jugés `Ok`, et sur reconnaissance du menu émet un événement **informationnel** `NodeBlockedOnLimit` (no-op de projection, comme `PipelineLint` — le nœud **reste `Running`**) et incrémente un compteur `blocked_on_limit` exposé par `GET /stale/health`. L'ancre textuelle du menu n'est pas documentée officiellement et **dérive** selon la version de CC : la détection est **best-effort / observabilité seule** (un ratage = statu quo, un faux positif = un événement inoffensif), volontairement scopée ainsi (#290 Slice 1). La **récupération** (auto-dismiss + attente + re-nudge, ou état `blocked_on_limit` first-class libérant le slot) et l'**échappatoire de concurrence** pour les pipelines à la minute restent des décisions humaines (action durable initiée par le runtime → ADR-0012) — Slices 2 et 3. NB : le proxy mtime est de toute façon inerte sur les vrais runs (`encode_working_dir` ne reproduit pas l'encodage de chemin de CC — bug suivi séparément), ce qui explique que le menu (comme l'idle #251) échappe au chemin mtime ; le détecteur de menu lit le pane directement et n'en dépend pas.

### Pont UI ↔ tmux : terminal inline xterm.js

ADR-0005. L'option A historique (preview read-only + spawn d'une fenêtre OS native) est **obsolète**. Mécanisme actuel :

- **Statut** (pending / running / awaiting_user / done / failed / blocked) — projeté depuis l'event log.
- **Terminal interactif inline** dans le panneau de détail du nœud, rendu via xterm.js. Le daemon expose `WS /sessions/<id>/pty` : pour chaque connexion, il spawn `tmux attach -t <session>` dans un PTY (crate `portable-pty`) et bridge les bytes I/O entre le browser et le PTY. Bidirectionnel : l'utilisateur tape dedans, voit la sortie en temps réel. Plus de polling 1-2 s — la WebSocket pousse.
- **Icônes du panneau** : (1) **agrandir** — le terminal occupe tout l'espace vertical du panneau de détail ; (2) **détacher** — fallback opt-in qui spawn une fenêtre OS native (`gnome-terminal`/`konsole`/`Terminal.app`/`kitty`) attachée à la session via `tmux attach`. Garde un escape hatch pour les cas limite (copy-paste exotique, freeze WebSocket).
- **« agrandir » est toujours un geste utilisateur explicite (#270)** : ni la sélection d'un nœud, ni l'auto-snap sur le nœud vivant à l'entrée d'un Run live n'agrandit le terminal de lui-même. On garde l'auto-sélection du nœud vivant ; seule l'expansion forcée est retirée. Un réglage rendant l'auto-agrandissement opt-in est différé : la *Configuration d'instance* existe (#129, ADR-0015) mais ce toggle terminal-spécifique reste hors du scope MVP de #129.

Détection du terminal natif (pour l'icône détacher) : variable `PDO_TERMINAL` ou heuristique sur `$TERM_PROGRAM` / OS / `which`.

Multi-client par session (deux onglets browser sur la même session tmux) : gratuit côté tmux, pas à coder. Sécurité : origin check sur la WebSocket pour éviter le DNS-rebinding (le daemon écoute sur `127.0.0.1` mais ce n'est pas suffisant en soi).

### Nœuds interactifs — signal de complétion

Un Node marqué `interactive: true` spawn une session tmux normale, et **n'auto-complète jamais**. La session reste attachable indéfiniment ; l'utilisateur peut détach/réattacher autant de fois que nécessaire et continuer à interagir.

La complétion est signalée **depuis l'UI**, par un bouton "Mark complete" sur le nœud. Click → `POST /runs/<id>/commands { kind: "mark_node_done", node_id, iter }`. Pas de slash-command in-session (un slash-command suppose qu'on est attaché ; le bouton UI reste toujours accessible).

À ce moment-là, les artefacts présents sur disque dans `<artifacts>/<node-id>/iter-<N>/` sont considérés comme finaux. Le préambule du nœud le dit explicitement à l'agent et au user : *"écris tes outputs aux chemins X, Y, Z ; quand tu cliques 'Mark complete' dans l'UI, ces fichiers seront pris tels quels"*.

---

## UX — un seul mode d'édition unifié

PDO est un **atelier de production de code** ; la conception de pipelines est un *moyen*, pas le centre de gravité. ADR-0007. L'ancienne dichotomie "mode Run" vs "mode Edit (toggle crayon)" est **obsolète** — un seul mode, le canvas est toujours interactif, et son comportement s'adapte à l'état de la pipeline (running ou pas).

> **Source visuelle de référence** : voir [`docs/design/`](./docs/design/) pour les écrans rendus en HTML/CSS/JS. Note : les écrans pré-2026-05 reflètent l'ancienne dichotomie Run/Edit avec toggle ; à re-designer en phase suivante.

### Layout 3 panneaux

- **Gauche — Liste, à trois onglets** `Runs | Triggers | Library` (l'ancien empilement de sections collapsibles devient une barre d'onglets, cf. mockup `lp-tabs`). Triggers est au milieu : il *produit* des Runs (gauche) et *consomme* des pipelines de la Library (droite).
  - **Runs** : les Runs **actifs** en haut (regroupés par repo cible si ≥ 2 repos distincts, sinon liste plate — cf. *Repo cible*), puis une section **« Archived »** repliable et **plate** regroupant les Runs `archived` (repliée par défaut ; s'ouvre d'office quand le Run sélectionné vient d'être archivé, pour ne pas le faire disparaître sous les yeux). La section Archived (dans la liste de gauche) est un **regroupement de vue** — pas ce dossier de la liste qui serait un répertoire disque, et jamais un delete (cf. *Cleanup vs archive*). À ne pas confondre avec le *Blackboard archivé* (`~/.pdo/runs/<id>/`), qui est bien un dossier disque durable mais côté store, pas côté UI. Un Run créé par un Trigger porte un badge de provenance (icône + nom du Trigger, cliquable vers celui-ci).
  - **Triggers** : liste des Triggers (cf. *Trigger*), toggle enable/disable, « + New Trigger ».
  - **Library** : pipelines templates avec badge favorite.
  - Click → bascule l'affichage middle/droite. Le contexte d'édition (run-snapshot ou template) est inféré du clic, pas d'un toggle global.
- **Centre — Canvas du graphe.** Render du DAG, toujours interactif (drag-drop nodes, créer edges, sélection multiple). Quand le contexte est un Run en cours :
  - **Highlight** sur le(s) nœud(s) en cours d'exécution (pluriel — fan-out parallèle peut en avoir plusieurs simultanés).
  - **Encart overlay** flottant : run-id, status global, boutons d'action niveau Run (cancel, cleanup, attacher manager).
  - Mutations contraintes par la politique d'édition pendant un Run (cf. *Édition pendant un Run*).
- **Droite — Détail du nœud sélectionné** (NodeRun ou node-template).
  - **Terminal interactif inline** (xterm.js, ADR-0005) si NodeRun sur Run actif. Icônes "agrandir" et "détacher OS".
  - **Inputs résolus** : noms des ports + chemins absolus des artefacts amont + bouton "open" pour les lire dans un viewer markdown.
  - **Outputs produits** : pareil pour les fichiers du nœud lui-même + le schéma de frontmatter déclaré.
  - **Prompt initial** : visualisation du préambule runtime + prompt-utilisateur tels que reçus par le Claude Code de cette session.
  - **Bouton "Mark complete"** si le nœud est interactif et en attente.
  - **Formulaire d'édition du node** : nom, type (`code-mutating`/`doc-only`), `interactive`, prompt (textarea reliée au `prompt_file`), inputs, outputs (avec frontmatter schema). En mutation, contraintes par la politique d'édition pendant un Run.

### Toolbar — bouton info pipeline

La toolbar du canvas (où vivent les types de nodes ajoutables) contient une icône `i` qui ouvre un panneau **info pipeline** :
- Nom de la pipeline, statut (running, idle), variables.
- Bouton **favoriter** (= ajouter / retirer de la bibliothèque).
- **Pipeline Manager** : si la pipeline tourne, le terminal manager (`pdo-mgr-<run-id>`) prend la place dominante du panneau ; les métadonnées restent en haut compactes. Hors run, pas de terminal manager — juste les métadonnées.

Realtime via WebSocket depuis le daemon → chaque événement de l'event log push une update vers l'UI.

### Workflow utilisateur typique

1. **Monitor** : ouvre PDO, voit ses Runs actifs, debug un Run bloqué via le manager (onglet info) ou en attachant directement (terminal inline).
2. **Lancer un nouveau Run** : bouton "+ New Run", modale avec sélecteur de **pipeline depuis la bibliothèque** (dropdown peuplé par les pipelines favorites) + textarea input (free-text ou lien d'issue ou mix) + accordion "variables overrides". Confirme → POST `/runs` qui clone la pipeline depuis la bibliothèque vers `<repo>/.pdo/runs/<run-id>/pipeline.yaml` et lance le Run.
3. **Créer une nouvelle pipeline** : depuis la liste de gauche, bouton "+ New Pipeline" → ouvre un canvas vierge dans le scope template-bibliothèque.
4. **Modifier une pipeline** : click dessus dans la liste, le canvas l'affiche, on édite. Pas de toggle.
5. **Modifier pendant un Run** : click sur le Run en cours, le canvas affiche le run-snapshot, on édite à chaud. La politique d'édition pendant un Run s'applique.

### Status icon par Run

Chaque entrée de la liste de gauche porte un icône coloré indiquant son statut, lisible en un coup d'œil :

| Status | Couleur / icône |
|---|---|
| `running` | bleu pulsant |
| `awaiting_user` (nœud interactif en attente) | jaune |
| `done` | vert plein |
| `blocked` (run_halted ou conflit non résolu) | orange |
| `failed` | rouge |
| `archived` | gris |

### Cleanup vs archive

Pas de "permanent delete" v1 (mais cf. `forget` ci-dessous). Le bouton "Cleanup" sur un Run terminé :

- supprime la branche `pdo/run-<run-id>`,
- supprime le worktree pipeline et tous les sous-worktrees,
- **copie** les sorties du Run vers le *Blackboard archivé* global (`~/.pdo/runs/<run-id>/` : `artifacts/` + `pipeline.yaml` + `pipeline.prompts/`, lecture seule) **avant** de supprimer la copie repo-local. Les outputs ne partent donc plus au cleanup — c'est ce qui rend un Run `archived` consultable (canvas + outputs) ; cf. **ADR-0020**.

**Mais ne touche pas à l'event log** : les événements en SQLite restent. Le Run passe en status `archived`, reste dans la liste de gauche avec un icône gris, et reste **interrogeable post-mortem** — via les events *et* via le Blackboard archivé (rouvrir le canvas en lecture seule, relire les outputs des nodes). Pas d'auto-cleanup, jamais.

L'event log **et** le Blackboard archivé peuvent grossir indéfiniment ; on évalue la taille avant de décider d'une politique de purge. Pas de v1. Le seul reclaim v1 du Blackboard archivé est le **`forget`** (`DELETE /runs/<id>`, autorisé sur un Run déjà `archived`) : il purge les events *et* `~/.pdo/runs/<id>`.

### Forget durable

Le `forget` (`DELETE /runs/<id>`) est **durable** (ADR-0024) : il pose un *tombstone* dans la table `forgotten_runs` et purge les events dans une même transaction. Conséquences :

- `append_event` refuse **tout** kind d'événement pour un run_id tombstoné (garde `INSERT … WHERE NOT EXISTS`, sans fenêtre TOCTOU) — un écrivain tardif (session `pdo-mgr` orpheline, tail détaché post-#304/ADR-0023) ne peut plus ressusciter le run ; il logge l'erreur et continue.
- `POST /runs/<id>/commands` et `POST …/nodes/<n>/done` répondent **410 Gone** pour un run oublié, avant tout side-effect.
- `forget` tue en best-effort les sessions `pdo-mgr-<id>` / `pdo-shell-<id>` (un run oublié n'a plus rien à récupérer).
- Un run_id oublié n'est **jamais réutilisable** (le tombstone bloque aussi `RunStarted`) ; les run_id sont horodatés, la collision est impraticable.
- `project()` ne projette jamais un log sans `RunStarted` : un fragment événementiel orphelin rend `None` au lieu d'un fantôme `running` sans nom.

### Notifications

Pas de notifications système v1. Le status icon dans la liste de gauche suffit. Si plus tard ça manque, on rajoute optionnellement (desktop notification API, opt-in). Pas avant.

---

## Stack technique

Cf. ADR-0003.

### Daemon (Rust)

- **Runtime async** : Tokio.
- **HTTP + WebSocket** : Axum (intégré avec Tokio).
- **DB** : SQLite via `sqlx` (async, type-safe, query-checked à la compile).
- **File-watching** : crate `notify` (cross-platform `inotify`/`fsevents`/`ReadDirectoryChangesW`).
- **YAML** : `serde_yaml` (parsing des pipelines + des frontmatters).
- **Process spawning** : `std::process` + `tokio::process` pour l'async ; plus une fine couche pour piloter `tmux new-session` / `tmux capture-pane` / `tmux kill-session` et `git worktree add` / `git merge`.
- **Frontend embedding** : `rust-embed` ou `include_dir` pour bundler le build statique du frontend dans le binaire du daemon.

### Frontend (React + Vite)

- **Framework UI** : React + Vite.
- **Canvas DAG** : **xyflow** (anciennement React Flow). Lib mature, custom nodes/edges/handles, support pan/zoom/mini-map/fit-view natif.
- **Composants UI** : **shadcn/ui** (Radix + Tailwind) pour la chrome (panneaux, dialogs, formulaires, dropdowns).
- **State management** : Zustand pour l'état UI ; **TanStack Query** pour les fetch HTTP avec cache.
- **WebSocket client** : natif + petit reconnect-wrapper.

### Distribution

- v1 : binaires pré-buildés sur GitHub Releases (Linux x86_64, Linux ARM64, macOS x86_64, macOS ARM64) + script `curl -fsSL <url>/install.sh | bash`. Pas de npm (le daemon est en Rust, pas en JS).
- Plus tard : formula Homebrew (macOS), package AUR (Arch).
- Plus tard (v2) : wrapper Tauri pour distribution desktop native, qui réutilise le même daemon Rust + le même frontend.

### Service unit persistant (#156)

**Service unit** : le fichier d'unité OS qui fait démarrer le daemon au boot et survivre au logout — une `systemd --user` unit sous Linux (`~/.config/systemd/user/pdo.service`), un LaunchAgent launchd sous macOS (`~/Library/LaunchAgents/com.pdo.daemon.plist`, best-effort). C'est la différence entre « les Triggers ne tournent que tant que tu es loggé » et un orchestrateur autonome fiable (résout la limitation v1 d'ADR-0012). Voir ADR-0019.

- **CLI** : `pdo service {install [--port N] [--dry-run] | uninstall | status}` — un sous-commande top-level (comme `daemon`/`complete`/`fail`/`skip`), one-shot bloquant sans runtime tokio. `install` génère l'unité, la `daemon-reload`, `enable-linger` (pour survivre au logout, sans sudo depuis une session active — dégrade avec un hint `sudo …` sinon), puis `enable --now`. `--dry-run` imprime l'unité + le plan de commandes **sans aucun effet de bord** (seam de test/preview).
- **Fidélité au recette prod** : l'unité systemd est un portage byte-fidèle de la recette `Makefile` (`service-install`), paramétrée. Deux lignes **load-bearing** : `KillMode=process` (le défaut `control-group` SIGKILL-erait le serveur tmux enfant qui tient toutes les sessions Claude live — cf. #234) et `Environment=PATH=…<dir de node>…` (le daemon shelle vers `claude`/`node`/`git`/`tmux` ; un PATH nu casse silencieusement les spawns). `WorkingDirectory` est load-bearing aussi (le daemon dérive `repo_root` du cwd). L'analogue macOS de `KillMode=process` est `AbandonProcessGroup=true`.
- **Garde de conflit de port** : deux daemons ne peuvent jamais partager un port (bind sans `SO_REUSEADDR`, `EADDRINUSE` fatal). `install` sonde `127.0.0.1:<port>` : port libre → `enable --now` ; un daemon PDO répond déjà → idempotent (enable pour le boot **sans** `--now`, pas de compétiteur) ; process étranger → **refus loud** (l'unité crash-looperait sous `Restart=on-failure`). Remplace l'item « lazy-start » de l'issue, qui reposait sur un mécanisme d'auto-spawn inexistant dans le code.
- **Signal UI** : le champ `service` de `GET /sessions` (`{ supervisor, persistent }`) est calculé **une fois au boot** et caché dans `AppState` (zéro coût par poll). `supervisor` = détection best-effort par marqueurs d'env (`systemd`/`launchd`/`none`) ; `persistent` = `systemctl --user is-enabled pdo.service` (timeout ~1s, dégrade en `null`, jamais une erreur). La status-bar reste silencieuse quand `persistent` vaut `true`/`null` et affiche une pastille ambre `ephemeral` (même token `text-st-await` que le dot reconnecting / le compteur near-cap) quand il vaut `false` — le seul signal que le dot de connexion ne peut pas exprimer (joignable ≠ persistant). Le seam d'observation `PDO_SERVICE_HEALTH` (`persistent`|`ephemeral`|`unknown`, `None` en prod) force l'état affiché pour exercer la branche `ephemeral` sur une box où une unité est déjà enabled.
- **Limites acceptées (v1)** : le chemin launchd réel n'est **pas testé sur la CI Linux** (génération golden-testée seulement) ; pas d'équivalent linger pour un LaunchAgent (headless macOS vrai = LaunchDaemon root, différé, human-ratified) ; la valeur `persistent` cachée peut être **stale** si on installe le service pendant qu'un daemon non-service tourne (le flux normal est install-puis-run) ; le bind `0.0.0.0` reste inchangé (→ #260).

### Versioning (#139)

- **Source de vérité unique : le `version` du `Cargo.toml` workspace.** `frontend/package.json` reste à `0.0.0` en permanence — intentionnel, ne jamais le bumper (le release flow ne touche que Cargo.toml).
- Le daemon expose sa version compilée (`CARGO_PKG_VERSION`) dans la réponse de **`GET /sessions`** (`{ live, cap, version, service }`), l'endpoint qui alimente déjà la status-bar. Pas de route `GET /version` dédiée : un champ JSON additionnel est rétro-compatible et évite une entrée de plus dans la whitelist du proxy vite dev. Le même argument vaut pour le champ **`service`** (#156, `{ supervisor, persistent }`, cf. *Service unit persistant*) : plutôt qu'une route `GET /service/status`, un champ additionnel — d'autant qu'il est **calculé une seule fois au boot et caché** (état quasi-statique, ne change qu'à l'install/uninstall), donc zéro coût subprocess par poll.
- Le footer affiche `v<version>` à partir de ce payload, rafraîchi au mount et à chaque event WebSocket. Tant que le daemon n'a pas répondu, **rien n'est rendu** (pas de placeholder) ; le dot de connexion signale déjà l'injoignabilité.
- En prod le binaire embarque le frontend, donc daemon et UI ne peuvent pas diverger. En dev le footer montre la version du daemon debug réellement joignable.

### Mono-user, local

Le daemon écoute sur `127.0.0.1:<port>` uniquement. Pas d'auth, pas de TLS, pas de multi-user. Single-user local par design. Tout ce qu'il faut pour ça : SQLite locale, FS local, tmux local, git local. Pas de dépendance réseau.

### Persistance et hot-reload

- **Save explicite** (#35) : un bouton **Save** dans la barre d'onglets, le raccourci **Cmd/Ctrl+S**, et un **flush automatique au lancement d'un Run** (toutes les modifs non sauvegardées sont écrites avant de démarrer le Run). Pas d'auto-save debounced. Le canvas EST le fichier YAML + les fichiers prompts.
- **Hot-reload bidirectionnel** : PDO watch les fichiers (`fswatch`/`inotify`). Édition externe (Vim, VS Code) → re-parse et re-render. Last-write-wins.
- **Historique d'édition (undo/redo)** (#226) : pile **par onglet** des états d'édition successifs du canvas (Ctrl/Cmd+Z annuler, Ctrl/Cmd+Shift+Z ou Ctrl+Y rétablir, plus deux boutons toolbar), scopée à l'**édition** (positions, nœuds, edges, loops, métadonnées) — **exclut l'état de Run** (statuts/overlay) et les prompts. In-memory (vidée au reload, pas de persistance cross-session), plafonnée. Vidée sur reload-propre / "Take theirs" / "Reload changes" ; conservée à travers un Save. À distinguer de l'**historique** d'un Run (events SQLite) et des fires d'un Trigger. Cf. ADR-0014.
- **Pas de git intégration v1.** Le user fait ses commits manuellement s'il versionne.

### Création d'un nouveau nœud

- **From scratch** : "+ Add node" → nœud vide à remplir.
- **Duplicate existing** : right-click sur un nœud → copie avec id auto-incrémenté.
- **Depuis la bibliothèque** : drag-drop d'un node favori (cf. *Bibliothèque* ci-dessous).
- **Pas de library de templates PDO-shipped en v1** (cohérent avec ADR-0001 : pas d'opinion vendor sur "à quoi ressemble un Implementer"). La bibliothèque est exclusivement user-managed.

---

## Bibliothèque

`~/.pdo/library/` — store user-managed à deux niveaux :

- **Nodes** (`~/.pdo/library/nodes/`) — nodes réutilisables d'une pipeline à l'autre. Drag-drop depuis le panneau bibliothèque vers le canvas pour les instancier. Endpoint daemon `POST /library/nodes` accepte une node spec inline ; la création n'est jamais bloquée par un état "pipeline dirty".
- **Pipelines** (`~/.pdo/library/pipelines/`) — pipelines complètes templatées. C'est cette liste qui peuple le **dropdown du modal "+ New Run"**. Bouton favoriter dans le panneau info de la toolbar pour ajouter / retirer une pipeline de la bibliothèque.

Le clone d'une pipeline depuis la bibliothèque vers `<repo>/.pdo/runs/<run-id>/pipeline.yaml` se produit au démarrage d'un Run. Les modifs pendant un Run propagent vers la template d'origine (auto-sync montant, ADR-0007).

- **Duplicate (library pipeline)** (`POST /library/pipelines/{id}/duplicate`, #224) — clone **délié** d'une template de la bibliothèque : id frais, nom suffixé `(copy)`/`(copy N)` (calculé unique sur les deux scopes), **aucune** métadonnée de promotion (`meta.json` / `promoted_from`). Le YAML est réécrit **verbatim sauf la ligne `name:` de colonne 0** (jamais re-sérialisé), pour préserver clés top-level inconnues (`auto_merge_resolver`), commentaires et ordre des champs. À distinguer de **Promote** (qui enregistre `promoted_from`) et du **duplicate** de nœud sur le canvas (`{name} copy`, sans parenthèses). _Éviter_ : copy, clone quand le contexte le rend ambigu avec promote.
- **Supprimer une pipeline ≠ supprimer sa copie en bibliothèque** (#227) — par défaut la copie favorite (durable) subsiste après la suppression de la pipeline de travail et reste visible comme entrée *library-only* (*Sharp tool* : on surface, on ne masque pas — pas d'auto-cleanup, pas de re-surfacement silencieux). Une case opt-in (décochée par défaut, libellé `Also remove the Library copy`) permet de retirer aussi la copie dans le même geste, **uniquement** si une seule copie de même nom existe (match sur le `name`, jamais sur l'id qui diverge) ; sur un double-favori ambigu la case est supprimée et les deux copies sont conservées. Toute suppression rafraîchit désormais les **deux** listes (merged `/pipelines` + `/library/pipelines`).

---

## Import de workflow (Claude Code → pipeline)

**Import de workflow** :
Décompilation **avec perte** d'un workflow Claude Code (`.claude/workflows/*.js`, format dynamique
officiel CC) en un **brouillon de Pipeline** déposé en Bibliothèque (scope user, jamais lancé). But =
**onboarding** depuis un artéfact officiellement supporté, **pas fidélité** — « importe le câblage,
signale le reste ». Parsing par AST statique (`oxc`), **jamais d'exécution du `.js`** (ADR-0016).
_Éviter_ : « conversion », « migration » — la **migration** (`pipeline_migrator`) réécrit du YAML PDO
d'un ancien schéma vers le courant (même format) ; l'**import** traduit un format étranger.

**Placeholder annoté** :
Nœud `doc-only` (aucun type de nœud dédié) dont le corps explique un idiome de workflow que l'import
v1 ne matérialise pas (boucle imbriquée, garde budgétaire, `try/finally`, accumulation cross-lap, prompt
bâti par helper nu). L'annotation **est** le tutoriel d'onboarding : elle nomme ce qu'un utilisateur PDO
n'écrirait jamais à la main (gestion worktrees, auto-cleanup, boucle budgétaire — remplacés par des
features plateforme) et le traduit en interaction délibérée. Distinct du *nom placeholder* d'un Run (#184).

**Extraction verbatim** :
Règle de récupération des prompts. Un string-literal ou un template-literal **sans** interpolation →
corps de prompt **verbatim**. Un template-literal inline **avec** `${…}` → texte statique extrait verbatim,
chaque trou rendu en marqueur annoté câblable en port d'entrée. Un prompt **sans aucun texte statique**
(appel de helper nu `agent(buildPrompt())`, identifiant) → placeholder annoté. Cohérent avec la *prompt
augmentation* (le corps est ajouté verbatim, jamais substitué ; les inputs arrivent en bloc de chemins).

### Relations

- Un **Import de workflow** produit un **brouillon de Pipeline** en **Bibliothèque** (scope user).
- Un workflow CC contient des **idiomes** mappés : `agent()` → **Node**, `pipeline()` → **boucle
  `collection`**, `for`/`while` (dont le corps contient un `agent()`) → **boucle `bounded`**, `if`/`return`
  gardé → **edge conditionnelle** (`when:`), schémas JSON → **frontmatter de port de sortie**.
- Un idiome hors sous-ensemble reconnu → **placeholder annoté**. Un `git merge` scripté (fixed-worktree,
  ordre imposé, build par merge) → **Node `code-mutating` annoté**, **pas** le **Merge** first-class
  (ADR-0006) dont il excède le contrat.
