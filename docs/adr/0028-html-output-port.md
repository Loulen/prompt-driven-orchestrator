# ADR-0028 — Type de port de sortie `html` : artefact HTML rendu, statique et sandboxé

Date : 2026-07-18 · Statut : accepté · Issue : #333

## Contexte

Sans cette ADR, on ajoute un port `html` en le servant comme les autres et en le rendant dans le
DOM. Or le contenu d'un artefact est **écrit par un agent**, donc atteignable par prompt-injection —
le modèle de menace d'ADR-0013 (mermaid), mais pire : du HTML arbitraire, pas un SVG compilé. L'app
n'a **aucune CSP** et le daemon écoute sans auth. ADR-0018 avait explicitement différé toute
nouvelle surface de contenu rendu à « son propre ADR » : c'est celui-ci.

## Décision

Ajouter un quatrième type de port `html` (après `markdown`, `image`, `image_list`, ADR-0010).
Périmètre v1 : **document statique** (HTML + CSS), **JS non exécuté** (tranché par l'owner).

- **Production.** L'agent (ou un node `script`) écrit un unique **`output.html`** dans le répertoire
  du port ; le validateur d'outputs vérifie l'existence du fichier (miroir de markdown), pas de
  frontmatter.
- **Rendu.** L'inspecteur ouvre l'artefact dans une **`<iframe sandbox="" srcDoc={texte}>`** : liste
  de permissions vide → pas d'exécution de script, origine opaque, pas de soumission de formulaire,
  pas de navigation top-level. Le HTML transite par `srcDoc`, **jamais** par
  `dangerouslySetInnerHTML`. C'est la route d'isolation qu'ADR-0013 avait conservée comme
  alternative défensive, ici appliquée à une classe de contenu où le `securityLevel: strict` in-DOM
  n'a pas de sens.
- **Le daemon ne sert JAMAIS l'artefact en `text/html`** : `.html` tombe dans le défaut
  `text/markdown` de la mime map, verrouillé par un test de régression. Servir `text/html` ferait de
  `/artifact?path=x.html` un document navigable exécutant du script agent à l'origine daemon, ce qui
  contournerait la sandbox du viewer.
- **`html` est une surface de relecture, non consommée en aval** (v1). La résolution d'input du
  runtime est aveugle au type et lit `output.md`, donc un port html branché en aval résout vers un
  fichier absent — même comportement qu'un port image aujourd'hui.

## Alternatives rejetées

- **Variante interactive (JS exécuté).** Rouvrirait le fork sécurité-vs-fonction de #240 sur un
  daemon sans auth ni CSP → ADR + issue dédiés.
- **Réutiliser `output.md` pour porter le HTML.** Nom de fichier trompeur sur disque et à
  l'archivage, pour un gain nul.
- **Enforcement dur « pas d'edge depuis un port html ».** Net-new, sans template (image/image_list
  ne sont pas non plus protégés) ; exigerait une décision cohérente pour tous les leaf → follow-up.

## Conséquences

- Défense-en-profondeur réelle malgré l'absence de CSP : le HTML vit dans une iframe hors du DOM
  principal.
- Résiduel : un `<meta http-equiv="refresh">` ou un lien dans l'iframe sandboxée peut au plus
  auto-naviguer l'iframe (nuisance phishing/redirection), sans exécution de code ni accès à l'origine.
- L'artefact est préservé à l'archivage gratuitement (ADR-0020) ; la route de lecture résout la copie
  durable à l'identique.
- Dette : la CSP app-wide (dette ADR-0013) reste un fast-follow orthogonal, non bloquant.

## Relations

Étend ADR-0010 (4e type de port) en en **restreignant** la clause « consommable » à une surface de
relecture. Applique la route iframe-sandbox conservée par ADR-0013. Lève le report d'ADR-0018. Non
contraint par ADR-0008. Indépendant d'ADR-0012. Préservé à l'archivage par ADR-0020. Ne supersede
aucun ADR.
