# Stack Rust + React + xyflow

PDO nécessite un daemon long-running qui supervise plusieurs sessions tmux + sous-worktrees git, watch des fichiers, expose un serveur HTTP + WebSocket, persiste un event log SQLite, et un frontend visuel pour l'éditeur DAG et la viz de Runs.

**Décision : daemon en Rust (Tokio + Axum + sqlx + notify + serde_yaml), frontend en React + Vite + xyflow + shadcn/ui.** Distribution v1 = binaires GitHub Releases + install script `curl | bash`, frontend embarqué dans le binaire (rust-embed). Le daemon expose `localhost:<port>` que le user ouvre dans son browser. Pas de npm. Tauri envisagé en v2 pour wrapper la même UI en app desktop.

**Pourquoi.** Choisi contre l'alternative *"TypeScript / Bun pour le daemon"* parce que la charge — file-watch sur N pipelines, supervision tmux/git de M NodeRuns, hot-path SQLite à chaque transition d'état — est typiquement où les stacks JS dégradent (latence GC, single-thread event loop, libs systèmes). L'utilisateur a une expérience Rust suffisante pour absorber le surcoût de vélocité. xyflow choisi parce que c'est la lib dominante 2025+ pour des éditeurs DAG (Langflow, n8n, Dify l'utilisent), avec un support natif de custom nodes/edges et d'interactions clavier/souris matures. Pas d'opinion forte sur shadcn vs autre lib UI — on peut switcher.
