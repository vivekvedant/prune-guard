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
release_payload_dir="${deb_root}/usr/lib/prune-guard/release"
config_dir="${deb_root}/etc/prune-guard"
doc_dir="${deb_root}/usr/share/doc/prune-guard"

mkdir -p "$debian_dir" "$release_payload_dir" "$config_dir" "$doc_dir"

cp -R "${release_dir}/." "$release_payload_dir/"
cp "$config_template" "${config_dir}/prune-guard.toml"
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
 This package ships prune-guard release artifacts and a fail-closed default
 configuration template for Linux installs.
EOF

# Mark config as a conffile so local admin edits are preserved across upgrades.
cat > "${debian_dir}/conffiles" <<EOF
/etc/prune-guard/prune-guard.toml
EOF

find "$deb_root" -exec touch -t 197001010000 {} +
find "$deb_root" -type d -exec chmod 0755 {} +
find "$deb_root" -type f -exec chmod 0644 {} +

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
