#!/bin/sh
# Installs Kossé from its GitHub Releases into <prefix>/bin (default ~/.local/bin).
#
#   curl -fsSL https://github.com/davidpp/kosse/releases/latest/download/install.sh | sh
#   curl -fsSL …/install.sh | sh -s -- --version 0.2.0 --prefix /opt/kosse
#
# Downloads the archive and its .sha256 with curl (no quarantine attribute, so the unsigned binary
# runs), refuses it unless the SHA-256 matches, and also verifies its build provenance with
# `gh attestation verify` when an authenticated GitHub CLI is available. Changes nothing outside
# <prefix>/bin/kosse: login startup stays off until `kosse startup enable`.
set -eu

repo="davidpp/kosse"
target="aarch64-apple-darwin"
prefix="${HOME}/.local"
version=""

fail() {
  printf 'kosse install: %s\n' "$1" >&2
  exit 1
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || fail "--version needs a value"; version="${2#v}"; shift 2 ;;
    --prefix) [ $# -ge 2 ] || fail "--prefix needs a value"; prefix="$2"; shift 2 ;;
    -h | --help)
      printf 'usage: install.sh [--version X.Y.Z] [--prefix DIR]\n'
      exit 0
      ;;
    *) fail "unknown argument: $1" ;;
  esac
done

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] ||
  fail "Kossé releases are built for macOS on Apple silicon only"

if [ -z "$version" ]; then
  # /releases/latest redirects to /releases/tag/vX.Y.Z.
  latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${repo}/releases/latest") ||
    fail "could not resolve the latest release"
  version="${latest##*/v}"
fi
case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) fail "not a release version: ${version}" ;;
esac

name="kosse-${version}-${target}"
base="https://github.com/${repo}/releases/download/v${version}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

printf 'Downloading Kossé %s…\n' "$version"
curl -fsSL -o "${work}/${name}.tar.gz" "${base}/${name}.tar.gz" || fail "download failed: ${base}/${name}.tar.gz"
curl -fsSL -o "${work}/${name}.tar.gz.sha256" "${base}/${name}.tar.gz.sha256" || fail "checksum download failed"
(cd "$work" && shasum -a 256 -c "${name}.tar.gz.sha256" >/dev/null) ||
  fail "SHA-256 mismatch: the archive is not the one the release published"
printf 'SHA-256 verified.\n'

if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  gh attestation verify "${work}/${name}.tar.gz" --repo "$repo" >/dev/null ||
    fail "build provenance verification failed (gh attestation verify)"
  printf 'Build provenance verified: built by %s GitHub Actions.\n' "$repo"
else
  printf 'Skipped provenance check (needs an authenticated gh): gh attestation verify %s.tar.gz --repo %s\n' "$name" "$repo"
fi

tar -xzf "${work}/${name}.tar.gz" -C "$work"
mkdir -p "${prefix}/bin"
install -m 0755 "${work}/${name}/kosse" "${prefix}/bin/kosse"
installed=$("${prefix}/bin/kosse" --version) || fail "the installed kosse does not run"
[ "$installed" = "kosse ${version}" ] || fail "the installed binary reports ${installed}"
printf 'Installed %s at %s/bin/kosse\n' "$installed" "$prefix"

case ":${PATH}:" in
  *":${prefix}/bin:"*) ;;
  *) printf '\n%s/bin is not on your PATH. Add it, then open a new terminal:\n  echo '\''export PATH="%s/bin:$PATH"'\'' >> ~/.zprofile\n' "$prefix" "$prefix" ;;
esac
printf '\nStart the menu-bar app with `kosse` (quit a running one first after an upgrade).\nOptional: `kosse startup enable` starts it at each login; `kosse doctor` checks the setup.\n'
