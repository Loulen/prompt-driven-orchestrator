# Rendu mermaid client-side — `strict` sans CSP, version épinglée

PDO affiche le corps markdown des artefacts de NodeRun dans `MarkdownArtifactModal`
(l'unique instanciation de `react-markdown`, donc *toute* la surface markdown
agent-authored : aucune autre vue ne rend du texte markdown — les autres surfaces
n'affichent que métadonnées et vignettes d'images). #240 demande de rendre les blocs
` ```mermaid ` du corps en vrais diagrammes plutôt qu'en code fencé brut.

Le contenu rendu est **écrit par un agent Claude Code** — donc atteignable par
prompt-injection : un agent compromis ou dérivé peut émettre du mermaid hostile.
mermaid compile ce source en SVG, et ce SVG doit être injecté dans le DOM via
`dangerouslySetInnerHTML` — **le premier de tout le frontend**. L'app n'a **aucune
CSP** (vérifié : rien côté daemon Axum ni côté HTML servi ; le daemon écoute
`127.0.0.1`, mono-user, sans auth ni TLS). Il n'y a donc pas de filet réseau derrière
le sink : la sécurité repose entièrement sur le sanitizer.

**Décision.** On rend le mermaid **client-side**, dans un composant `MermaidDiagram`
monté depuis l'override `pre` de `MarkdownArtifactModal`, avec :

- **`securityLevel: 'strict'`** (mermaid injecte le texte des labels comme *texte*,
  échappe le HTML, et passe son SVG par le DOMPurify qu'il **embarque déjà**, configuré
  pour préserver les `<foreignObject>`). On retient `strict` plutôt que `sandbox`
  (iframe) : `strict` garde le diagramme intégré au flux markdown et au sizing du modal ;
  `sandbox` isole plus fort mais casse l'intégration visuelle et ajoute scroll/sizing
  d'iframe. Décision owner-ratifiée 2026-06-22.
- **`mermaid` épinglé `^11.15.0`.** La version *fait partie de la frontière de sécurité*,
  pas un détail de dépendance. Plancher justifié par la **chaîne de GHSA publiée
  2026-05-11, corrigée en 11.15.0** (CVE-2026-41149 `classDef` HTML-injection ;
  GHSA-xcj9-5m2h-648r et GHSA-87f9-hvmw-gh4p CSS-injection ; GHSA-6m6c-36f7-fhxh DoS
  Gantt). Les CVE XSS antérieures (CVE-2025-54880/54881, sequence/architecture) sont
  déjà corrigées en 11.10.0 — donc couvertes a fortiori. Ne jamais dé-épingler sous ce
  plancher.
- **`secure: [...]` verrouillé + `suppressErrorRendering: true`** à l'`initialize`, pour
  qu'une frontmatter / un `%%{init}%%` par-diagramme ne puisse pas ré-élever le niveau de
  sécurité à l'exécution. `securityLevel` et `suppressErrorRendering` sont déjà dans le
  `secure` par défaut ; on le pose **explicitement** pour rendre le verrou visible en
  review.
- **Pas de passe DOMPurify applicative *par défaut*.** mermaid sanitise déjà en interne
  sous `strict` avec la config qui préserve les labels `<foreignObject>` ; une passe
  applicative *en config par défaut* **casse** ces labels **et** rate la classe de CVE
  visée (le texte non-fiable atteint le sink *avant* le sanitizer de mermaid). On épingle
  la version à la place. (Nuance : une passe *correctement configurée* —
  `USE_PROFILES:{svg:true,svgFilters:true}, ADD_TAGS:['foreignObject']` — serait une
  défense-en-profondeur valide ; on ne la retient pas v1 pour ne pas ajouter une
  dépendance non-semver à la frontière de sécurité.)
- **Dégradation douce** : tout échec de `parse`/`render` retombe sur le `<pre><code>`
  brut (le diagramme reste lisible comme source). C'est cohérent avec la posture
  *surface, ne masque pas* de PDO — **à ne pas confondre** avec *Sharp tool* (ADR-0001),
  qui parle du refus de contraindre le **design de pipeline**, pas du rendu UI.

**Pourquoi.** Le contenu est agent-authored et prompt-injection-reachable, donc on ne
peut pas le traiter comme du HTML de confiance. Sans CSP, le sink
`dangerouslySetInnerHTML` n'a qu'une seule ligne de défense : le sanitizer. `strict` +
version épinglée + config verrouillée fait du sanitizer interne de mermaid cette ligne,
sur une cible *mono-user, local, `127.0.0.1`* où la surface d'attaque réseau est minime.

Alternatives considérées :

- **`securityLevel: 'sandbox'` (iframe)** — rejeté comme défaut, mais conservé comme
  bascule défensive. Isolation plus forte (le SVG vit dans un iframe sandboxé, hors du DOM
  principal), donc vraie défense-en-profondeur vu l'absence de CSP. Perd l'intégration au
  flux markdown et complique le sizing dans un modal de 560px. *Consciemment* retenu comme
  l'alternative-sécurité, pas comme un oubli.
- **Pas de mermaid (laisser le code fencé brut)** — rejeté : c'est le besoin même de #240.
  Le fallback `<pre><code>` *est* cet état, mais en chemin d'erreur uniquement.
- **Prérendu server-side (daemon → SVG)** — rejeté : déplace un moteur de rendu lourd
  (headless browser / node) dans un daemon Rust mono-user, alourdit le build et l'embed du
  binaire, et n'élimine pas le besoin d'injecter du SVG côté client. Sur-ingénierie pour
  une cible locale.
- **Passe DOMPurify applicative en plus (v1)** — rejeté pour v1 (voir Décision) : casse les
  labels `<foreignObject>` en config par défaut, et DOMPurify n'est pas semver, ajoutant
  une dérive de sécurité au lieu d'en retirer.

Conséquences :

- **Premier `dangerouslySetInnerHTML` du frontend.** Le pattern existe désormais ; tout
  futur sink raw-HTML doit être tenu au même standard (source de confiance ou sanitizer
  audité).
- **`mermaid` devient une dépendance security-sensitive.** Toute bump/downgrade passe par
  la grille « est-ce que ça reste ≥ plancher CVE (11.15.0) ? ». Lazy `import()` pour
  code-splitter (mermaid est lourd, >500 kB) — sans impact sécurité.
- **Risque résiduel assumé — pas de CSP.** Si une CVE mermaid franchit `strict` *avant*
  qu'on bump, il n'y a aucun second rideau. Le risque est borné par la cible (mono-user,
  `127.0.0.1`, sans auth — un attaquant capable d'écrire le mermaid via un agent a déjà un
  pied dans l'environnement local) et atténué par le pin `^11.15.0` + la config verrouillée.
  **Mitigation recommandée en fast-follow** : ajouter une CSP minimale app-wide
  (`script-src 'self'` sans `'unsafe-inline'`, `object-src 'none'`, `base-uri 'none'`) —
  contrôle le plus rentable, app-wide, qui neutralise l'exécution de script de tout futur
  bypass `strict` sans réverer le choix `strict`-over-`sandbox` (contrairement à `sandbox`,
  une CSP ne casse ni les liens intra-app ni le sizing SVG). Tracé comme dette de sécurité
  explicite.

Interagit avec ADR-0004 (la couche de test qui valide ce rendu est Layer 3b Playwright +
Layer 5 agentique, jsdom ne pouvant pas exécuter mermaid faute de `getBBox`).
