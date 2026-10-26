# Recipe — PDO on Windows through Docker

**Problem.** PDO runs its agents in **tmux** sessions and creates **git worktrees**: it needs a
Unix environment and ships for macOS and Linux only.

**What this recipe does.** The PDO daemon and an agent harness run in a Linux container under
Docker Desktop. Your repositories are cloned **inside the container** and you push from there.
You need Docker Desktop with Linux containers; no WSL commands and no admin rights.

**How to use it.** PDO ships no Docker files: this page describes what to build and why. Hand it
to a coding agent on the Windows machine ("set up PDO following this recipe, with harness X"), or
follow it yourself. The steps marked **[human]** need you: a browser login, a key on your Git
host, a certificate export. The example harness is GitHub Copilot CLI, the first one validated
this way; [Other harnesses](#other-harnesses) lists what changes for another one. If you test it
or adapt it, share what you found ([CONTRIBUTING.md](../../CONTRIBUTING.md)).

This is not the [Sandbox](../adr/0030-sandbox-execution-model.md): the Sandbox runs each Run in
its own container started by a daemon on a Unix host. Here the **daemon itself** runs in the
container, and every Run executes there.

## What to build

Put the files in a folder of their own, outside any repository (e.g. `%USERPROFILE%\pdo-docker`):
a `Dockerfile`, a `compose.yaml`, a `.env`, an `entrypoint.sh` and, for Copilot, a
`copilot-config.sh`.

**Line endings.** Every file the container executes must use LF. A file written with CRLF fails
with `/bin/bash^M`. Write them with LF, and have the `Dockerfile` strip CR from the scripts it
copies (`sed -i 's/\r$//'`) as a safety net. If the folder is versioned, add a `.gitattributes`
with `* text eol=lf`.

### Image (`Dockerfile`)

| Requirement | Why |
|---|---|
| A Debian (or Ubuntu) slim base | PDO's install script ships Linux x86_64 and ARM64 binaries |
| Packages: `ca-certificates curl git openssh-client tmux tini bash procps jq` (plus `less`, `xz-utils` if the harness installer needs them) | tmux and git are PDO's runtime requirements; `tini` reaps tmux's processes as PID 1; `jq` edits the harness config |
| An optional corporate CA: copy `certs/*.crt` into `/usr/local/share/ca-certificates/`, run `update-ca-certificates`, set `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS` and `GIT_SSL_CAINFO` to `/etc/ssl/certs/ca-certificates.crt` | Behind a TLS-intercepting proxy, every download fails otherwise (see [Corporate TLS proxy](#corporate-tls-proxy)). Keep `certs/` in the build context even when empty |
| The harness, installed **as root into `/usr/local/bin`** (Copilot: `curl -fsSL https://raw.githubusercontent.com/github/copilot-cli/main/install.sh \| PREFIX=/usr/local bash`) | The home directory is a volume: a binary installed there would shadow every rebuilt version |
| PDO, installed **into `/usr/local/bin`**: `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-daemon-installer.sh \| PDO_DAEMON_INSTALL_DIR=/usr/local/bin PDO_DAEMON_NO_MODIFY_PATH=1 sh` | Same reason. Replace `latest` with a tag (`v1.126.0`) to pin a release |
| Your project's toolchain (language runtime, build tool, linters) | Agents run your build and tests inside the container |
| A non-root user (e.g. `dev`, UID 1000) with a home directory; `USER dev` | Agents should not run as root |
| `ENV LANG=C.UTF-8 LC_ALL=C.UTF-8 TERM=xterm-256color` | tmux and the harness's interface need a UTF-8 locale and a 256-colour terminal |
| `ENV NO_PROXY=localhost,127.0.0.1,::1` and `no_proxy` (same value) | PDO's internal calls (`pdo complete`) must never go through a proxy |
| `ENV PDO_DEFAULT_HARNESS=<harness>` (e.g. `copilot`) | Without it, runs created from the UI and child runs of an orchestrator fall back to `claude`. A default harness chosen in Settings takes precedence |
| Turn off the harness's self-update (Copilot: `ENV COPILOT_AUTO_UPDATE=false`) | An update lands in the home volume and survives a rebuild |
| `EXPOSE 5172`; `ENTRYPOINT ["/usr/bin/tini", "--", "/opt/pdo-docker/entrypoint.sh"]` | |

Run `pdo --version` and `<harness> --version` at the end of their install steps so a broken
download fails the build.

### Service (`compose.yaml` and `.env`)

- One service, `container_name: pdo`, built from the folder, `restart: unless-stopped`.
- Port `127.0.0.1:5172:5172`: the UI is reachable from Windows only.
- One named volume mounted on the whole home (`home:/home/dev`): it holds the clones (`~/work`),
  PDO's state (`~/.pdo`), the harness login, the git and SSH config. Do not bind-mount a Windows
  folder (see [Why the repositories live in the container](#why-the-repositories-live-in-the-container)).
- `GIT_USER_NAME` and `GIT_USER_EMAIL` passed from `.env` into the environment. Keep `.env` out of
  any repository.

### Startup (`entrypoint.sh`)

Each start, with `set -euo pipefail`:

1. Set `git config --global user.name` / `user.email` from the environment when they are set.
2. Create `~/work` and add `/.pdo/*` and `!/.pdo/pipelines/` to the global git exclude
   (`~/.config/git/ignore`) if missing: PDO's runtime files stay out of every clone, while a
   repository can still version its pipelines in `.pdo/pipelines/`.
3. Apply the harness config (Copilot: run `copilot-config.sh`), **tolerating failure**: a broken
   harness config must not keep the daemon from starting, or the container enters a restart
   loop.
4. `exec pdo daemon --bind 0.0.0.0 --port 5172`. Binding every interface is needed inside the
   container; the host side stays on `127.0.0.1` through the port mapping.

### Copilot config (`copilot-config.sh`)

Copilot asks to trust each folder it works in, which blocks a node. The script:

- adds `~/work` and `~/.pdo` to `trustedFolders` in `~/.copilot/config.json` (trust cascades to
  subfolders; PDO creates the Run worktrees under `~/.pdo`), without duplicates;
- sets `autoUpdate: false` in `~/.copilot/settings.json`;
- creates either file as `{}` when missing, and keeps them `chmod 600`.

Copilot writes these files as JSONC: drop the `//` comment lines before passing them to `jq`.
The login rewrites `config.json`, so the script runs again after `copilot login`.

## Why the repositories live in the container

Bind-mounting a Windows folder looks simpler, but PDO creates a worktree per Run and watches
files: on a Windows mount, file I/O goes through Docker Desktop's file sharing, which is slower
than the container's own filesystem. On managed machines, Docker Desktop's *Enhanced Container
Isolation* may also reject bind mounts outright (`ID-mapped mount … function not implemented`).
To edit or browse the code from Windows, attach VS Code to the running container (*Dev
Containers: Attach to Running Container*) or use PDO's diff review.

## First start

1. **Check the prerequisites.** `docker version` must answer and show `OS/Arch: linux/…` on the
   server side; otherwise Docker Desktop is stopped or set to Windows containers.
2. **Build and start.**

   ```powershell
   cd $env:USERPROFILE\pdo-docker
   docker compose up -d --build
   docker logs pdo     # the daemon must be listening on 5172
   ```

   A `certificate signed by unknown authority` error means a TLS proxy: see
   [Corporate TLS proxy](#corporate-tls-proxy).
3. **[human] Log in with the harness.**

   ```powershell
   docker exec -it pdo copilot login --device-code
   docker exec pdo /opt/pdo-docker/copilot-config.sh   # re-apply trust after login
   ```

   Use the device flow: the web flow calls back to the container's `127.0.0.1`, which Windows
   cannot reach. Open `https://github.com/login/device` on Windows and enter the code. The
   container has no keyring: when asked whether to store the token in plain text, answer `y`. It
   lands in the `home` volume.
4. **[human] Give the container git credentials**, then clone under `~/work`:

   ```powershell
   docker exec -it pdo bash
   ```

   ```bash
   ssh-keygen -t ed25519 -C "pdo container"    # add ~/.ssh/id_ed25519.pub to your Git host
   cd ~/work && git clone git@github.com:<org>/<repo>.git
   ```

   HTTPS with a personal access token works too (`git config --global credential.helper store`).
   Agents run with their permission prompts bypassed: they can use these credentials, and
   Copilot's built-in GitHub MCP server acts with your Copilot login. Scope both accounts to what
   PDO needs.

### Check the installation

The setup is done when all of these hold:

| Check | Expected |
|---|---|
| `docker exec pdo pdo --version` and `docker exec pdo copilot --version` | both print a version |
| `curl.exe -s -o NUL -w "%{http_code}" http://localhost:5172` from PowerShell | `200` |
| `docker exec pdo git -C /home/dev/work/<repo> fetch` | no error, no credential prompt |
| A Run on the cloned repository, started from the UI | its node starts, its live terminal connects, the agent works without a trust dialog, and the node completes |

## Daily use

- **Web UI**: http://localhost:5172.
- **Create a run from the CLI** (inside the container):

  ```bash
  pdo run create <pipeline> --target-repo /home/dev/work/<repo> --source-branch <branch> --input "..."
  ```
- **Shell**: `docker exec -it pdo bash`.
- **Update**: rebuild the image. The in-app **Update** button does not apply here: the binaries
  sit in `/usr/local/bin`, owned by root. With `latest` URLs, Docker caches the install layers, so
  force them: `docker compose build --pull --no-cache; docker compose up -d`. The `home` volume is
  kept. With a pinned tag, bump it and rebuild.
- **Stop / reset**: `docker compose stop` keeps everything, `docker compose down` removes the
  container and keeps the volume, `docker compose down -v` also deletes clones, PDO state and the
  harness login. With `restart: unless-stopped`, the container restarts with Docker Desktop.

## Other harnesses

For Claude Code, OpenCode or pi, the structure stays the same. Adapt:

- the install step in the `Dockerfile`, still into `/usr/local/bin` (or a root-owned prefix on
  the `PATH`), with the harness's self-update turned off;
- `PDO_DEFAULT_HARNESS`;
- the login: run it with `docker exec -it pdo <harness> …` and choose a flow that does not need a
  callback to the container's `127.0.0.1` (device code, or a code you paste back);
- the trust step: approve `~/work` and `~/.pdo` the way that harness expects, so no node stops on
  a trust dialog ([harnesses.md](../reference/harnesses.md), *Prerequisites*);
- the entrypoint's config step, if the harness needs one.

The support table in [harnesses.md](../reference/harnesses.md) lists the last harness versions
validated by PDO's test suite.

## Corporate TLS proxy

If `curl` or `git` fails during the build with `certificate signed by unknown authority`, a
proxy is intercepting TLS. **[human]** Export its root and intermediate CAs from the Windows
certificate store (reading it needs no admin rights) into `certs/` as PEM files ending in `.crt`,
then run `docker compose build`:

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
| Live terminals in the UI don't connect | The UI is opened on an origin other than `localhost:5172` / `127.0.0.1:5172` (another host port, a hostname): add that origin to `PDO_ALLOWED_WS_ORIGINS` in the service environment, then `docker compose up -d`. |
| The harness is no longer authenticated | Log in again with `docker exec -it pdo <harness> login …`. |
| A node stops on a trust dialog | Re-run the trust step (Copilot: `docker exec pdo /opt/pdo-docker/copilot-config.sh`). |
| `x509: certificate signed by unknown authority` | Missing or changed proxy CA: re-export it, then `docker compose build`. |
| Scripts fail with `/bin/bash^M` | CRLF line endings: convert the scripts to LF and rebuild. |
| The harness config script fails in the logs | Its JSON file is invalid: fix or delete it, then `docker compose restart`. The daemon still starts. |
| TLS warning from PDO's update checker in the logs | Harmless behind a proxy: updates go through a rebuild. |
