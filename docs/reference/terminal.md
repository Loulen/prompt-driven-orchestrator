# Terminal pane

Every node session, shell and manager runs in tmux; the web terminal pane attaches to it, so you
can watch an agent, type into its session, or take over.

## Copy and paste in the terminal pane

Selection happens in the browser, so copy and paste work over plain `http://<host>:<port>` too: no https or `localhost` is required (#772).

| Action | How |
| --- | --- |
| Select | Drag with the mouse (Shift+drag and Option+drag on macOS also work) |
| Copy | Ctrl+Shift+C, Ctrl+C while text is selected, or the copy button in the pane toolbar |
| Paste | Ctrl+Shift+V, Ctrl+V, or right-click |
| Scroll tmux scrollback | Mouse wheel (forwarded to tmux) |

Ctrl+C without a selection still interrupts the program in the pane. On macOS use Cmd instead of Ctrl.

If the pane shows `disconnected` on a remote origin, the daemon rejected the WebSocket origin: add it to `PDO_ALLOWED_WS_ORIGINS` (see [reverse-proxy.md](reverse-proxy.md)).
