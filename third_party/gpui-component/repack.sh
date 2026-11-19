#!/usr/bin/env bash
set -eu -o pipefail

if [ "$#" -ne 1 ]; then
    printf 'Usage: %s /path/to/upstream/gpui-component-0.10.0.crate\n' "$0" >&2
    exit 2
fi

upstream_crate=$1
registry=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
expected=$(jq -er .cksum "$registry/upstream-index.json")
actual=$(sha256sum -- "$upstream_crate" | cut -d ' ' -f1)
if [ "$actual" != "$expected" ]; then
    printf 'The upstream archive checksum does not match the recorded release.\n' >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
package=gpui-component-0.10.0
selection="$package/src/diff/selection.rs"
state="$package/src/diff/state.rs"
gzip -dc -- "$upstream_crate" > "$work/package.tar"
tar -xf "$work/package.tar" -C "$work" "$selection" "$state"
patch --batch --fuzz=0 -d "$work/$package" -p3 < "$registry/first-visible-source-line.patch"
tar --delete -f "$work/package.tar" "$selection" "$state"
member_options=(--mtime=@1 --owner=0 --group=0 --numeric-owner --mode=0644)
tar --append -f "$work/package.tar" "${member_options[@]}" -C "$work" "$selection" "$state"
gzip -n -9 -c "$work/package.tar" > "$work/$package.crate"

checksum=$(sha256sum "$work/$package.crate" | cut -d ' ' -f1)
jq -ce --arg checksum "$checksum" '
    .cksum = $checksum |
    .deps |= map(if .registry == null then .registry = "https://github.com/rust-lang/crates.io-index" else . end)
' "$registry/upstream-index.json" > "$work/index-entry"
cp -- "$work/$package.crate" "$registry/$package.crate"
cp -- "$work/index-entry" "$registry/index/gp/ui/gpui-component"
