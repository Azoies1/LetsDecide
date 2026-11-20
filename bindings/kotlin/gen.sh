#!/usr/bin/env bash
# Builds the Rust library and generates the Kotlin glue. Run from anywhere.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
cargo build -p letsdecide-uniffi --release
case "$(uname -s)" in
  Darwin) lib=libletsdecide_uniffi.dylib ;;
  MINGW*|MSYS*|CYGWIN*) lib=letsdecide_uniffi.dll ;;
  *) lib=libletsdecide_uniffi.so ;;
esac
out=bindings/kotlin/build/generated
rm -rf "$out/uniffi"
cargo run -q -p letsdecide-uniffi --bin uniffi-bindgen -- generate \
  --library "target/release/$lib" --language kotlin --no-format \
  --config bindings/uniffi/uniffi.toml --out-dir "$out/uniffi"
# Packaged in the jar so JNA finds it on the classpath.
mkdir -p "$out/native"
cp "target/release/$lib" "$out/native/"
