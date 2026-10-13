#!/usr/bin/env bash
# Build the embedded browser client into dist/.
#
#   web/*.{html,js,css,png,ico} UI source (no framework, no npm)
#   src/wasm                  the measurement client, compiled to WebAssembly
#
# Steps: cargo (wasm32, size profile) -> wasm-bindgen --target web ->
# wasm-opt -Oz (if installed) -> gzip -9 -n. Only the .gz files land in dist/;
# the server sends them with Content-Encoding: gzip (and gunzips for the rare
# client that does not accept gzip). Afterwards `cargo build --release` embeds
# dist/ into the nettest binary.
#
# Requirements: rustup target wasm32-unknown-unknown, and the wasm-bindgen CLI
# at exactly the version of the wasm-bindgen crate in Cargo.lock:
#   cargo install wasm-bindgen-cli --version <version printed below>
# Optional: binaryen's wasm-opt (roughly halves the .wasm).
#
# Every build also writes dist.sha256: the checksums of everything the page
# was built from (the Rust sources the WASM build compiled, Cargo.toml,
# Cargo.lock, the web/ files, this script) and of the files in dist/.
#
#   ./scripts/build-web.sh --check
#
# verifies that manifest without building anything (CI runs it), so a change
# to web/ or the WASM client without a rebuild fails the check. A byte-for-byte
# rebuild in CI is not used because the WASM differs between rustc versions.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WEB_DIR="${WEB_DIR:-$ROOT/web}"
DIST_DIR="$ROOT/dist"
PROFILE="wasm-release"
TARGET="wasm32-unknown-unknown"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

say() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

MANIFEST="dist.sha256"

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

