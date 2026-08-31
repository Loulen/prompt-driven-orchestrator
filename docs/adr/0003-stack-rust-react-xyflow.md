# Stack Rust + React + xyflow

**Le daemon est en Rust et le restera : pas de daemon ni de sidecar JS/TypeScript**, parce que la charge — file-watch sur N pipelines, supervision tmux/git de M NodeRuns, hot-path SQLite à chaque transition d'état — est typiquement où les stacks JS dégradent. Frontend en React + Vite + xyflow (lib dominante des éditeurs DAG) + shadcn/ui.

Contraintes de distribution qui survivent au code : binaires GitHub Releases + install script `curl | bash`, frontend **embarqué dans le binaire** (rust-embed), **pas de npm** chez l'utilisateur. Tauri envisagé en v2 pour wrapper la même UI en app desktop.

Pas d'opinion forte sur shadcn vs une autre lib UI — on peut switcher.
