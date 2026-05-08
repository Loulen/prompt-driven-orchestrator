# Maestro — Glossaire métier

Glossaire vivant. Mis à jour au fil des décisions, lazy.

---

## Pipeline

Un **Pipeline** est un DAG nommé, à **topologie figée**, qui décrit l'enchaînement de rôles d'agents pour accomplir une tâche d'ingénierie.

- **Topologie figée** : aucun *conditional edge*. Le graphe est tracé à l'édition, et l'exécution suit ce graphe sans qu'aucun LLM ne décide d'embranchement à l'exécution.
- **Pas de routage probabiliste** : le déterminisme porte sur la *structure* (qui appelle qui dans quel ordre), pas sur le contenu produit par chaque nœud (les LLM aux feuilles restent stochastiques).
- **Multiples pipelines plutôt qu'embranchements** : pour gérer des trade-offs coût/complexité (ex. *quick-fix* vs *feature-with-adversarial-review*), on définit plusieurs pipelines distincts. Pas un seul pipeline avec des branches.

Contrairement à : Liza (pipelines YAML), Langgraph (conditional edges + LLM-router), TPM workflow (orchestrateur LLM qui décide quand spawner).

---

## Node

Unité atomique d'un Pipeline. Un **Node** représente un rôle d'agent — typiquement une instance de Claude Code à laquelle on confie un prompt système qui définit sa mission (Implementer, Planner, Reviewer, etc.).

Un Node se définit par :

- **Nom** — identifiant lisible affiché dans le canvas.
- **Prompt système** — le rôle, écrit dans la zone de texte qui s'ouvre à l'édition.
- **Ports d'entrée** — un ou plusieurs, chacun reçoit un document amont. Multi-fan-in supporté (ex. Implementer reçoit `plan` + `task` + `reviews_bloquantes`).
- **Ports de sortie** — un ou plusieurs documents produits. Multi-fan-out supporté (ex. Planner sort `plan.md` + `task_list.md` consommés par des Nodes différents).

Distinct de :

- **NodeRun** *(à valider)* — l'exécution d'un Node au sein d'un Pipeline Run précis. Un NodeRun = une session tmux Claude Code dans un sous-worktree dédié, avec un statut (pending/running/done/failed).

## Dataflow

Modèle (A) — **document-first, code en side-channel** :

- Les arêtes du DAG transportent **uniquement des documents** (artefacts markdown).
- Le **code** vit dans la branche du Pipeline Run. Quand un NodeRun finit, son sous-worktree est mergé dans la branche du Pipeline Run. Le NodeRun suivant fork un nouveau sous-worktree depuis cet état.
- Les wires de l'éditeur = dataflow documentaire intentionnel. L'état du code suit en arrière-plan.

À traiter plus tard : conflit potentiel quand deux NodeRuns parallèles modifient le code → stratégie de waves / disjoint-files (cf. Liza/TPM).

---

## Switch — branchement conditionnel mécanique

Le **`Switch`** est un nœud first-class dont la fonction est de router un artefact d'entrée vers une de N branches selon des prédicats mécaniques (jamais LLM, ADR-0002). Il remplace l'ancien modèle "clause `when:` portée par l'edge" : **les conditions vivent désormais sur les ports de sortie d'un Switch, pas sur les edges**.

Justifié par les pratiques des outils matures (n8n `Switch`, Unreal `Branch`/`Switch on Int`, ComfyUI `ifElse`) : un nœud routeur dédié rend les points de décision visibles au coup d'œil et permet une UI de composition AND/OR par branche.

### Forme

- 1 input port (`in`) qui reçoit l'artefact à inspecter.
- N output ports nommés, chacun porteur d'une clause `when:`.
- 1 output port `default` implicite, sans clause, qui fire si aucune autre branche n'a matché.

### Évaluation

`first-match-wins`, dans l'ordre déclaré (ordre dans l'Inspector, persisté en YAML). Les conditions opèrent sur :

- Tout champ de frontmatter de l'artefact entrant.
- Toute variable pipeline référencée par `$<name>`.

`iter` n'est **plus** un champ de `when:` — le compteur d'itération est désormais une propriété du nœud `Loop` (cf. ci-dessous), pas une variable globale d'un nœud source.

### Composabilité

Plusieurs prédicats dans une même branche sont **AND'd implicitement**. Pour OR :
- `in: [...]` pour OR-sur-un-même-champ (cas dominant).
- Plusieurs branches Switch qui wirent vers la même target downstream (cas cross-fields).

