#!/usr/bin/env bash
# macOS only: builds the Rust core for macOS and iOS, packs it as an
# XCFramework and generates the Swift glue. Then: swift test.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
cd "$root"
targets=(aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim)
rustup target add "${targets[@]}"
for t in "${targets[@]}"; do cargo build -p letsdecide-uniffi --release --target "$t"; done

gen="$here/build/gen"
rm -rf "$gen" && mkdir -p "$gen/include"
cargo run -q -p letsdecide-uniffi --bin uniffi-bindgen -- generate \
  --library target/aarch64-apple-darwin/release/libletsdecide_uniffi.a --language swift \
  --config bindings/uniffi/uniffi.toml --out-dir "$gen"
mkdir -p "$here/Sources/LetsDecideFFI"
mv "$gen/LetsDecideFFI.swift" "$here/Sources/LetsDecideFFI/"
mv "$gen/LetsDecideFFIFFI.h" "$gen/include/"
mv "$gen/LetsDecideFFIFFI.modulemap" "$gen/include/module.modulemap"

lipo -create -output "$here/build/libletsdecide_macos.a" \
  target/aarch64-apple-darwin/release/libletsdecide_uniffi.a \
  target/x86_64-apple-darwin/release/libletsdecide_uniffi.a
rm -rf "$here/LetsDecideFFIFFI.xcframework"
xcodebuild -create-xcframework \
  -library "$here/build/libletsdecide_macos.a" -headers "$gen/include" \
  -library target/aarch64-apple-ios/release/libletsdecide_uniffi.a -headers "$gen/include" \
  -library target/aarch64-apple-ios-sim/release/libletsdecide_uniffi.a -headers "$gen/include" \
  -output "$here/LetsDecideFFIFFI.xcframework"
