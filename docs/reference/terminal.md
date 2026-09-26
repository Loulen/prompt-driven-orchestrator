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

## Shared terminal: one pilot, spectators

When two browsers show the same terminal (node, Manager, library assistant or Run shell), only one of them pilots it (#867, [ADR-0075](../adr/0075-un-terminal-partage-n-a-qu-un-pilote-qui-seul-pese-sur-la-taille.md)). This stops the flicker of a tmux window resized by each browser in turn.

| Role | Who | What it sees and can do |
| --- | --- | --- |
| Pilot | The first browser to open the terminal | Its window size sets the terminal size, and only its keystrokes reach the session. An eye with a number above the terminal shows how many other browsers are watching. |
| Spectator | Every other browser | Read-only: keystrokes are not sent, and hovering the terminal says so. It shows the pilot's whole screen at the pilot's columns × rows, with the font reduced to fit (below a readability floor, the area scrolls). It cannot lift a declared wait: on a node waiting for an answer, the banner says to take control to reply. |
| Solo | A browser alone on the terminal, or several tabs of one browser | Nothing changes and nothing is shown. Tabs of one browser share its identity (kept in `localStorage`) and all type and resize as before. |

A spectator **takes control** with the hand icon above the terminal (**Take control**), with no confirmation. It becomes the pilot: the terminal takes its window size, at the normal font, and its keystrokes reach the session. The former pilot becomes a spectator, reads **Another browser took control** for a few seconds, and gets the hand icon in turn. Control is held per terminal: taking one does not change your role on another.

When the pilot closes the terminal, the first browser to have arrived among the others takes over. If only one browser is left, it is solo again, with the normal size and font.

A direct `tmux attach` (over SSH) and **Detach to OS terminal** open ordinary tmux clients outside these roles: they still weigh on the window size and can type.

