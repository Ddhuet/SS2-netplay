Project goal:
Produce usable rollback multiplayer for Shining Soul II using
tangobattle/mgba-rollback.

Repository layout:

mgba-rollback/
    Generic linked-GBA rollback integration.
    Make changes here first where appropriate.

getgud/
    Generic rollback/session algorithm.
    Do not make GBA- or Shining-Soul-specific changes here.

mgba-rs/
    Rust mGBA wrapper.

mgba-rs/mgba-sys/
    Raw C bindings/build glue.

mgba-rs/mgba-sys/mgba/
    Tango's mGBA C fork.
    Modify only when emulator/SIO/state correctness requires it.

notes/
    Documentation about the current project and architecture.
recordings/
    Heavy logs of recorded emulation + netplay.

Initial priority:
1. Preserve upstream tests.
2. Establish Shining Soul II 2-player local linked simulation.
3. Prove deterministic save/load/replay.
4. Prove rollback under artificial latency.
5. Only then build real network transport/UI.

Rules:
- Do not redesign getgud merely because another design seems cleaner.
- Do not rewrite mGBA systems without a demonstrated correctness problem.
- Prefer regression tests before fixing suspected determinism bugs.
- Keep generic logic generic.
- Keep Shining Soul II-specific diagnostics/tests separate from generic emulator logic whenever possible.
- Do not commit or redistribute ROMs.
- Do not commit personal save files.
- Never silently weaken or delete tests merely to make a test suite pass.
- Avoid large unrelated refactors.
- When encountering a desync, first instrument and isolate the divergent state rather than guessing.
- Preserve the ability to compare our changes against untouched upstream behavior.

Add any additional repository-specific guidance you discover during inspection.

----DEVELOPMENT DISCIPLINE----
You may:
- clone repositories
- initialize/update Git submodules
- inspect history
- run Git commands
- run Cargo
- run CMake/build tools
- install ordinary development dependencies when permissions allow
- edit source
- create tests
- create scripts
- create notes/documentation
- use local environment variables
- compile/debug code
- run automated test suites
Do not:
- download copyrighted ROMs
- search for ROM piracy sources
- commit ROMs
- commit personal saves
- disable security software
- execute random binaries downloaded from unrelated sources
- use destructive system commands
- rewrite large sections of upstream code without demonstrated need
- delete or weaken tests simply because they fail
- silently ignore failing tests
- treat warnings or suspected desyncs as fixed without validating them
Use Git aggressively for safety:
- inspect git status before major changes
- keep unrelated changes separate
- make changes in small understandable units
- preserve the known-good baseline
- make it easy to diff our work against upstream
You may create branches for our changes.
When changing low-level emulator or rollback behavior, prefer:
```rust
reproduce -> add diagnostic/test -> patch -> prove regression fixed
```
rather than:
```rust
speculate -> rewrite -> hope
```
Phase 4 guidance:
- Keep manual captures, initial save copies, state dumps, and captured executables
  under ignored `recordings/`; preserve original artifacts when rechecking a fix.
- Record button masks at `Link::try_tick`, and compare only settled rollback
  observations against a direct baseline. Matched-input frontiers can run ahead
  of the actually simulated settled checkpoint.
- RCNT read-only line levels are snapshot state: normal CPU register-write
  semantics are insufficient for restore. Keep the RCNT regression when changing
  SIO deserialization. The build glue explicitly tracks `src/gba/io.c` edits.
- Inactive timing-event deadlines are stale storage, not pending events. Preserve
  scheduled-event coverage and a regression for any diagnostic canonicalization.
