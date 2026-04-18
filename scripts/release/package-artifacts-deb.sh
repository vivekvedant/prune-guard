#!/usr/bin/env bash
set -euo pipefail

# Package Linux release outputs into a deterministic .deb artifact.
# The script is fail-closed: any missing input or metadata aborts packaging.

workspace_root="$(pwd)"
output_dir=""

usage() {
  cat <<'EOF'
Usage: package-artifacts-deb.sh [--workspace-root PATH] [--output-dir PATH]
EOF
}

while (($# > 0)); do
  case "$1" in
    --workspace-root)
      workspace_root="${2:?missing value for --workspace-root}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing value for --output-dir}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [ -z "$output_dir" ]; then
  output_dir="${workspace_root}/dist"
fi

if [ "$(uname -s)" != "Linux" ]; then
  echo "Debian packaging is only supported on Linux hosts." >&2
  exit 1
fi

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "dpkg-deb is required for Debian packaging." >&2
  exit 1
fi

if ! command -v dpkg >/dev/null 2>&1; then
  echo "dpkg is required to resolve Debian architecture." >&2
  exit 1
fi

release_dir="${workspace_root}/target/release"
if [ ! -d "$release_dir" ]; then
  echo "Release directory missing: $release_dir" >&2
  exit 1
fi

first_release_file="$(find "$release_dir" -type f -size +0c -print -quit)"
if [ -z "$first_release_file" ]; then
  echo "Release directory does not contain any non-empty files." >&2
  exit 1
fi

config_template="${workspace_root}/config/prune-guard.toml"
if [ ! -s "$config_template" ]; then
  echo "Install config template missing: $config_template" >&2
  exit 1
fi

service_template="${workspace_root}/packaging/systemd/prune-guard.service"
if [ ! -s "$service_template" ]; then
  echo "Systemd service template missing: $service_template" >&2
  exit 1
fi

timer_template="${workspace_root}/packaging/systemd/prune-guard.timer"
if [ ! -s "$timer_template" ]; then
  echo "Systemd timer template missing: $timer_template" >&2
  exit 1
fi

daemon_binary="${release_dir}/prune-guard"
if [ ! -f "$daemon_binary" ]; then
  echo "Daemon binary missing: $daemon_binary" >&2
  exit 1
fi

version="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "${workspace_root}/Cargo.toml" | head -n1)"
if [ -z "$version" ]; then
  echo "Could not resolve package version from Cargo.toml." >&2
  exit 1
fi

deb_arch="$(dpkg --print-architecture | tr -d '\n')"
if [ -z "$deb_arch" ]; then
  echo "Could not resolve Debian architecture." >&2
  exit 1
fi

mkdir -p "$output_dir"
staging_root="$(mktemp -d "${TMPDIR:-/tmp}/prune-guard-deb.XXXXXX")"
trap 'rm -rf "$staging_root"' EXIT

deb_root="${staging_root}/prune-guard_${version}_${deb_arch}"
debian_dir="${deb_root}/DEBIAN"
config_dir="${deb_root}/etc/prune-guard"
doc_dir="${deb_root}/usr/share/doc/prune-guard"
installed_daemon_path="/usr/bin/prune-guard"
installed_systemd_unit_path="/lib/systemd/system/prune-guard.service"
installed_systemd_timer_path="/lib/systemd/system/prune-guard.timer"
daemon_target="${deb_root}${installed_daemon_path}"
systemd_unit_target="${deb_root}${installed_systemd_unit_path}"
systemd_timer_target="${deb_root}${installed_systemd_timer_path}"

mkdir -p \
  "$debian_dir" \
  "$(dirname "$daemon_target")" \
  "$config_dir" \
  "$doc_dir" \
  "$(dirname "$systemd_unit_target")" \
  "$(dirname "$systemd_timer_target")"

cp "$daemon_binary" "$daemon_target"
cp "$config_template" "${config_dir}/prune-guard.toml"
cp "$service_template" "$systemd_unit_target"
cp "$timer_template" "$systemd_timer_target"
cp "${workspace_root}/README.md" "${doc_dir}/README.md"
cp "${workspace_root}/Cargo.toml" "${doc_dir}/Cargo.toml"
cp "${workspace_root}/Cargo.lock" "${doc_dir}/Cargo.lock"

cat > "${debian_dir}/control" <<EOF
Package: prune-guard
Version: ${version}
Section: admin
Priority: optional
Architecture: ${deb_arch}
Maintainer: Prune Guard Maintainers <noreply@example.com>
Description: Safety-first cleanup daemon release payload and config template
 This package ships the prune-guard daemon binary and a fail-closed default
 configuration template for Linux installs.
EOF

# Mark config as a conffile so local admin edits are preserved across upgrades.
cat > "${debian_dir}/conffiles" <<EOF
/etc/prune-guard/prune-guard.toml
EOF

cat > "${debian_dir}/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
  systemctl daemon-reload || true
  # Service drives recurring interval from /etc/prune-guard/prune-guard.toml.
  # Timer is bootstrap-only and should not define cadence.
  systemctl stop prune-guard.timer || true
  systemctl disable prune-guard.timer || true
  systemctl enable prune-guard.service || true
  systemctl restart prune-guard.service || true
fi
exit 0
EOF

cat > "${debian_dir}/prerm" <<'EOF'
#!/bin/sh
set -e
if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
  systemctl stop prune-guard.timer || true
  systemctl disable prune-guard.timer || true
  systemctl stop prune-guard.service || true
  systemctl daemon-reload || true
fi
exit 0
EOF

find "$deb_root" -exec touch -t 197001010000 {} +
find "$deb_root" -type d -exec chmod 0755 {} +
find "$deb_root" -type f -exec chmod 0644 {} +
chmod 0755 "$daemon_target" "${debian_dir}/postinst" "${debian_dir}/prerm"

deb_path="${output_dir}/prune-guard_${version}_${deb_arch}.deb"
checksum_path="${deb_path}.sha256"

dpkg-deb --build "$deb_root" "$deb_path"

if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$deb_path" | awk '{print $1 "  " $2}' > "$checksum_path"
elif command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "$deb_path" | awk '{print $1 "  " $2}' > "$checksum_path"
else
  echo "Neither sha256sum nor shasum is available." >&2
  exit 1
fi

printf '%s\n' "$deb_path"
printf '%s\n' "$checksum_path"
