# Terminal inline xterm.js plutôt que spawn OS natif

> Amendé par ADR-0075 : entre deux postes, un terminal partagé a un seul pilote (le multi-client n'est plus « gratuit »).

**Le terminal d'une session tmux vit inline dans l'UI (xterm.js + PTY côté daemon bridgé en WebSocket) ; ni polling `capture-pane`, ni fenêtre OS comme mécanisme principal.** Le spawn d'un terminal OS reste une **échappatoire** derrière l'icône détacher (copy/paste exotique, debug d'un freeze WebSocket), jamais le chemin par défaut : sortir de l'app pour intervenir, c'est moins d'interventions, et *Deliberate over autonomous* suppose l'inverse. Ça supprime aussi un détecteur de terminal cross-platform fragile par construction.

**Garde de sécurité non dérivable du code.** Le daemon bind `0.0.0.0` (#260), donc l'**origin check** — pas le bind — est la seule protection contre le DNS-rebinding / CSWSH. Allowlist explicite, défaut loopback, extensible par configuration (`PDO_ALLOWED_WS_ORIGINS`, #564), et elle doit couvrir **les deux** routes WS : le PTY et le flux d'événements.

Le multi-client par session repose sur tmux ; entre deux postes, un seul pilote pèse sur la taille et tape (ADR-0075).
