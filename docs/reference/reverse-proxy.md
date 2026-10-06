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

## WebSockets through a proxy

The UI keeps two kinds of WebSocket open: `/ws` (live Run updates) and `/sessions/<id>/pty` (one
per node terminal). The proxy must forward the HTTP upgrade and must not close a quiet socket.

| Requirement | Why |
| --- | --- |
| Forward `Upgrade` and `Connection: upgrade` over HTTP/1.1 | Without them the upgrade fails and the terminal stays `disconnected` |
| Read timeout above 60 s (`proxy_read_timeout` in nginx) | The daemon sends a heartbeat on `/ws` every 5 s and pings each terminal socket every 25 s; a shorter timeout cuts a terminal whose agent is thinking |
| No response buffering on these routes | Buffered frames reach the browser late, or in bursts |

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    ''      close;
}

server {
    # … listen, server_name, TLS, authentication …

    location / {
        proxy_pass http://127.0.0.1:5172;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_set_header Host $host;
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
        proxy_buffering off;
        client_max_body_size 64m;   # ≥ max_attachments_mb, see below
    }
}
```

When a socket drops anyway (network cut, proxy restart, laptop asleep), the UI recovers on its own:

- the live stream reconnects with an increasing delay (1 s, 2 s, 4 s … up to 30 s), treats 15 s
  without a heartbeat as a dead socket, and re-reads every Run after each reconnection and each
  return to the tab;
- a node terminal whose socket closed shows `Terminal connection closed` with a **Reconnect**
  button, and reconnects by itself when the tab becomes visible again.

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
origin: add it to `PDO_ALLOWED_WS_ORIGINS`. If it connects and then shows `Terminal connection
closed` after a while, the proxy closed it: check the upgrade headers and the read timeout in
[WebSockets through a proxy](#websockets-through-a-proxy). Copy and paste in the pane are covered in
[terminal.md](terminal.md).
