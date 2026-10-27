#!/bin/sh
# Installs Enco from GitHub Releases: the latest release, or nightly until there is one.
#   curl -fsSL https://install.enco.sh | sh
#   curl -fsSL https://install.enco.sh | sh -s -- --nightly
set -eu

REPO="${ENCO_REPO:-Darkatse/Enco}"
INSTALL_DIR="${ENCO_INSTALL_DIR:-}"
VERSION=""
NIGHTLY=0
BINARY="enco"

usage() {
  cat <<'EOF'
Install Enco from GitHub Releases.

Usage:
  install.sh [--nightly] [--version <version>] [--dir <path>] [--repo <owner/name>]

By default this installs the latest release, or the nightly build while there is no release yet.
EOF
}

say() {
  printf '%s\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "need sha256sum or shasum to verify the download"
  fi
}

exists() {
  curl -fsSIL "$1" >/dev/null 2>&1
}

asset_name() {
  case "$(uname -s)" in
    Linux) os="linux" ;;
    Darwin) os="macos" ;;
    *) die "unsupported operating system: $(uname -s); Enco runs on Linux and macOS" ;;
  esac
  case "$(uname -m)" in
    x86_64 | amd64) arch="x64" ;;
    arm64 | aarch64) arch="arm64" ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
  printf '%s-%s-%s' "$BINARY" "$os" "$arch"
}

install_dir() {
  if [ -n "$INSTALL_DIR" ]; then
    printf '%s' "$INSTALL_DIR"
  elif [ "$(id -u)" -eq 0 ] || { [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; }; then
    printf '%s' /usr/local/bin
  else
    [ -n "${HOME:-}" ] || die "HOME is not set; pass --dir"
    printf '%s' "$HOME/.local/bin"
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --nightly) NIGHTLY=1; shift ;;
    --version)
      [ "$#" -ge 2 ] || die "--version needs a value"
      VERSION="$2"; shift 2 ;;
    --dir)
      [ "$#" -ge 2 ] || die "--dir needs a value"
      INSTALL_DIR="$2"; shift 2 ;;
    --repo)
      [ "$#" -ge 2 ] || die "--repo needs a value"
      REPO="$2"; shift 2 ;;
    -h | --help) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

case "$VERSION" in
  "" | v* | nightly) ;;
  *) VERSION="v$VERSION" ;;
esac
[ -z "$VERSION" ] || [ "$NIGHTLY" -eq 0 ] || die "--nightly and --version cannot be used together"

for command in curl uname mktemp awk chmod cp mkdir id; do
  need "$command"
done

ASSET="$(asset_name)"
DIR="$(install_dir)"
RELEASES="https://github.com/$REPO/releases"
if [ -n "$VERSION" ]; then
  LABEL="release $VERSION"
  BASE="$RELEASES/download/$VERSION"
elif [ "$NIGHTLY" -eq 0 ] && exists "$RELEASES/latest/download/$ASSET" && exists "$RELEASES/latest/download/SHA256SUMS.txt"; then
  LABEL="the latest release"
  BASE="$RELEASES/latest/download"
else
  LABEL="nightly"
  BASE="$RELEASES/download/nightly"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

say "Installing Enco from $LABEL"
curl -fsSL "$BASE/$ASSET" -o "$TMP/$ASSET" || die "failed to download $ASSET"
curl -fsSL "$BASE/SHA256SUMS.txt" -o "$TMP/SHA256SUMS.txt" || die "failed to download SHA256SUMS.txt"

EXPECTED="$(awk -v file="$ASSET" '$2 == file { print $1 }' "$TMP/SHA256SUMS.txt")"
[ -n "$EXPECTED" ] || die "no checksum for $ASSET in SHA256SUMS.txt"
[ "$(sha256 "$TMP/$ASSET")" = "$EXPECTED" ] || die "checksum mismatch for $ASSET"

mkdir -p "$DIR"
cp "$TMP/$ASSET" "$DIR/$BINARY"
chmod 755 "$DIR/$BINARY"

say "Installed $("$DIR/$BINARY" --version) to $DIR/$BINARY"
case ":${PATH:-}:" in
  *":$DIR:"*) ;;
  *) say "Add $DIR to PATH to run enco from anywhere." ;;
esac
say ""
say "Next: run 'enco init', edit the config it creates, then 'enco serve'."
say "Enco builds plugins with Rust. To let it write or change its own plugins, install rustup"
say "(https://rustup.rs); Enco names the toolchain it needs the first time it builds one."
