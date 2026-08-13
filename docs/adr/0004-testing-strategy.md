# Pyramide de tests inversée + agent en couche 5

PDO a livré 186 unit tests verts pour des modules à couture forte (file watcher ↔ WebSocket, scheduler ↔ event log, frontend ↔ daemon, tmux/git/fs side-effects). Au premier lancement du binaire, rien ne fonctionne : bundle frontend stale (build.rs manquant), session tmux qui meurt à la naissance (claude jamais invoqué), Edit mode qui s'écrase tout seul (file watcher self-write loop). Aucune couche au-dessus du test unitaire n'avait été pensée — chaque slice avait validé son module en isolation, jamais l'effet utilisateur.

**Décision : 5 couches, centre de gravité en couche 3.**

1. **Unit** (`#[cfg(test)]` inline) — logique pure : parser, condition evaluator, prompt augmenter, frontmatter parser, serializer.
2. **Intégration HTTP in-process** (`tower::ServiceExt::oneshot`) — contrats API : status codes, payloads, validation.
3a. **Cargo intégration daemon réel** (`crates/pdo-daemon/tests/`) — coutures backend : daemon spawné sur port éphémère, vrai notify, vraie sqlite, vrai axum, vrai broadcast WS, vrai tmux. Pas de mocking. *(Pour les tests qui spawnent une session tmux, on substitue `bash -c 'sleep 60'` à `claude` — claude n'est pas en CI ; sa validation réelle vit en couche 5.)*
3b. **Playwright** (`frontend/e2e/`) — coutures browser ↔ daemon, parcours UI déterministes (Edit + save sans écrasement, NewRun modal s'ouvre, etc.). Daemon spawné par Playwright via `webServer` config.
4. **Smoke bash** (`tests/smoke.sh`) — pre-merge, gratuit : daemon démarre, `/runs` et `/pipelines` répondent JSON, `index.html` contient "PDO", asset JS répond 200.
5. **Scénarios agentiques** — un agent Claude Code joue un parcours utilisateur (UI via navigateur, Bash, `tmux capture-pane`, lecture filesystem) et rend des **findings** (bloquant / non-bloquant). Deux niveaux : les **Happy Paths (HP)** — suite **permanente, ≤ 3**, dans `docs/test-scenarios/` : les parcours pris par 80 %+ des utilisateurs (HP-01 *author & save*, HP-02 *run to completion*), qui valent autant comme documentation du chemin critique que comme non-régression ; gate `integration → develop` (décision humaine). Et les **Feature Paths (FP)** — dans la section « Acceptance criteria → Feature Path » de la sous-issue, **jetables** (meurent avec l'issue) ; gate sous-issue → `integration` (auto-merge). Runner = skill `/agentic-tests` ; pilotage PDO = `docs/agents/run-scenario.md` ; gates détaillées dans `git-flow`.

**Règle d'or : aucune AC fermée sans test couche ≥3.** Les tests couche 1 et 2 sont insuffisants pour valider une slice — c'est ce qu'on vient de prouver. La règle est écrite ici, pas enforcée par CI : elle dépend de la review humaine.

**Pourquoi.** Pas de mocking au-dessus de couche 1 — sinon les tests mentent (le file watcher mocké aurait reproduit le bug E parfaitement, sauf que le bug n'aurait pas existé). Couche 5 agentique plutôt que bash-only parce qu'un bash teste des invariants techniques mais pas l'expérience utilisateur (le DAG s'anime ? le footer dit "connected" ? le terminal attaché reste vivant ? le contenu de la session tmux montre claude qui tourne ?) — un agent qui pilote l'UI peut juger ça. La couche 5 reste hors CI : coût API Anthropic non-négligeable, flakiness à arbitrer cas par cas, on n'industrialise pas avant d'en avoir besoin.

**Alternatives écartées.** Cypress (3b) — moins flexible que Playwright, monobrowser, opinionated d'une façon qu'on évite. Couche 5 tout-bash — décrit ci-dessus, ne juge pas l'UX. Pyramide classique avec dominante unit — vient d'échouer, on n'y retourne pas.

**La résilience n'est pas un Happy Path.** Les invariants d'adversité (mort de session, kill du daemon, fuite de slot d'admission, rejet d'édition mid-run, « jamais de stall silencieux ») sont des cas limites, coûteux à jouer, et **couverts en permanence par les tests automatisés de couche 3** (`crates/pdo-daemon/tests/` et `frontend/e2e/`) — pas par un HP.