Prédicats : `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `in`, `not_in`. Pas d'eval libre, pas de string-expression — le runtime parse YAML, résout les variables, applique les prédicats.

### YAML

```yaml
- id: gate
  type: switch
  inputs:
    - { name: in }
  outputs:
    - name: pass
      when:
        verdict: { eq: PASS }
        complexity_score: { lt: 3 }
    - name: rework
      when:
        verdict: { in: [FAIL, NEEDS_WORK] }
    - { name: default }
```

---

## Loop — itération bornée first-class

Le **`Loop`** est un nœud first-class qui matérialise les boucles précédemment "émergentes". Il remplace le pattern back-edge + `iter < N`. L'ancienne formulation "Cycle = propriété émergente" est **obsolète** : les cycles d'une pipeline Maestro sont désormais des Loop nodes explicites, dans l'esprit de Unreal `ForLoopWithBreak` ou n8n `Loop Over Items`.

### Forme

- 2 input ports : `in` (entrée), `break` (force la sortie immédiate).
- 2 output ports : `body` (fire une fois par itération), `done` (fire à la sortie).
- Config : `max_iter: int` (référencable via `$<var>` pipeline).

### Sémantique runtime

- Réception sur `in` : initialise `iter = 1`, fire `body`.
- Le **body subgraph** = ensemble des nœuds joignables depuis `body` qui ne pointent pas vers `break`/`done`. Calculé par le runtime au lancement, pas déclaré dans le YAML.
- Quand tous les NodeRuns du body subgraph pour iter N sont terminés : si `iter < max_iter`, incrémente `iter` et re-fire `body` avec mêmes inputs ; sinon, fire `done` (event `loop_max_reached`).
- Trigger sur `break` : court-circuite, fire `done` immédiatement (event `loop_break`).

Le compteur `iter` est scopé à un Loop instance pour ce Run. Plusieurs Loops dans une même pipeline ont chacun leur propre compteur, indépendant.

### Itération intra-Run uniquement

Les compteurs `iter` sont remis à zéro pour chaque nouveau Run. Pas de "mémoire d'itérations" entre Runs distincts du même pipeline.

### Accumulation côté input (inchangé)

Un nœud du body avec un input port `repeated: true` lit le glob `iter-*/<port>.md` du nœud source amont — le mécanisme reste valide, scopé au Loop courant via le sous-dossier d'artefacts.

### YAML

```yaml
- id: review-loop
  type: loop
  inputs:
    - { name: in }
    - { name: break }
  outputs:
    - { name: body }
    - { name: done }
  max_iter: 5
```

---

## Edges — purement structurelles

Une edge transporte un artefact d'un output port à un input port. Forme :

```yaml
- source: { node: <id>, port: <port> }
  target: { node: <id>, port: <port> }
```

**Plus aucune clause `when:` sur les edges.** Toute la logique conditionnelle est portée par les nœuds `Switch` et `Loop`. Les edges sont un graphe muet — leur rôle est purement structurel : déclarer le câblage des ports.

Le pattern halt-edge décrit dans des versions antérieures est lui aussi déprécié — depuis #39, la terminaison du Run passe par un edge vers le nœud `End` mandatoire.

---

## Blackboard

Le **Blackboard** est le store partagé où vivent tous les artefacts d'un Pipeline Run. Toutes les sorties documentaires de tous les NodeRuns y sont persistées et adressées par chemin.

- **Localisation** : `<pipeline-worktree>/.maestro/artifacts/`. Suit la branche du Pipeline Run, part au cleanup.
- **Format** : markdown brut (`.md`) avec **YAML frontmatter** pour les métadonnées structurées (verdict, statut, références, etc.). Le corps reste lisible humainement, le frontmatter est parsable par le runtime.
- **Wires** : dans l'éditeur, un wire de `Node A → Node B` n'est pas un transport ; c'est une **déclaration de dépendance**. Le runtime traduit en : *"avant de lancer B, attendre que A ait posé son artefact ; l'input port de B le lit depuis le Blackboard"*.
- **Cycles + accumulation** : chaque tour de cycle écrit dans un sous-dossier `iter-<N>/`. Les ports d'entrée qui veulent accumuler (ex. `reviews_bloquantes`) lisent un glob `iter-*/review.md` → liste naturellement ordonnée.

### Schéma d'adressage

Chaque artefact produit par un NodeRun a un chemin canonique :

```
<pipeline-worktree>/.maestro/artifacts/<node-id>/iter-<N>/<port-name>.md
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
2. **Préambule runtime** — généré déterministiquement à partir des ports configurés. Ne dépend pas du LLM, écrit par Maestro à chaque NodeRun.

