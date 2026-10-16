# Recipe — PDO + GitHub Copilot CLI on Windows (Docker)

**Problem.** PDO runs its agents in **tmux** sessions and creates **git worktrees**: it needs a
Unix environment and ships for macOS and Linux only.

**What this recipe does.** The PDO daemon and GitHub Copilot CLI run in a Linux container under
Docker Desktop. Your repositories are cloned **inside the container** and you push from there.
You only need Docker Desktop (Linux containers), no WSL commands and no admin rights.

The files live in [`contrib/windows-docker/`](../../contrib/windows-docker/): copy the folder,
edit nothing but `.env`.

**An example, not the only way.** This setup uses Copilot because that is what it was first
validated with. For another harness such as Claude Code, install it in the `Dockerfile`, log in
with it inside the container, trust `~/work` and `~/.pdo` the way that harness expects, and set
`PDO_DEFAULT_HARNESS` accordingly. If you test it on your machine or adapt it, share what you
found or contribute your variant ([CONTRIBUTING.md](../../CONTRIBUTING.md)).

> Pinned to PDO 1.126.0 and Copilot CLI 1.0.98. The image is built and smoke-tested on Linux;
> the end-to-end Windows validation (login, Runs) was done on an earlier, more locked-down
> variant of this setup. The harness support table ([harnesses.md](../reference/harnesses.md))
> lists the last Copilot version validated by the PDO test suite, which may be older.

This is not the [Sandbox](../adr/0030-sandbox-execution-model.md): the Sandbox runs each Run in
its own container started by a daemon on a Unix host. Here the **daemon itself** runs in the
container, and every Run executes there.

## Files

| File | Role |
|---|---|
| `Dockerfile` | Debian + git, tmux, tini, ssh client; Copilot CLI and PDO at pinned versions in `/usr/local/bin`; optional corporate CA |
| `compose.yaml` | the `pdo` service, UI published on `127.0.0.1:5172`, one `home` volume |
| `.env.example` | Git identity and pinned versions |
| `scripts/entrypoint.sh` | Git identity, a global git exclude for PDO's runtime dir (`/.pdo/*` except `.pdo/pipelines/`), Copilot config, then `pdo daemon` |
| `scripts/copilot-config.sh` | trusts `~/work` (cascades to every clone below) and `~/.pdo`, where PDO creates run worktrees; sets `autoUpdate: false` |

The image also sets:

- `PDO_DEFAULT_HARNESS=copilot`: without it, runs created from the UI and child runs of an
  orchestrator fall back to `claude`, which is not installed. A default harness chosen in
  Settings takes precedence.
- `COPILOT_AUTO_UPDATE=false` (plus `autoUpdate: false`): Copilot stays at the pinned version.
  An update would otherwise land in the persistent `home` volume and survive a rebuild.
- `LANG`/`LC_ALL=C.UTF-8` and `TERM=xterm-256color`, which tmux and Copilot's interface need.
- `NO_PROXY` covering loopback, so PDO's internal calls (`pdo complete`) never go through a
  proxy.

`.gitattributes` keeps `scripts/` and the `Dockerfile` in LF even with `core.autocrlf=true`, so
the scripts don't fail with `/bin/bash^M`. As a safety net, the Dockerfile also strips CR.

Add your project's toolchain (language runtime, linters…) to the `Dockerfile` where marked:
agents run your build and tests inside the container.

## Why the repo lives in the container

Bind-mounting a Windows folder looks simpler, but PDO creates a worktree per Run and watches
files: on a Windows mount, file I/O goes through Docker Desktop's file sharing, which is slower
than the container's own filesystem. On managed machines, Docker Desktop's *Enhanced Container
Isolation* may also reject bind mounts outright (`ID-mapped mount … function not implemented`).
Keep the clone in the `home` volume. To edit or browse the
code from Windows, attach VS Code to the running container (*Dev Containers: Attach to Running
Container*) or use PDO's diff review.

## First start

```powershell
cd <path>\windows-docker
Copy-Item .env.example .env    # then fill in .env
docker compose up -d --build   # build the image and start the daemon
docker exec -it pdo copilot login --device-code
docker exec pdo /opt/pdo-docker/copilot-config.sh   # re-apply trust after login
```

- **Device flow is required**: the web flow calls back to the container's `127.0.0.1`, which
  Windows cannot reach. Open `https://github.com/login/device` on Windows and enter the code.
- The container has no keyring: when Copilot asks whether to store the token in plain text,
  answer `y`. It lands in the `home` volume.

Then give the container git credentials and clone your repository under `~/work`:

```powershell
docker exec -it pdo bash
```

```bash
ssh-keygen -t ed25519 -C "pdo container"    # then add ~/.ssh/id_ed25519.pub to your Git host
cd ~/work && git clone git@github.com:<org>/<repo>.git
```

HTTPS with a personal access token works too (`git config --global credential.helper store`).
Agents run with `--allow-all`: they can use these credentials, and Copilot's built-in GitHub
MCP server acts with your Copilot login. Scope both accounts to what PDO needs.

## Daily use

- **Web UI**: http://localhost:5172.
- **Create a run from the CLI** (inside the container):

  ```bash
  pdo run create <pipeline> --target-repo /home/dev/work/<repo> --source-branch <branch> --input "..."
  ```
- **Shell**: `docker exec -it pdo bash`.
- **Update**: bump the versions in `.env`, then
  `docker compose build --pull; docker compose up -d`. The `home` volume is kept.
- **Stop / reset**: `docker compose stop` keeps everything, `docker compose down` removes the
  container and keeps the volume, `docker compose down -v` also deletes clones, PDO state and the
  Copilot login. With `restart: unless-stopped`, the container restarts with Docker Desktop.

## Corporate TLS proxy

If `curl` or `git` fails during the build with `certificate signed by unknown authority`, a
proxy is intercepting TLS. Export its root and intermediate CAs from the Windows certificate
store (reading it needs no admin rights) into `certs/` as PEM files ending in `.crt`
(git-ignored), then run `docker compose build`:

```powershell
# find the proxy's certificates
Get-ChildItem Cert:\LocalMachine\Root, Cert:\LocalMachine\CA |
  Where-Object Subject -match '<proxy vendor>' | Select-Object PSParentPath, Thumbprint, Subject

# export one as PEM with LF line endings
$c = Get-Item Cert:\LocalMachine\Root\<thumbprint>
"-----BEGIN CERTIFICATE-----`n" + [Convert]::ToBase64String($c.RawData, 'InsertLineBreaks').Replace("`r`n","`n") + "`n-----END CERTIFICATE-----`n" |
  Set-Content -NoNewline -Encoding ascii certs\proxy-root.crt
```

Never disable TLS verification instead.

## Troubleshooting

| Symptom | Fix |
|---|---|
| Live terminals in the UI don't connect | Set `PDO_ALLOWED_WS_ORIGINS=http://localhost:5172,http://127.0.0.1:5172` in `.env`, then `docker compose up -d`. |
| `copilot` no longer authenticated | `docker exec -it pdo copilot login --device-code` again. |
| `x509: certificate signed by unknown authority` | Missing or changed proxy CA: re-export it, then `docker compose build`. |
| Scripts fail with `/bin/bash^M` | CRLF line endings: convert `scripts/*` to LF. |
| `entrypoint: copilot-config failed` in the logs | `~/.copilot/config.json` is invalid JSON: fix or delete it, then `docker compose restart`. The daemon still starts. |
| TLS warning from PDO's update checker in the logs | Harmless behind a proxy: updates go through a rebuild. |