# The web/ files that are copied into dist/ (same patterns as the cp below).
web_files() {
  local f
  for f in "$WEB_DIR"/*.html "$WEB_DIR"/*.js "$WEB_DIR"/*.css "$WEB_DIR"/*.png "$WEB_DIR"/*.ico; do
    [ -f "$f" ] && printf '%s\n' "${f#"$ROOT"/}"
  done
  return 0
}

check_manifest() {
  [ -f "$MANIFEST" ] || die "$MANIFEST not found; run ./scripts/build-web.sh"
  local ok=1 listed f
  # Files that changed or disappeared since the last build.
  if ! grep -v '^#' "$MANIFEST" | sha256 -c --quiet - 2>&1 | sed 's/^/  /'; then
    ok=0
  fi
  # Files that were added since the last build.
  listed="$(grep -v '^#' "$MANIFEST" | sed 's/^[0-9a-f]*  //')"
  for f in $(web_files) dist/*; do
    [ -e "$f" ] || continue
    printf '%s\n' "$listed" | grep -qxF "$f" || { echo "  $f: not in $MANIFEST"; ok=0; }
  done
  if [ "$ok" = 1 ]; then
    say "dist/ is up to date with web/ and the WASM client"
  else
    die "dist/ is out of date. Run ./scripts/build-web.sh and commit dist/ and $MANIFEST."
  fi
}

if [ "${1:-}" = "--check" ]; then
  check_manifest
  exit 0
fi
[ $# -eq 0 ] || die "usage: $0 [--check]"

command -v cargo >/dev/null || die "cargo not found"
command -v gzip >/dev/null || die "gzip not found"
command -v wasm-bindgen >/dev/null || die "wasm-bindgen not found (cargo install wasm-bindgen-cli --version $(
  awk '/^name = "wasm-bindgen"$/{getline; gsub(/version = |"/, ""); print; exit}' Cargo.lock))"

# The CLI and the crate must match exactly, or the generated glue will not
# load the module.
CRATE_VER="$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/version = |"/, ""); print; exit}' Cargo.lock)"
CLI_VER="$(wasm-bindgen --version | awk '{print $2}')"
[ "$CRATE_VER" = "$CLI_VER" ] || die "wasm-bindgen CLI $CLI_VER != crate $CRATE_VER (cargo install wasm-bindgen-cli --version $CRATE_VER)"

if command -v rustup >/dev/null && ! rustup target list --installed | grep -qx "$TARGET"; then
  say "rustup target add $TARGET"
  rustup target add "$TARGET"
fi

# Reproducible output: no absolute build paths in panic locations. Setting the
# target's RUSTFLAGS replaces .cargo/config.toml's, so repeat its stack size
# (the client's I/O buffers overflow the default 1 MiB wasm stack).
REGISTRY="${CARGO_HOME:-$HOME/.cargo}/registry/src"
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C link-arg=-zstack-size=8388608 \
--remap-path-prefix=$ROOT=/nettest --remap-path-prefix=$REGISTRY=/cargo"

say "cargo build --lib --target $TARGET --profile $PROFILE"
cargo build --lib --target "$TARGET" --profile "$PROFILE"

say "wasm-bindgen $CLI_VER --target web"
wasm-bindgen "target/$TARGET/$PROFILE/nettest.wasm" \
  --out-dir "$STAGE" --target web --no-typescript

RAW_SIZE=$(stat -c%s "$STAGE/nettest_bg.wasm" 2>/dev/null || stat -f%z "$STAGE/nettest_bg.wasm")
if command -v wasm-opt >/dev/null; then
  say "wasm-opt -Oz"
  # Feature set rustc's wasm32 target emits by default.
  wasm-opt -Oz --strip-debug --strip-producers \
    --enable-bulk-memory --enable-mutable-globals --enable-nontrapping-float-to-int \
    --enable-sign-ext --enable-reference-types --enable-multivalue \
    "$STAGE/nettest_bg.wasm" -o "$STAGE/nettest_bg.opt.wasm"
  mv "$STAGE/nettest_bg.opt.wasm" "$STAGE/nettest_bg.wasm"
else
  say "wasm-opt not found, skipping (install binaryen for a ~40% smaller module)"
fi

say "assembling dist/"
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"
cp "$WEB_DIR"/*.html "$WEB_DIR"/*.js "$WEB_DIR"/*.css "$WEB_DIR"/*.png "$WEB_DIR"/*.ico "$STAGE"/nettest.js "$STAGE"/nettest_bg.wasm "$DIST_DIR"/

# -n: no file name or timestamp in the header, so identical input gives
# identical bytes (and identical ETags).
for f in "$DIST_DIR"/*; do
  gzip -9 -n "$f"
done

printf '\n%-24s %10s\n' "file" "gzip bytes"
total=0
for f in "$DIST_DIR"/*.gz; do
  s=$(stat -c%s "$f" 2>/dev/null || stat -f%z "$f")
  total=$((total + s))
  printf '%-24s %10d\n' "$(basename "$f")" "$s"
done
printf '%-24s %10d\n' "total" "$total"
printf '(wasm before wasm-opt: %d bytes raw)\n\n' "$RAW_SIZE"

# Inputs: the local sources rustc read for the WASM build (from its dep-info
# file), the build configuration, the web/ files and this script.
say "writing $MANIFEST"
DEP_INFO="target/$TARGET/$PROFILE/nettest.d"
[ -f "$DEP_INFO" ] || die "$DEP_INFO not found"
{
  echo "# Written by scripts/build-web.sh; verified in CI by scripts/build-web.sh --check."
  echo "# rustc: $(rustc --version)"
  echo "# wasm-bindgen: $CLI_VER"
  echo "# wasm-opt: $(command -v wasm-opt >/dev/null && wasm-opt --version || echo 'not used')"
  {
    sed 's/^[^:]*://; s/\\$//' "$DEP_INFO" | tr ' ' '\n' | grep -v '^$' |
      sed "s|^$ROOT/||" | grep -v '^/' | grep '^src/'
    for f in Cargo.toml Cargo.lock .cargo/config.toml scripts/build-web.sh; do
      [ -f "$f" ] && echo "$f"
    done
    web_files
    ls dist/*
  } | LC_ALL=C sort -u | while read -r f; do sha256 "$f"; done
} > "$MANIFEST"

say "done. Now: cargo build --release"