Le préambule contient au minimum :

- **Inputs disponibles** :
  - Pour chaque port d'entrée : nom du port + chemin absolu sur disque + (optionnel) inline du contenu si court.
  - Ex. *"Tu as accès à : `plan` (lis `<artifacts>/planner-1/iter-1/plan.md`), `task` (lis `<artifacts>/planner-1/iter-1/task.md`), `reviews_bloquantes` (lis tous les fichiers `<artifacts>/reviewer-1/iter-*/review.md`)."*
- **Outputs attendus** :
  - Pour chaque port de sortie : chemin où écrire + schéma de frontmatter requis.
  - Ex. *"Tu dois produire à `<artifacts>/reviewer-1/iter-2/review.md` un fichier markdown avec frontmatter YAML contenant le champ `verdict: PASS | FAIL`. Le contenu détaillé (blocking issues, justifications) va dans le corps."*
- **Capacités Maestro-specific (CLI)** :
  - `maestro complete` — à appeler via Bash quand le NodeRun est terminé (cf. signal de complétion, Q10).
  - `maestro fail --reason "..."` — à appeler en cas d'incapacité à finir.
  - Ces commandes ne sont **pas** packagées comme skills Claude Code — elles sont 100% systématiques, sans bénéfice de progressive disclosure.
- **Itération courante** : *"Tu es à l'itération {iter} de ce nœud."* Permet à l'agent d'adapter son comportement au tour de boucle (par exemple : Implementer en iter 1 implémente from scratch ; en iter 2+ il itère sur les reviews).
- **Variables pipeline résolues** : injecte les valeurs des variables référencées dans le préambule (utile si l'agent doit savoir le `max_iter_review` pour adapter son verbosité, par exemple).

Conséquence : le designer du pipeline n'a pas à se soucier dans son prompt utilisateur de *"où écrire / quoi mettre en frontmatter / comment signaler la fin"* — c'est imposé par le runtime. Il se concentre sur le *rôle*.

### Skills Claude Code — délégué

Maestro **ne gère pas** les skills. Les skills disponibles dans une session NodeRun sont ceux que Claude Code charge naturellement : `~/.claude/skills/`, `<target-repo>/.claude/skills/`, `<sub-worktree>/.claude/skills/`. Pas d'attachement par-Node, pas de symlink, pas de mécanisme custom. Si le user veut une capacité spécifique, il l'exprime soit dans le prompt du nœud, soit en modifiant la pipeline elle-même.

---

## `code-mutating` vs `doc-only`

Chaque Node est typé par son **effet sur le code** :

- **`code-mutating`** — Implementer, Refactorer, Migrator, Merge Resolver. Reçoit un sous-worktree forké depuis la branche du Pipeline Run. Peut éditer/commit/merger. À la fin du NodeRun, son sous-worktree est mergé dans la branche du Pipeline Run.
- **`doc-only`** — Planner, Reviewer, Architect, PRD-writer. Pas de sous-worktree. Lit la branche du Pipeline Run en read-only (`git show`, `git diff`, `git log`). Écrit uniquement dans le Blackboard.

Garde-fou : à la fin d'un NodeRun `doc-only`, la branche du Pipeline Run doit rester intacte (pas de commit). Si une violation est détectée, le NodeRun échoue.

Conséquence sur la parallélisation : les `doc-only` sont gratis-parallèles (pas de merge possible). Les `code-mutating` parallèles voient leurs branches mergées séquentiellement à la fin (ordre de complétion).

---

## Merge Resolver

Rôle `code-mutating` **built-in** (livré par défaut, prompt overridable au niveau du pipeline) auto-spawné quand un `git merge` entre branches `code-mutating` parallèles produit un conflit.

- **Trigger déterministe** : conflit Git, signal mécanique. Pas d'orchestration LLM ambiante — c'est le runtime qui décide de spawn, pas un agent.
- **Mission** : pas un résolveur syntaxique. Le Merge Resolver lit les artefacts du Blackboard produits par chaque branche amont (summaries, plans, reviews) pour reconstituer l'**intention** de chaque Implementer, puis fusionne en préservant les deux intentions.
- **Gate** : suppression des markers + `git status` clean + commit posé. Aucune validation sémantique : si le pipeline n'a pas de Reviewer downstream pour rattraper une fusion bancale, c'est un défaut de design du pipeline (cf. principe *sharp-tool*).
- **Toggle** : `auto_merge_resolver: enabled | disabled` au niveau pipeline.

---

## Principe — Sharp tool, not safe tool

L'outil ne contraint pas l'utilisateur à dessiner des pipelines "sains". Pas de validation prescriptive du graphe (genre *"interdit fan-out `code-mutating` sans Reviewer downstream"*), pas de warnings paternalistes. Si une pipeline est foireuse — fan-out non revu, accumulation infinie, deadlock conceptuel — c'est la responsabilité du designer du pipeline. Maestro fournit des primitives nettes ; l'usage est libre.

Conséquences à anticiper sur les décisions futures :
- Pas de schéma rigide imposé sur les ports (cf. Q6 à venir) ; au plus du typage opportuniste.
- Pas de "lint pipeline" bloquant. Au max, un lint info-only.
- L'éditeur permet des graphes "exotiques" (cycles, fan-out `code-mutating` sans Merger explicite, ports déconnectés). Le runtime se débrouille ou halt explicitement.

---

## Principe — Deliberate over autonomous

Maestro ne vise pas le *"set it and forget it"*. La valeur est dans le **temps passé en conception**, pas dans la rapidité d'exécution. Conséquences :

- **Tout NodeRun est attachable** en tmux à n'importe quel moment ; l'utilisateur peut intervenir, converser, corriger.
- **Un Node peut être marqué `interactive: true`** à l'édition. Quand son NodeRun spawn, il s'arrête en attente que l'utilisateur attache la session et signale la complétion (slash command, fichier sentinelle, ou autre — TBD). Cas typique : nœud d'entrée qui grille l'utilisateur pour construire l'input du pipeline (à la `grill-with-docs`).
- **Le Pipeline Manager** (Q8) est conversationnel et permet de débloquer des Runs (relancer un cycle pour N itérations de plus, etc.) — pas juste de lire l'état.
- **Pas d'auto-merge vers main, jamais.** Pas d'auto-cleanup. L'humain tranche les actions à effets durables.

À distinguer de *Sharp tool* (ADR-0001) : *Sharp tool* parle de l'**éditeur** (on ne contraint pas le design). *Deliberate over autonomous* parle du **runtime** (on ne court-circuite pas l'humain à l'exécution).

---

## Pipeline Run — cycle de vie

### Input

Un Run prend un **input unique**, qui est soit :

- du **free-text prompt** (description en texte libre),
- une **référence d'issue** GitHub (URL ou `#123` — résolue via `gh issue view`),
- un mélange des deux dans le free-text (l'utilisateur colle un lien d'issue dans son prompt — le nœud d'entrée, qui est un Claude Code avec accès à tous ses tools/MCP, va lui-même chercher l'info).

Le runtime ne distingue pas (i) de (ii) : il pose le contenu utilisateur tel quel dans un artefact `<artifacts>/_input.md` du Blackboard. Le nœud d'entrée se débrouille à partir de là.

L'input peut aussi être **construit interactivement** via un nœud d'entrée marqué `interactive: true` (cf. principe *Deliberate over autonomous*). Pattern typique : le user écrit un prompt brut court, attache la session du nœud d'entrée, l'agent grille jusqu'à un input structuré, le user "submit", le pipeline démarre vraiment.

### Termination

À la fin d'un Run réussi, **niveau 0** par défaut : la branche `maestro/run-<run-id>` reste en l'état, le worktree reste sur disque, l'utilisateur fait ce qu'il veut. Maestro ne fait **pas** de PR auto, **pas** de commentaire d'issue, **pas** d'auto-merge. Si un projet veut ce comportement, il l'exprime en ajoutant un nœud "Shipper" dans son pipeline (un Claude Code avec `gh pr create` dans son prompt).

### Échec / blocage

NodeRun en échec, halt déclenché par une `when:` clause (`run_halted` event), Merge Resolver foiré, etc. → le Run passe en status `BLOCKED` ou `FAILED`. La branche pipeline et les sous-worktrees restent vivants pour debug. **Pas d'auto-cleanup, jamais.** L'utilisateur peut :

- Cleanup manuel intégral (suppression branches/worktrees).
- Reprendre la main directement sur la branche.
- Débloquer via le **Pipeline Manager** : conversation au cours de laquelle le user peut, par exemple, demander *"continue le cycle pour 3 itérations de plus"*. Le manager dispose des commandes pour modifier l'état runtime (cf. Q8).

### Parallélisation entre Runs

Plusieurs Runs du même pipeline (ou de pipelines différents) peuvent tourner simultanément sur le même repo target. Convention de nommage qui garantit l'absence de collision :

- Branche : `maestro/run-<run-id>` (ex. `maestro/run-2026-05-05-1430-a3f`).
- Worktree pipeline : `<repo>/.maestro/runs/<run-id>/worktree/`.
- Sous-worktrees `code-mutating` : `<repo>/.maestro/runs/<run-id>/nodes/<node-id>/iter-<N>/`.
- Blackboard : `<pipeline-worktree>/.maestro/artifacts/...` (déjà défini).

`<run-id>` = slug `<timestamp>-<short-uuid>` pour rester lisible humainement et garanti unique.

---

## Pipeline Manager

Agent conversationnel attaché à un Pipeline Run. Permet à l'utilisateur de **lire l'état** et **émettre des commandes** sur le Run.

### Cycle de vie

- **Un manager par Run.** Spawn automatique au démarrage du Run dans une session tmux dédiée nommée `maestro-mgr-<run-id>`. Persiste tant que le Run n'est pas cleanup (donc aussi après success/failed/blocked, pour interrogation post-mortem).
- **Pas de polling actif.** Le manager ne tourne effectivement que quand l'utilisateur lui parle. Quand attaché, il lit l'état frais à la demande.

### Implémentation

- Le manager **est** une instance Claude Code standard, pas un agent custom.
- Son **prompt système est augmenté** par le runtime avec :
  - L'identité du Run qu'il gère (`<run-id>`).
  - La liste des **endpoints HTTP** du daemon Maestro accessibles (URL de base, schéma, exemples d'invocation curl).
  - La liste des **commandes** disponibles avec leur payload attendu.
- **Pas de MCP custom.** L'agent appelle les endpoints via `bash` + `curl`. Justification : MCP est utile pour des clients agentiques distants/inconnus ; ici on possède le prompt de la session, autant documenter les endpoints en clair.
- Pour la lecture brute (sans passer par les endpoints), le manager a accès à `bash` complet : `ls`, `cat`, `git log`, `tmux capture-pane`, etc. Tout l'état du Run est sur disque, donc grep-able.

### Commandes disponibles (v1)

Toutes exposées comme endpoints `POST /runs/<id>/commands` du daemon :

| Commande | Effet |
|---|---|
| `extend_cycle` | Augmente le `max_iter` d'un cycle bloqué de N et relance |
| `resume_run` | Relance le Run depuis l'état actuel (utile post-conflit résolu manuellement) |
| `kill_node` | Tue un NodeRun en cours |
| `restart_node` | Re-spawn un NodeRun (perd l'état tmux courant) |
| `mark_node_done` | Force la complétion d'un NodeRun (cas typique : nœud `interactive: true` que l'utilisateur signale fini) |
| `inject_artifact` | Pose un artefact à la main dans le Blackboard |
| `cleanup_run` | Supprime branches, worktrees, artefacts, événements |

L'effet de chaque commande est l'**append d'un événement** dans l'event log. Le runtime consomme ces événements et agit en conséquence.

### Ce que le manager ne peut **pas** faire

- **Spawner des sous-agents ad hoc.** Pas d'orchestration probabiliste émergente. Le manager parle, lit, et exécute des commandes prédéfinies. Si l'utilisateur veut une investigation profonde, il attache directement la session tmux du nœud concerné.

---

## Architecture runtime — event-sourced

### Source de vérité = event log

Toutes les transitions d'état d'un Pipeline Run sont enregistrées comme **événements append-only** dans une **SQLite locale** (`~/.maestro/maestro.db`). L'état courant d'un Run = projection des événements de ce Run.

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

### Daemon Maestro

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
```

### Conséquence pour la prompt augmentation

Le préambule runtime injecté dans chaque NodeRun (cf. section *Prompt augmentation*) inclut, en plus des chemins d'inputs/outputs, **l'URL de base du daemon** (`http://localhost:<port>`) pour les nœuds qui en ont besoin (typiquement le manager, mais aussi un nœud "Shipper" qui voudrait poster un commentaire sur l'issue source via les endpoints, etc.).

---

## Sessions tmux

### Modèle d'exécution

Chaque NodeRun = **une session tmux détachée** créée par le daemon (`tmux new-session -d -s <name>`). Le contenu de la session est Claude Code en mode interactif, lancé avec le prompt augmenté du Node. Conventions de nommage :

- NodeRun : `maestro-<run-id>-<node-id>-iter-<N>`.
- Manager : `maestro-mgr-<run-id>` (cf. section *Pipeline Manager*).

Les sessions sont **invisibles à l'utilisateur** par défaut — pas de fenêtre OS qui s'ouvre. Elles tournent en arrière-plan et survivent au crash de l'UI ou du daemon (le runtime peut récupérer leur état au redémarrage).

### Pont UI ↔ tmux : option A (terminal natif spawn-on-demand)

L'UI affiche pour chaque NodeRun :

- **Statut** (pending / running / awaiting_user / done / failed / blocked) — projeté depuis l'event log.
- **Preview** — pull périodique (~1-2 s) de `tmux capture-pane -pe -S -1000 -t <session>`, rendu read-only avec ANSI dans l'UI. Push WebSocket-driven possible plus tard pour realtime parfait.
- **Bouton "Open terminal"** — déclenche un `POST /sessions/<id>/attach` sur le daemon, qui `exec` un terminal OS natif (`gnome-terminal` / `konsole` / `Terminal.app` / `kitty`...) avec `tmux attach -t <session>`. Une fenêtre OS apparaît, l'utilisateur tape dedans, détach standard `Ctrl+B D`. La session tmux survit à la fermeture de la fenêtre.

Détection du terminal préféré : variable `MAESTRO_TERMINAL` ou heuristique sur `$TERM_PROGRAM` / OS / `which`.

### Nœuds interactifs — signal de complétion

Un Node marqué `interactive: true` spawn une session tmux normale, et **n'auto-complète jamais**. La session reste attachable indéfiniment ; l'utilisateur peut détach/réattacher autant de fois que nécessaire et continuer à interagir.

La complétion est signalée **depuis l'UI**, par un bouton "Mark complete" sur le nœud. Click → `POST /runs/<id>/commands { kind: "mark_node_done", node_id, iter }`. Pas de slash-command in-session (un slash-command suppose qu'on est attaché ; le bouton UI reste toujours accessible).

À ce moment-là, les artefacts présents sur disque dans `<artifacts>/<node-id>/iter-<N>/` sont considérés comme finaux. Le préambule du nœud le dit explicitement à l'agent et au user : *"écris tes outputs aux chemins X, Y, Z ; quand tu cliques 'Mark complete' dans l'UI, ces fichiers seront pris tels quels"*.

---

## UX — modes Run et Edit

Maestro est un **atelier de production de code** ; la conception de pipelines est un *moyen*, pas le centre de gravité. La disposition par défaut reflète ça.

> **Source visuelle de référence** : voir [`docs/design/`](./docs/design/) pour les 8 écrans rendus en HTML/CSS/JS (bundle Claude Design). Le `README.md` de ce dossier mappe chaque écran à la section correspondante du présent document. Les tokens CSS de `docs/design/project/styles.css` sont la spec de design tokens (couleurs, typo, espaces) — à reprendre tels quels dans le frontend React/Vite.

### Mode Run (par défaut au lancement)

Layout 3 panneaux :

- **Gauche — Liste des Runs.** Runs actifs en haut (status `running`/`awaiting_user`/`blocked`), Runs récents en dessous. Click sur un Run → bascule l'affichage middle/droite.
- **Centre — Vue graphe du Run sélectionné.** Render du DAG de la pipeline, avec :
  - **Highlight** sur le(s) nœud(s) en cours d'exécution (pluriel — fan-out parallèle peut en avoir plusieurs simultanés).
  - **Encart overlay** flottant : run-id, pipeline name + version, variables en cours, status global, boutons d'action niveau Run (cancel, cleanup, attacher manager).
  - Click sur n'importe quel nœud (en cours ou pas) → bascule le panneau de droite.
- **Droite — Détail du nœud sélectionné.** Pour le NodeRun sélectionné :
  - **Preview du terminal** (rendu read-only de `tmux capture-pane -pe`).
  - **Inputs résolus** : noms des ports + chemins absolus des artefacts amont + bouton "open" pour les lire dans un viewer markdown.
  - **Outputs produits** : pareil pour les fichiers du nœud lui-même.
  - **Prompt initial** : visualisation du préambule runtime + prompt-utilisateur tels que reçus par le Claude Code de cette session. Permet de debug "qu'est-ce que l'agent a vu en entrée".
  - Bouton **"Open terminal"** (cf. Q9b — spawn natif d'un terminal OS attaché à la session tmux).
  - Bouton **"Mark complete"** si le nœud est interactif et en attente.

Realtime via WebSocket depuis le daemon → chaque événement de l'event log push une update vers l'UI.

### Mode Edit (toggle via icône crayon)

Bascule globale, signalée par une icône crayon dans la chrome de l'app. En mode Edit :

- **Gauche** — la liste devient les **Pipelines** (définitions), pas les Runs. Toutes les pipelines disponibles : repo-scoped (`<repo>/.maestro/pipelines/`) + user-scoped (`~/.maestro/pipelines/`), avec badge `repo` / `user`.
- **Centre** — canvas éditable. Drag-drop de nœuds, création d'edges, sélection multiple, déplacement. Pas de validation bloquante (cf. ADR-0001).
- **Droite** — formulaire de configuration du nœud / edge / pipeline sélectionné. Champs : name, type (`code-mutating`/`doc-only`), `interactive`, prompt (textarea reliée au `prompt_file` correspondant), inputs, outputs (avec frontmatter schema). Pour une edge sélectionnée : `when:` clause, target type. Pour la pipeline elle-même (rien de sélectionné) : nom, description, variables, config.
- **Onglets** — multi-pipeline ouvert en parallèle. Copier-coller de nœuds entre onglets supporté.

### Workflow utilisateur typique au démarrage

1. **Monitor** : ouvre Maestro, voit ses Runs actifs, debug un Run bloqué via le manager ou en attachant directement.
2. **Lancer un nouveau Run** : depuis la liste de gauche, bouton "+ New Run", modale avec sélecteur de pipeline + textarea input (free-text ou lien d'issue ou mix) + accordion "variables overrides" (déplié au besoin, valeurs par défaut sinon). Confirme → POST `/runs`, le Run apparaît dans la liste.
3. **Créer/modifier une pipeline** : toggle crayon → bascule en mode Edit, choisit "+ New Pipeline" ou édite une existante.

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

Pas de "permanent delete" v1. Le bouton "Cleanup" sur un Run terminé :

- supprime la branche `maestro/run-<run-id>`,
- supprime le worktree pipeline et tous les sous-worktrees,
- supprime le dossier des artefacts (Blackboard) du Run.

**Mais ne touche pas à l'event log** : les événements en SQLite restent. Le Run passe en status `archived`, reste dans la liste de gauche avec un icône gris, et reste **interrogeable post-mortem** (history, manager peut encore répondre à *"qu'est-ce qui s'est passé sur ce Run ?"* en lisant les events). Pas d'auto-cleanup, jamais.

L'event log peut grossir indéfiniment ; on évalue la taille avant de décider d'une politique de purge. Pas de v1.

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

### Mono-user, local

Le daemon écoute sur `127.0.0.1:<port>` uniquement. Pas d'auth, pas de TLS, pas de multi-user. Single-user local par design. Tout ce qu'il faut pour ça : SQLite locale, FS local, tmux local, git local. Pas de dépendance réseau.

### Persistance et hot-reload

- **Auto-save debounced** (1-2 s d'inactivité) sur toutes les modifications du canvas en mode Edit. Pas de "Ctrl+S", pas de modal. Le canvas EST le fichier YAML + les fichiers prompts.
- **Hot-reload bidirectionnel** : Maestro watch les fichiers (`fswatch`/`inotify`). Édition externe (Vim, VS Code) → re-parse et re-render. Last-write-wins.
- **Pas de git intégration v1.** Le user fait ses commits manuellement s'il versionne.

### Création d'un nouveau nœud

- **From scratch** : "+ Add node" → nœud vide à remplir.
- **Duplicate existing** : right-click sur un nœud → copie avec id auto-incrémenté.
- **Pas de library de templates Maestro-shipped en v1** (cohérent avec ADR-0001 : pas d'opinion vendor sur "à quoi ressemble un Implementer").
