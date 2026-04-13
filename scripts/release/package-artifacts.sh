#!/usr/bin/env bash
set -euo pipefail

# Package the release build into a deterministic tarball.
# The archive is treated as a release artifact, so the script fails closed if
# the release build is missing or empty.

workspace_root="$(pwd)"
output_dir=""

usage() {
  cat <<'EOF'
Usage: package-artifacts.sh [--workspace-root PATH] [--output-dir PATH]
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

host_triple="$(rustc -vV | awk -F': ' '/^host: / {print $2}')"
if [ -z "$host_triple" ]; then
  echo "Could not determine the Rust host triple." >&2
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

mkdir -p "$output_dir"
staging_root="$(mktemp -d "${TMPDIR:-/tmp}/prune-guard-package.XXXXXX")"
trap 'rm -rf "$staging_root"' EXIT

package_name="prune-guard-${host_triple}"
package_root="${staging_root}/${package_name}"
metadata_root="${package_root}/metadata"
release_copy_root="${package_root}/release"

mkdir -p "$metadata_root" "$release_copy_root"

cp -R "${release_dir}/." "$release_copy_root/"
cp "${workspace_root}/Cargo.toml" "$metadata_root/Cargo.toml"
cp "${workspace_root}/Cargo.lock" "$metadata_root/Cargo.lock"
cp "${workspace_root}/README.md" "$metadata_root/README.md"
cp "${workspace_root}/config/prune-guard.toml" "$metadata_root/prune-guard.toml"

# Normalize timestamps so the archive contents are reproducible for the same
# source tree and Rust toolchain.
find "$package_root" -exec touch -t 197001010000 {} +

archive_tar="${output_dir}/${package_name}.tar"
archive_path="${archive_tar}.gz"
checksum_path="${archive_path}.sha256"

entries=()
while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  entries+=("$entry")
done < <(cd "$staging_root" && find "$package_name" -type f -print | LC_ALL=C sort)

tar -cf "$archive_tar" -C "$staging_root" "${entries[@]}"
gzip -n -f "$archive_tar"

if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$archive_path" | awk '{print $1 "  " $2}' > "$checksum_path"
elif command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "$archive_path" | awk '{print $1 "  " $2}' > "$checksum_path"
else
  echo "Neither sha256sum nor shasum is available." >&2
  exit 1
fi

printf '%s\n' "$archive_path"
printf '%s\n' "$checksum_path"
