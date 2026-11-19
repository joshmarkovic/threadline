# GPUI Component 0.10.0 source patch

Threadline uses the original crates.io GPUI Component 0.10.0 release with [one source patch](first-visible-source-line.patch) that exposes the first visible source line in the diff. The upstream repository is `longbridge/gpui-kit`, release commit `f697c3133d55c64fa8a5b67926147fb3dadd276d`, path `crates/component`. The original archive SHA-256 is `607c90f6985ac2b96e54102338994eaea143aff521375c6f9e7fc7ecb3ba3646`. Its Apache 2.0 license and all other archive members are preserved. [Upstream license](LICENSE-APACHE) · [Original index record](upstream-index.json)

Cargo resolves this single package from the tracked relative local registry. It resolves the component's dependencies from crates.io using the original requirements. A normal locked build uses the checked-in archive without a regeneration step.

To regenerate the archive and registry entry offline, supply the original crate from a trusted Cargo cache. Run this with Bash, GNU tar, gzip, patch, jq and sha256sum.

```bash
bash third_party/gpui-component/repack.sh /path/to/upstream/gpui-component-0.10.0.crate
```

Keep the archive immutable at its recorded source identity. For a future source patch change, use a new local registry directory and source identity before updating the lockfile. This gives Cargo a fresh cache entry.
