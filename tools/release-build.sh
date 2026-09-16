#!/bin/sh
# Build the release tarball for THIS machine's target, the same shape the
# release workflow builds for x86_64 and aarch64.  Useful for looking at what
# a release contains before pushing a tag, and for the case the workflow
# cannot cover: a machine nobody else has.
#
# Output lands in dist/, which is not tracked.
set -eu

cd "$(dirname "$0")/.."

ver=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
target=$(rustc -vV | sed -n 's/^host: //p')
name="copal-tm-$ver-$target"

echo "building $name"
cargo build --release --locked

rm -rf "dist/$name"
mkdir -p "dist/$name"
cp target/release/copal-tm "dist/$name/"
strip "dist/$name/copal-tm" 2>/dev/null || echo "  (strip unavailable; shipping unstripped)"
cp README.md LICENSE "dist/$name/"

tar -C dist -czf "dist/$name.tar.gz" "$name"
rm -rf "dist/$name"
( cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256" )

echo
ls -lh "dist/$name.tar.gz"
cat "dist/$name.tar.gz.sha256"
