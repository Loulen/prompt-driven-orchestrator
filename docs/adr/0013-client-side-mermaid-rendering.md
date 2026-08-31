# Rendu mermaid client-side — `strict` sans CSP, version épinglée

**Le sanitizer interne de mermaid est la seule ligne de défense du seul sink `dangerouslySetInnerHTML` du frontend : il n'y a aucune CSP derrière lui.** Le markdown rendu est le corps d'artefacts **écrits par un agent**, donc atteignable par prompt-injection ; le daemon est local, mono-user, sans auth ni TLS, et ne sert aucune CSP. D'où trois verrous, tous partie de la frontière de sécurité :

- **`securityLevel: 'strict'`**, retenu contre `sandbox` (iframe) qui isole plus fort mais casse l'intégration au flux markdown et au sizing du modal. Décision owner-ratifiée 2026-06-22 ; `sandbox` reste la bascule défensive assumée si `strict` devient insuffisant.
- **`mermaid` épinglé `^11.15.0` — la version *est* un contrôle de sécurité, pas un détail de dépendance.** Plancher justifié par la chaîne de GHSA publiée 2026-05-11 et corrigée en 11.15.0 (CVE-2026-41149 `classDef` HTML-injection, deux CSS-injection, un DoS Gantt). **Ne jamais dé-épingler sous ce plancher** ; tout bump/downgrade passe par cette grille.
- **Config `secure` verrouillée + `suppressErrorRendering`**, pour qu'un `%%{init}%%` par-diagramme ne puisse pas ré-élever le niveau de sécurité à l'exécution.

**Pas de passe DOMPurify applicative.** En config par défaut elle **casse** les labels `<foreignObject>` **et** rate la classe de CVE visée (le texte non-fiable atteint le sink *avant* le sanitizer de mermaid). Correctement configurée elle serait une défense-en-profondeur valide : non retenue v1 pour ne pas ajouter une dépendance non-semver à la frontière de sécurité. Le prérendu server-side est écarté aussi : il déplace un moteur de rendu lourd dans un daemon Rust mono-user sans supprimer l'injection SVG côté client.

**Dégradation douce** : tout échec de parse/render retombe sur le code fencé brut. Posture *surface, ne masque pas* — **à ne pas confondre** avec *Sharp tool* (ADR-0001), qui parle du refus de contraindre le design de pipeline, pas du rendu UI.

**Conséquences.** Le pattern `dangerouslySetInnerHTML` existe désormais : tout futur sink raw-HTML est tenu au même standard (source de confiance ou sanitizer audité). **Risque résiduel assumé** : si une CVE franchit `strict` avant un bump, il n'y a aucun second rideau. Mitigation recommandée en fast-follow, tracée comme dette de sécurité : une CSP minimale app-wide (`script-src 'self'` sans `'unsafe-inline'`, `object-src 'none'`, `base-uri 'none'`), qui neutralise l'exécution de script de tout bypass sans revenir sur le choix `strict`-over-`sandbox`.
