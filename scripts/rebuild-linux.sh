#!/usr/bin/env bash
set -euo pipefail

BUNDLES="appimage,deb"
NO_BUNDLE=false
CLEAN_DIST=false
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundles)
      BUNDLES="${2:-}"
      shift 2
      ;;
    --no-bundle)
      NO_BUNDLE=true
      shift
      ;;
    --clean-dist)
      CLEAN_DIST=true
      shift
      ;;
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APPS_ROOT="$ROOT/apps"
TAURI_DIR="$APPS_ROOT/src-tauri"
ROOT_TARGET="$ROOT/target"
TAURI_TARGET="$TAURI_DIR/target"
DIST_DIR="$APPS_ROOT/dist"

step() { echo "$*"; }

remove_dir() {
  local path="$1"
  if [[ ! -e "$path" ]]; then
    step "skip: $path not found"
    return
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    step "DRY RUN: remove $path"
    return
  fi
  rm -rf "$path"
}

run_cmd() {
  local display="$1"
  shift
  if [[ "$DRY_RUN" == "true" ]]; then
    step "DRY RUN: $display"
    return
  fi
  "$@"
}

command -v cargo >/dev/null 2>&1 || { echo "cargo not found in PATH" >&2; exit 1; }
if ! command -v pnpm >/dev/null 2>&1; then
  echo "warning: pnpm not found; tauri beforeBuildCommand may fail." >&2
fi

remove_dir "$ROOT_TARGET"
remove_dir "$TAURI_TARGET"
if [[ "$CLEAN_DIST" == "true" ]]; then
  remove_dir "$DIST_DIR"
fi

pushd "$TAURI_DIR" >/dev/null
if [[ "$NO_BUNDLE" == "true" ]]; then
  run_cmd "cargo tauri build --no-bundle" cargo tauri build --no-bundle
else
  run_cmd "cargo tauri build --bundles $BUNDLES" cargo tauri build --bundles "$BUNDLES"
fi
popd >/dev/null

step "done"
