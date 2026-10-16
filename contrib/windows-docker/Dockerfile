# PDO + GitHub Copilot CLI in a Linux container, for Windows hosts.
# See docs/recipes/windows-docker.md.
FROM debian:trixie-slim

ARG COPILOT_VERSION=1.0.98
ARG PDO_VERSION=1.126.0
ARG USER_UID=1000

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl git openssh-client tmux tini bash less procps jq xz-utils \
 && rm -rf /var/lib/apt/lists/*

# Optional corporate CA (TLS-intercepting proxy): drop *.crt files in certs/.
COPY certs/ /usr/local/share/ca-certificates/
RUN update-ca-certificates
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt \
    GIT_SSL_CAINFO=/etc/ssl/certs/ca-certificates.crt

# --- Add your project toolchain here (language runtime, linters...) ---

# Binaries go to /usr/local so the "home" volume never shadows a rebuilt version.
RUN curl -fsSL https://raw.githubusercontent.com/github/copilot-cli/main/install.sh \
      | VERSION="v${COPILOT_VERSION}" PREFIX=/usr/local bash \
 && copilot --version
RUN curl --proto '=https' --tlsv1.2 -LsSf \
      "https://github.com/Loulen/prompt-driven-orchestrator/releases/download/v${PDO_VERSION}/pdo-daemon-installer.sh" \
      | PDO_DAEMON_INSTALL_DIR=/usr/local/bin PDO_DAEMON_NO_MODIFY_PATH=1 sh \
 && pdo --version

COPY scripts/ /opt/pdo-docker/
# Safety net: strip CR in case scripts/ was checked out with CRLF line endings.
RUN sed -i 's/\r$//' /opt/pdo-docker/* && chmod 755 /opt/pdo-docker/*

RUN useradd -m -u "${USER_UID}" -s /bin/bash dev
USER dev
WORKDIR /home/dev
# Loopback stays off any proxy, or PDO's internal calls (`pdo complete`) would go through it.
# tmux and Copilot's interface need a UTF-8 locale and a 256-colour terminal.
# COPILOT_AUTO_UPDATE=false keeps the pinned version (an update would land in the home volume).
# PDO_DEFAULT_HARNESS: without it, runs created from the UI and child runs fall back to claude.
ENV NO_PROXY=localhost,127.0.0.1,::1 \
    no_proxy=localhost,127.0.0.1,::1 \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    TERM=xterm-256color \
    COPILOT_AUTO_UPDATE=false \
    PDO_DEFAULT_HARNESS=copilot

EXPOSE 5172
ENTRYPOINT ["/usr/bin/tini", "--", "/opt/pdo-docker/entrypoint.sh"]
