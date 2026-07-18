# ADR-0028 — Type de port de sortie `html` : artefact HTML rendu, statique et sandboxé

Date : 2026-07-18 · Statut : accepté · Issue : #333

## Contexte

Les ports de sortie sont des répertoires typés (ADR-0010) : `markdown` (`output.md`), `image`,
`image_list`. La relecture experte (expert-in-the-loop, ADR-0012) se fait aujourd'hui sur du markdown
brut. #333 veut un artefact **HTML rendu** pour une relecture plus confortable.

Le contenu d'un artefact est **écrit par un agent Claude Code**, donc atteignable par prompt-injection —
exactement le modèle de menace d'ADR-0013 (rendu mermaid), mais pire : du HTML arbitraire, pas un SVG
compilé. L'app n'a **aucune CSP** et le daemon écoute `127.0.0.1` sans auth. ADR-0018 (note de canvas)
a explicitement différé toute nouvelle surface de contenu rendu à « son propre ADR » : c'est celui-ci.

## Décision

Ajouter un quatrième type de port `html`. Périmètre v1 : **document statique** (HTML + CSS), **JS non
exécuté** (tranché « statique » par l'owner).

- **Production.** L'agent (ou un node `script`) écrit un unique **`output.html`** dans le répertoire du
  port (helper `blackboard::artifact_path_html`, parallèle à `artifact_path`). `outputs_validator`
  vérifie l'existence du fichier (miroir de markdown) ; pas de frontmatter.
- **Rendu.** L'inspecteur ouvre l'artefact dans une **`<iframe sandbox="" srcDoc={texte}>`** : liste de
  permissions vide → pas d'exécution de script, origine opaque, pas de soumission de formulaire, pas de
  navigation top-level. Le HTML transite par la prop `srcDoc` (jamais `dangerouslySetInnerHTML`). C'est
  la route d'isolation « iframe sandboxée » qu'ADR-0013 avait conservée comme alternative défensive,
  ici appliquée à une classe de contenu où le `securityLevel: strict` in-DOM n'a pas de sens.
- **Le daemon ne sert JAMAIS l'artefact en `text/html`.** La route `/artifact` mappe l'extension à la
  main ; `.html` tombe dans `_ => "text/markdown"`. Servir `text/html` ferait de
  `/artifact?path=x.html` un document navigable exécutant du script agent à l'origine daemon. La mime
  map reste **inchangée** et un test de régression verrouille `.html → text/markdown`.
- **`html` est une surface de relecture, non consommée en aval** (v1). Rien ne route sur son contenu ;
  la résolution d'input du runtime est aveugle au type et lit `output.md`, donc un port html branché en
  aval résout vers un fichier absent — même comportement qu'un port image aujourd'hui.

## Alternatives rejetées

- **Variante interactive (JS exécuté).** Rouvrirait le fork sécurité-vs-fonction de #240 sur un daemon
  sans auth/CSP → ADR + issue dédiés. Hors périmètre v1.
- **Servir `.html` en `text/html` + `<iframe src>`.** Crée un document top-level exécutable à l'origine
  daemon, contournant la sandbox du viewer. Interdit.
- **Réutiliser `output.md` pour porter le HTML.** Nom de fichier trompeur sur disque et à l'archivage ;
  un `artifact_path_html` dédié est plus honnête pour un coût minime.
- **Enforcement dur « pas d'edge depuis un port html ».** Net-new, sans template (image/image_list ne
  sont pas non plus protégés), forcerait une décision cohérente pour tous les leaf → follow-up.

## Conséquences

- **Pas de `dangerouslySetInnerHTML`** : contrairement à ADR-0013, ce sink n'est pas utilisé ; le HTML
  vit dans une iframe hors du DOM principal. Défense-en-profondeur réelle malgré l'absence de CSP.
- Résiduel : un `<meta http-equiv="refresh">` ou un lien dans l'iframe sandboxée peut au plus
  auto-naviguer l'iframe (nuisance phishing/redirection), sans exécution de code ni accès à l'origine.
- L'artefact est préservé à l'archivage gratuitement (ADR-0020, c'est un fichier du Blackboard) ; la
  route de lecture résout la copie durable à l'identique.
- Dette : la CSP app-wide (dette ADR-0013) reste un fast-follow orthogonal, non bloquant.

## Relations

Étend ADR-0010 (4e type de port) en en **restreignant** la clause « consommable » à une surface de
relecture. Applique la route iframe-sandbox conservée par ADR-0013. Lève le report d'ADR-0018 (« toute
surface de contenu rendu exige son propre ADR »). Non contraint par ADR-0008 (aucun contrat frontmatter
amont). Indépendant d'ADR-0012 (n'initie aucun effet durable). Préservé à l'archivage par ADR-0020. Ne
supersede aucun ADR.
