# Pyramide de tests inversée + agent en couche 5

PDO a livré 186 unit tests verts pour des modules à couture forte (file watcher ↔ WebSocket, scheduler ↔ event log, frontend ↔ daemon, tmux/git/fs side-effects). Au premier lancement du binaire, rien ne fonctionne : bundle frontend stale (build.rs manquant), session tmux qui meurt à la naissance (claude jamais invoqué), Edit mode qui s'écrase tout seul (file watcher self-write loop). Aucune couche au-dessus du test unitaire n'avait été pensée — chaque slice avait validé son module en isolation, jamais l'effet utilisateur.

**Décision : 5 couches, centre de gravité en couche 3.**

1. **Unit** (`#[cfg(test)]` inline) — logique pure : parser, condition evaluator, prompt augmenter, frontmatter parser, serializer.
2. **Intégration HTTP in-process** (`tower::ServiceExt::oneshot`) — contrats API : status codes, payloads, validation.
3a. **Cargo intégration daemon réel** (`crates/pdo-daemon/tests/`) — coutures backend : daemon spawné sur port éphémère, vrai notify, vraie sqlite, vrai axum, vrai broadcast WS, vrai tmux. Pas de mocking. *(Pour les tests qui spawnent une session tmux, on substitue `bash -c 'sleep 60'` à `claude` — claude n'est pas en CI ; sa validation réelle vit en couche 5.)*
3b. **Playwright** (`frontend/e2e/`) — coutures browser ↔ daemon, parcours UI déterministes (Edit + save sans écrasement, NewRun modal s'ouvre, etc.). Daemon spawné par Playwright via `webServer` config.
4. **Smoke bash** (`tests/smoke.sh`) — pre-merge, gratuit : daemon démarre, `/runs` et `/pipelines` répondent JSON, `index.html` contient "PDO", asset JS répond 200.
5. **Scénarios agentiques** (`docs/testing/scenarios/*.md`) — manuel pre-release : un agent Claude Code joue un parcours utilisateur (UI via Chrome DevTools MCP / Playwright, Bash, `tmux capture-pane`, lecture filesystem), juge PASS/FAIL avec rationale. MVP démarre avec deux scénarios : `run-minimal` (lance un Run réel et observe la complétion) et `edit-and-save` (modifie une pipeline en mode Edit et vérifie la persistance).

**Règle d'or : aucune AC fermée sans test couche ≥3.** Les tests couche 1 et 2 sont insuffisants pour valider une slice — c'est ce qu'on vient de prouver. La règle est écrite ici, pas enforcée par CI : elle dépend de la review humaine.

**Pourquoi.** Pas de mocking au-dessus de couche 1 — sinon les tests mentent (le file watcher mocké aurait reproduit le bug E parfaitement, sauf que le bug n'aurait pas existé). Couche 5 agentique plutôt que bash-only parce qu'un bash teste des invariants techniques mais pas l'expérience utilisateur (le DAG s'anime ? le footer dit "connected" ? le terminal attaché reste vivant ? le contenu de la session tmux montre claude qui tourne ?) — un agent qui pilote l'UI peut juger ça. Couche 5 reste manuelle au MVP : coût API Anthropic non-négligeable, flakiness à arbitrer cas par cas, on n'industrialise pas avant d'en avoir besoin.

**Alternatives écartées.** Cypress (3b) — moins flexible que Playwright, monobrowser, opinionated d'une façon qu'on évite. Couche 5 tout-bash — décrit ci-dessus, ne juge pas l'UX. Pyramide classique avec dominante unit — vient d'échouer, on n'y retourne pas.
