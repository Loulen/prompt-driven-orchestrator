# Pyramide de tests inversée + agent en couche 5

**Aucune AC n'est fermée sans un test de couche ≥ 3 : le centre de gravité est l'intégration réelle, pas l'unitaire.** 186 unit tests verts n'avaient rien empêché — au premier lancement du binaire, bundle frontend stale, session tmux morte à la naissance, Edit mode qui s'écrasait tout seul. Chaque slice avait validé son module en isolation, jamais l'effet utilisateur. La règle est écrite ici, **pas enforcée par CI** : elle dépend de la review humaine.

1. **Unit** (`#[cfg(test)]` inline) — logique pure : parser, condition evaluator, prompt augmenter, serializer.
2. **Intégration HTTP in-process** (`tower::ServiceExt::oneshot`) — contrats API.
3. a. **Cargo intégration daemon réel** (`crates/pdo-daemon/tests/`) — daemon sur port éphémère, vrais notify/sqlite/axum/WS/tmux. **Pas de mocking au-dessus de la couche 1** : un file watcher mocké aurait reproduit le bug parfaitement, sauf que le bug n'aurait pas existé. (`claude` est substitué par `bash -c 'sleep 60'` — il n'est pas en CI ; sa validation réelle vit en couche 5.)
   b. **Playwright** (`frontend/e2e/`) — coutures browser ↔ daemon.
4. **Smoke bash** (`tests/smoke.sh`) — pre-merge, gratuit.
5. **Scénarios agentiques** — un agent joue un parcours et rend des **findings** (bloquant / non-bloquant), parce qu'un bash teste des invariants techniques mais ne juge pas l'expérience (le DAG s'anime ? le terminal attaché reste vivant ?). **Happy Paths (HP)** : suite permanente, **≤ 3**, dans `docs/test-scenarios/` — gate `integration → develop`, décision humaine. **Feature Paths (FP)** : dans l'AC de la sous-issue, **jetables** — gate sous-issue → `integration` (auto-merge). Runner = `/agentic-tests`. **Hors CI** : coût API non négligeable, flakiness arbitrée cas par cas.

**La résilience n'est pas un Happy Path.** Les invariants d'adversité (mort de session, kill du daemon, fuite de slot d'admission, rejet d'édition mid-run, « jamais de stall silencieux ») sont coûteux à jouer et **couverts en permanence par la couche 3** — pas par un HP.
