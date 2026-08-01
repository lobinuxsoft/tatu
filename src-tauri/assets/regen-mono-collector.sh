#!/usr/bin/env bash
# Regenerate the vendored Mono collector DLL.
#
# The collector is NOT downloaded — tatu builds it. We vendor the pre-built,
# release, stripped artifact here so building tatu itself never needs the
# windows-gnu cross toolchain (CI/release runners stay toolchain-free).
#
# RUN THIS whenever the `cheat-mono-collector` crate changes, then commit the
# updated `cheat-mono-collector.dll`.
#
# Requires: rustup target add x86_64-pc-windows-gnu + mingw (x86_64-w64-mingw32-*).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
out="$here/cheat-mono-collector.dll"

cargo build -p cheat-mono-collector --target x86_64-pc-windows-gnu --release \
  --manifest-path "$repo/Cargo.toml"

src="$repo/target/x86_64-pc-windows-gnu/release/cheat_mono_collector.dll"
cp "$src" "$out"
x86_64-w64-mingw32-strip --strip-all "$out"

exports="$(x86_64-w64-mingw32-objdump -p "$out" | grep -c 'WinHttp' || true)"
size="$(stat -c%s "$out")"
echo "wrote $out ($size bytes, $exports WinHttp exports)"
[ "$exports" -ge 45 ] || { echo "ERROR: expected >=45 WinHttp exports, got $exports" >&2; exit 1; }
