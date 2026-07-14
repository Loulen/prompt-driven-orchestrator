# Import de workflow — parsing par AST statique, jamais d'exécution du `.js`

L'importeur de workflows Claude Code (#155) « décompile » un `.claude/workflows/*.js` (workflow
dynamique CC) en un brouillon de pipeline PDO déposé en bibliothèque. Les prompts n'y sont **pas**
des littéraux inline : ils sont souvent construits par des helpers et interpolés à l'exécution
(`agent(implPrompt(p, branch), …)`, `` `…${bugReport}…` ``). Trois mécaniques de parsing
s'excluaient : **A** — AST Rust (`oxc`) ; **B** — sous-process `node` + stubs, qui *exécute* le JS
et récupère les prompts résolus ; **C** — regex.

**Décision : A — parsing par AST via `oxc`, zéro exécution du JS importé.** On lit l'AST et on
traduit les idiomes reconnus (`agent`/`parallel`/`pipeline`/`for`/`while`/`if` + schémas JSON) en
`PipelineDef`. Extraction des prompts : les string-literals **et** les template-literals **sans
interpolation** sont extraits **verbatim** ; un template-literal inline **avec** interpolation voit
son texte statique extrait verbatim et chaque trou `${…}` rendu comme marqueur annoté (câblable en
port d'entrée) ; un prompt **sans aucun texte statique** (appel de helper nu `agent(buildPrompt())`,
identifiant) devient un **placeholder annoté** — un nœud `doc-only` dont le corps explique quoi
écrire. Tout ce qui sort du sous-ensemble reconnu (boucle imbriquée, garde budgétaire,
`try/finally`, accumulation cross-lap) devient un placeholder annoté pédagogique.

**Pourquoi.** Contre **B** : le daemon bind `0.0.0.0` (LAN-reachable, cf. #260) ; exécuter du JS
arbitraire d'un fichier importé y serait une **RCE** — inacceptable pour un simple import. B ne voit
en outre qu'**un seul** chemin de contrôle (l'exécution réelle), là où l'AST voit **toutes** les
branches ; il faudrait de toute façon stubber `agent`/`parallel`/`pipeline`. Contre **C** : fragile
par construction (17 `if`, boucles imbriquées, edges implicites dans les fixtures) — un parseur de
langage ne se fait pas en regex. `oxc` est déterministe, sans effet de bord, et voit la structure
complète.

## Conséquences

- **Nouvelle dépendance** `oxc` (parser + AST + allocator) dans le workspace — lourde mais activement
  maintenue ; MSRV à vérifier contre l'edition 2021 du projet.
- L'import est une **décompilation AVEC perte** sur un sous-ensemble assumé : la valeur est
  l'onboarding (« importe le câblage, signale le reste »), pas la fidélité. L'annotation d'un
  placeholder **est** le tutoriel d'onboarding — elle traduit ce qu'un utilisateur PDO n'écrirait
  jamais à la main (gestion worktrees, auto-cleanup, boucle budgétaire) en interaction délibérée.
- **Aucun nouveau type de nœud** : le placeholder réutilise `doc-only` (inputs émergents → toute
  edge entrante est valide) ; les avertissements de traduction remontent via un canal `warnings`
  (`Vec<Diagnostic>`) dans le résultat d'import, cohérent avec le lint info-only d'ADR-0001.
- **Reconnaissance de boucle gardée sur la présence d'un `agent()` dans le corps** : un `while`/`for`
  de plomberie (double-décodage des `args`, retry `JSON.parse`) ne matérialise **pas** une boucle —
  sinon un `bounded` fantôme. Même garde pour `if` → edge conditionnelle.
- **Réutilisé pour l'import d'un nœud YAML unique** (#345) : `POST /nodes/parse` applique le même
  locus *parse-côté-daemon + canal `warnings[]`* à un **nœud nu** (désérialise une `LibraryEntry`,
  pas une pipeline ; disk-free — ni lecture ni écriture de la bibliothèque). Différence de nature :
  c'est un round-trip **natif et sans perte** (l'export d'un nœud produit exactement cette forme),
  pas la décompilation *avec perte* d'un format étranger — d'où le nom produit *Add node from YAML…*
  et non « import ». Le refus de `for-each` / le défaut-`doc-only` sont partagés via
  `pipeline::normalize_node_value`, seule source de vérité de la coercition de type.
