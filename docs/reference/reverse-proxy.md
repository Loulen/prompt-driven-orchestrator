# Reverse proxy

The daemon listens on `0.0.0.0:<port>` without authentication or TLS.

Put authentication and TLS in front of PDO before exposing it beyond a trusted network.

Behind a proxy on the same machine, start the daemon with `--bind 127.0.0.1`.

Set every public browser origin as an exact `scheme://host[:port]` value, for example
`PDO_ALLOWED_WS_ORIGINS="https://pdo.example.tld,https://pdo.internal:8443" pdo daemon`.

| Setting | Behavior |
| --- | --- |
| Default WebSocket origins | `localhost` and `127.0.0.1:<port>` |
| Extra WebSocket origins | Comma-separated `PDO_ALLOWED_WS_ORIGINS` values |
| WebSocket routes | `/ws` and `/sessions/<id>/pty` |
| TLS proxy | The UI uses `wss://` automatically |

Add `PDO_ALLOWED_WS_ORIGINS` to the service environment when the daemon runs as a service. Keep it
in a `pdo.service.d/override.conf` drop-in, which survives unit rewrites including Update (see
[`pdo service install`](cli.md#cli-commands)).

## Attachments through a proxy

A Run's attachments travel in one multipart `POST /runs`. The daemon's own budget is
`max_attachments_mb` in Settings (50 MB by default), but a reverse proxy checks the body size
first, and nginx refuses anything over 1 MB unless told otherwise (#839).

| Symptom | Meaning | Fix |
| --- | --- | --- |
| `The server in front of PDO … refused the request body (413)` | The proxy's body limit, not PDO's: the refusal carried no PDO error body | Raise `client_max_body_size` (nginx) or the equivalent, at or above `max_attachments_mb` |
| `attachments exceed the per-run limit of N MB` | PDO's own budget | Raise `max_attachments_mb` in Settings or attach less |
| `Upload interrupted … the network or a proxy in front of PDO cut the connection` | The connection dropped before the daemon answered | Check the proxy's timeouts (`client_body_timeout`, `proxy_read_timeout`) and the link |
| `Upload timed out after Ns` | Nothing answered within the upload budget (a minute plus ten seconds per MB) | Same as above; the CLI applies the same wait |
| `upload interrupted while reading attachment X` (400 from the daemon) | The body was cut mid-stream between the proxy and the daemon | Check the proxy's `proxy_request_buffering` / timeouts |

```nginx
location / {
    client_max_body_size 64m;   # ≥ max_attachments_mb
    proxy_pass http://127.0.0.1:5172;
}
```

## Terminal on a remote origin

If a terminal pane shows `disconnected` on a remote origin, the daemon rejected the WebSocket
origin: add it to `PDO_ALLOWED_WS_ORIGINS`. Copy and paste in the pane are covered in
[terminal.md](terminal.md).
