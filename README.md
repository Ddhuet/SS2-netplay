# SS2 Netplay

Experimental rollback netplay for *Shining Soul II*, built around Tango's
`mgba-rollback`, `getgud`, `mgba-rs`, and mGBA work.

The repository contains the project-specific Windows harness, diagnostics, and
integration history. It does **not** contain a game ROM, save files, recordings,
or other copyrighted/private gameplay data. Supply your own legally obtained
game image when running the harness.

## Clone

The project uses nested Git submodules, including private project mirrors:

```powershell
git clone --recursive https://github.com/Ddhuet/SS2-netplay.git
cd SS2-netplay
```

For an existing checkout:

```powershell
git submodule update --init --recursive
```

GitHub authentication is required to clone the private dependency mirrors.

## Layout

- `app/` — Shining Soul II local-link, replay, diagnostics, and QUIC netplay harness.
- `notes/` — architecture, determinism investigations, and validation results.
- `mgba-rollback/` — generic linked-GBA rollback integration plus proven state fixes.
- `getgud/` — unchanged generic rollback/session engine.
- `mgba-rs/` — Rust mGBA wrapper and raw bindings; contains the nested mGBA fork.

Exact upstream revisions and local deltas are recorded in
[`notes/UPSTREAM_PROVENANCE.md`](notes/UPSTREAM_PROVENANCE.md).

## Build and test

From the repository root on Windows:

```powershell
cargo test --manifest-path app/Cargo.toml
cargo build --release --manifest-path app/Cargo.toml --bin SS2-Netplay
```

See [`app/README.md`](app/README.md) and the documents under `notes/` for the
current workflow, command-line options, and known limitations.

## Project boundaries

- Do not commit or redistribute ROMs, personal saves, recordings, or captured executables.
- Keep game-specific behavior in `app/` or dedicated diagnostics.
- Keep reusable rollback fixes generic and covered by regressions.
- Preserve the ability to compare every dependency mirror with its upstream remote.

This is experimental software and is not affiliated with Sega, Grasshopper
Manufacture, Nintendo, Tango, or the mGBA project.
