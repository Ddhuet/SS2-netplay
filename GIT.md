# Git workflow

This repository is a Git superproject with several independent repositories
checked out as submodules. A push from the repository root does **not** save
uncommitted source changes inside a submodule.

## Repository and remotes

| Directory | Purpose | Push remote |
|---|---|---|
| repository root | SS2 harness, `app/`, `notes/`, and submodule pointers | `origin` -> private `Ddhuet/SS2-netplay` |
| `getgud/` | Unmodified Tango rollback engine | Do not modify or push without an explicit decision to create a project mirror |
| `mgba-rollback/` | Generic rollback integration and state fixes | `project` -> private project mirror |
| `mgba-rs/` | Rust mGBA wrapper and build glue | `project` -> private project mirror |
| `mgba-rs/mgba-sys/mgba/` | Embedded C mGBA fork | `project` -> private project mirror |

In the dependency repositories, `origin` is the Tango upstream. Do not push
project work to `origin`; push it to `project`.

## Before every commit

Start at the repository root and inspect both the superproject and every nested
repository:

```powershell
git status --short --branch
git submodule foreach --recursive git status --short --branch
```

Review changes before staging:

```powershell
git diff --stat
git diff
```

Never commit ROMs, saves, recordings, captured executables, credentials, private
keys, diagnostic logs, or firewall snapshots. The ignore rules cover the known
locations and extensions, but always review `git status` rather than relying on
the ignore file alone.

Preserve unrelated user changes. Never use `git reset --hard`, `git clean -fd`,
or checkout/restore commands that discard work merely to prepare a commit.

## Ordinary harness or documentation change

For changes confined to top-level files, `app/`, or `notes/`:

```powershell
git add -A
git status --short
git diff --cached --stat
git diff --cached
git commit -m "Describe the change"
git push origin main
```

The root branch is `main`, not `master`. Since it tracks `origin/main`, a plain
`git push` also works after the upstream is configured.

## Dependency or emulator change

Commit and push nested repositories from the inside out. Only after the inner
commit exists should the containing repository record its new submodule pointer.

### Embedded mGBA changed

```powershell
cd mgba-rs/mgba-sys/mgba
git status --short --branch
git diff --check
git add -A
git diff --cached
git commit -m "Describe the emulator fix"
git push project HEAD:main

cd ../..
```

The final `cd` above returns to `mgba-rs/`, not the repository root. Record the
new embedded mGBA commit in `mgba-rs`:

```powershell
git status --short --branch
git add mgba-sys/mgba
# Also stage any intentional mgba-rs source/build changes.
git diff --cached
git commit -m "Update mGBA integration"
git push project HEAD:main

cd ..
```

Now at the repository root, record the updated `mgba-rs` pointer:

```powershell
git add mgba-rs
git diff --cached
git commit -m "Update mgba-rs dependency"
git push origin main
```

### `mgba-rs` changed without an mGBA change

Commit and push inside `mgba-rs`, then commit its pointer at the root:

```powershell
git -C mgba-rs add -A
git -C mgba-rs diff --cached
git -C mgba-rs commit -m "Describe the wrapper change"
git -C mgba-rs push project HEAD:main

git add mgba-rs
git diff --cached
git commit -m "Update mgba-rs dependency"
git push origin main
```

### `mgba-rollback` changed

```powershell
git -C mgba-rollback add -A
git -C mgba-rollback diff --cached
git -C mgba-rollback commit -m "Describe the rollback change"
git -C mgba-rollback push project HEAD:main

git add mgba-rollback
git diff --cached
git commit -m "Update mgba-rollback dependency"
git push origin main
```

If multiple dependency layers changed, finish and push all inner repositories
before making the single top-level pointer commit.

## Tests before pushing

For rollback/emulator changes, run the generic regressions:

```powershell
$env:RUSTFLAGS = "-l shell32"
cargo test --manifest-path mgba-rollback/Cargo.toml
```

For harness changes, run:

```powershell
$env:RUSTFLAGS = "-l shell32"
cargo test --manifest-path app/Cargo.toml
```

The harness integration tests can print a large amount of mGBA SIO logging.
Judge success by the final test result and exit code; do not hide or ignore a
failure merely because the output is noisy.

After committing, verify everything is clean and points at the intended commits:

```powershell
git status --short --branch
git submodule status --recursive
git submodule foreach --recursive git status --short --branch
```

## If `git add .` staged too much

Staging does not modify or delete the working files. First inspect what is
actually staged:

```powershell
git status --short
git diff --cached --stat
git diff --cached
```

Unstage everything without discarding edits:

```powershell
git restore --staged .
```

Or unstage one path:

```powershell
git restore --staged path/to/file
```

Then stage only the intended paths. Do not use `git reset --hard` to correct an
over-broad `git add`.
