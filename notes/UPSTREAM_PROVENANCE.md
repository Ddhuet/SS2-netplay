# Upstream provenance

This workspace began from exact Tango repository revisions. The dependency
repositories retain their original `origin` remotes, while modified layers also
have private project mirrors used by the top-level submodules.

| Layer | Upstream | Starting revision | Project revision / status |
|---|---|---|---|
| `getgud` | `https://github.com/tangobattle/getgud.git` | `747b51bba50589f83f6e6724fcb01ccf35d73c73` | Unmodified; top-level submodule points directly upstream. |
| `mgba-rollback` | `https://github.com/tangobattle/mgba-rollback.git` | `0e037e9bfacd4bc70ccd9f1935d854d08f0fae6a` | Private mirror `Ddhuet/SS2-netplay-mgba-rollback`, branch `main`. |
| `mgba-rs` | `https://github.com/tangobattle/mgba-rs.git` | `3a4621b158adef0ca1d82cde79d54ea52a795587` | Private mirror `Ddhuet/SS2-netplay-mgba-rs`, branch `main`. |
| mGBA C fork | `https://github.com/tangobattle/mgba.git` | `89e7fd7ff21e369f325fba9ef8b486510a1eebc9` | Private mirror `Ddhuet/SS2-netplay-mgba`, branch `main`. |

## Local delta ownership

The initial top-level audit documented only local path dependency changes in
`mgba-rollback/Cargo.toml` and `Cargo.lock`. Subsequent behavior changes were
made against the revisions above and are now committed in the private mirrors:

- mGBA restores RCNT read-only line levels exactly during state load.
- `mgba-rs` exposes the bus write used by savedata regressions and rebuilds when
  the relevant embedded C sources change.
- `mgba-rollback` preserves savedata and latched video contention across
  rollback, adds deterministic diagnostic components, and retains regressions
  for RCNT, savedata, timing-event canonicalization, and video contention.

Project-specific UI, QUIC transport, capture/replay tools, diagnostics, and
packaging live in the top-level `app/` history rather than the dependency mirrors.

## Recovery material

Pre-migration Git bundles and private logs are kept locally under ignored
`recordings/`. They are intentionally not part of the GitHub repository.
