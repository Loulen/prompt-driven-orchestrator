#!/usr/bin/env bash
set -euo pipefail

REPO="Loulen/prompt-driven-orchestrator"
INSTALL_DIR="${PDO_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${PDO_VERSION:-latest}"

info() { printf '  \033[1;32m%s\033[0m %s\n' "$1" "$2"; }
err()  { printf '  \033[1;31merror:\033[0m %s\n' "$1" >&2; exit 1; }

detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)  os="linux" ;;
    Darwin) os="macos" ;;
    *)      err "Unsupported OS: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64)   arch="x86_64" ;;
    aarch64|arm64)   arch="aarch64" ;;
    *)               err "Unsupported architecture: $arch" ;;
  esac

  PLATFORM="${os}-${arch}"
}

resolve_version() {
  if [ "$VERSION" = "latest" ]; then
    VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')" \
      || err "Failed to fetch latest release version"
    [ -n "$VERSION" ] || err "Could not determine latest version"
  fi
}

download_and_verify() {
  local base_url="https://github.com/${REPO}/releases/download/${VERSION}"
  local archive="pdo-${VERSION}-${PLATFORM}.tar.gz"
  local checksum_file="pdo-${VERSION}-SHA256SUMS.txt"

  local tmpdir
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT

  info "Downloading" "${archive}"
  curl -fsSL "${base_url}/${archive}" -o "${tmpdir}/${archive}" \
    || err "Failed to download ${archive}"

  info "Downloading" "checksums"
  curl -fsSL "${base_url}/${checksum_file}" -o "${tmpdir}/${checksum_file}" \
    || err "Failed to download checksum file"

  info "Verifying" "SHA256 checksum"
  local expected
  expected="$(grep -F "${archive}" "${tmpdir}/${checksum_file}" | awk '{print $1}')"
  [ -n "$expected" ] || err "No checksum found for ${archive}"

  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "${tmpdir}/${archive}" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "${tmpdir}/${archive}" | awk '{print $1}')"
  else
    err "No sha256sum or shasum found — cannot verify download"
  fi

  if [ "$expected" != "$actual" ]; then
    err "Checksum mismatch: expected ${expected}, got ${actual}"
  fi

  info "Extracting" "to ${INSTALL_DIR}"
  mkdir -p "$INSTALL_DIR"
  tar -xzf "${tmpdir}/${archive}" -C "${tmpdir}"
  install -m 755 "${tmpdir}/pdo" "${INSTALL_DIR}/pdo"
}

check_path() {
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      printf '\n'
      info "Warning" "${INSTALL_DIR} is not in your PATH"
      printf '  Add it by appending this to your shell profile (~/.bashrc, ~/.zshrc, etc.):\n'
      printf '\n    export PATH="%s:$PATH"\n\n' "$INSTALL_DIR"
      ;;
  esac
}

main() {
  info "Installing" "PDO"
  detect_platform
  resolve_version
  info "Platform" "${PLATFORM}"
  info "Version" "${VERSION}"
  download_and_verify
  check_path

  info "Verifying" "installation"
  if "${INSTALL_DIR}/pdo" --version >/dev/null 2>&1; then
    local ver
    ver="$("${INSTALL_DIR}/pdo" --version 2>&1)"
    info "Installed" "${ver}"
  else
    err "Installation verification failed — pdo binary did not run"
  fi
}

main
