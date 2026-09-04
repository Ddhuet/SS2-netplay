# GBA rollback environment and architecture audit

Audit date: 2026-09-03. This report covers setup, untouched-upstream validation,
implementation inspection, and the controlled switch to local path dependencies.
No upstream source was changed during the audit. The existing ROM in the project
root was treated as user-owned and was not inspected, executed, copied, or used by
any command.

## Repository state and dependency graph

The repositories were cloned directly under the project root; `mgba-rs` was cloned
recursively and its `mgba-sys/mgba` submodule was initialized.

| Layer | Revision used for the known-good baseline | Role |
|---|---|---|
| `mgba-rollback` | `0e037e9bfacd4bc70ccd9f1935d854d08f0fae6a` | Multi-core link and GBA-specific `getgud` integration |
| `getgud` | `747b51bba50589f83f6e6724fcb01ccf35d73c73` | Generic deterministic rollback session |
| `mgba-rs` (`mgba` and `mgba-sys`) | `3a4621b158adef0ca1d82cde79d54ea52a795587` | Safe-ish wrapper plus raw build/FFI layer |
| embedded `tangobattle/mgba` | `89e7fd7ff21e369f325fba9ef8b486510a1eebc9` | C emulator core and SIO drivers |

The first three revisions come directly from the untouched
`mgba-rollback/Cargo.lock` (`getgud` at lines 136-140 and `mgba`/`mgba-sys` at
lines 207-240). The embedded mGBA revision is the submodule commit recorded by
the pinned `mgba-rs` tree.

The intended architecture is already the implemented architecture:

```text
caller-owned frontend and transport
  -> mgba-rollback::session::Session (one per peer)
     -> getgud::Session<LinkWorld>
        -> mgba-rollback::Link (all local GBA cores are one rollback world)
           -> mgba-rs / mgba-sys
              -> embedded Tango mGBA lockstep or wireless SIO driver
```

Every peer runs every GBA. Only joypad rows, tick sequence metadata, optional
clock-skew metadata, and caller-selected digest checks need to cross a real
network. Emulated cable/RFU traffic remains local and deterministic.

## Host and build environment

- Windows x86-64, native MSVC Rust target.
- Git 2.46.2.windows.1.
- rustup 1.29.1, stable `x86_64-pc-windows-msvc`.
- rustc 1.98.1 (`48a229cea`, LLVM 22.1.8), Cargo 1.98.1.
- Visual Studio 2022 Build Tools 17.14.12, MSVC 14.44.35207, x64/x86 C++
  tools, and Windows SDK.
- CMake 3.30.2, LLVM/Clang/libclang 22.1.8, and Ninja available.

The tools actually required are established by `mgba-rs/mgba-sys/build.rs`:
bindgen requires libclang; `cmake::Config` builds the C core; `cc::Build`
separately compiles the lockstep and wireless SIO sources; native Windows uses
MSVC. The build forces `COLOR_16_BIT`, `MINIMAL_CORE=1`, and
`DISABLE_THREADING` (`mgba-rs/mgba-sys/build.rs:69-99`), configures the native
core and explicitly restores pruned SIO sources (`:125-212`), extracts matching
C defines (`:8-67`), generates bindings (`:264-316`), and links Windows system
libraries (`:245-256`). No standalone Tango mGBA clone is needed.

Cargo's normal user cache is not writable in the managed environment, so the
commands used `CARGO_HOME=<project>/.cargo-home`. Cargo's libgit2/schannel path
also lacked usable credentials in the sandbox; Git dependencies were fetched
with `CARGO_NET_GIT_FETCH_WITH_CLI=true`, and initial registry downloads were
allowed through the normal elevated network execution.

### Windows linker finding

The untouched pinned build compiles but initially fails at link time:

```text
config.obj : error LNK2019: unresolved external symbol SHGetKnownFolderPath
fatal error LNK1120: 1 unresolved externals
```

`mgba-sys/build.rs` links `shlwapi`, `ole32`, and `uuid` on Windows but omits
`shell32`, which owns `SHGetKnownFolderPath`. The Windows SDK is present; this is
an upstream build-script omission, not a missing host component. To preserve the
untouched baseline, validation used environment-only `RUSTFLAGS="-l shell32"`.
No upstream build script was patched.

## Baseline commands and results

All mGBA commands were run after importing the x64 environment from
`VsDevCmd.bat`, setting `LIBCLANG_PATH=C:\Program Files\LLVM\bin`, using the
project-local Cargo home, and setting `RUSTFLAGS=-l shell32`.

1. Untouched baseline:

   ```text
   cargo test --locked --manifest-path mgba-rollback\Cargo.toml
   ```

   PASS: 46 tests, 0 failed, 0 ignored: 10 library tests; 3 audio-fidelity;
   4 fidelity; 1 hotplug; 3 loopback; 5 netplay; 1 slice-budget; 19 wireless;
   plus 0 doctests.

2. ROM-free release benchmark:

   ```text
   cargo run --locked --release --manifest-path mgba-rollback\Cargo.toml --example link_bench
   ```

   PASS using the built-in 2-player SIO ping-pong ROM. Results for 600 measured
   ticks after 120 warmup: plain 1062.4 ticks/s (17.8x realtime), remote-skip
   1122.4 (18.8x), no-render 1025.4 (17.2x), per-tick snapshot 1093.5 (18.3x),
   rollback workload 770.1 (12.9x). `examples/link_bench.rs:1-17,21-173`
   defines these workloads and uses the built-in ROM when no path is supplied.

3. Local-path baseline, after checking the editable dependencies out at the
   exact revisions above and updating only dependency declarations/lock data:

   ```text
   cargo test --manifest-path mgba-rollback\Cargo.toml
   ```

   PASS: the same 46 tests, 0 failed. Cargo reported local packages for
   `getgud`, `mgba`, and `mgba-sys`; the lockfile now intentionally omits Git
   sources for them. Rust 1.98 emits warnings from generated bindgen declarations
   and transmutes, but no test failure. A final rerun of the same suite with
   `--locked` also passed, proving the updated local-path lockfile is stable.

4. Generic rollback engine independently:

   ```text
   cargo test --locked --manifest-path getgud\Cargo.toml
   ```

   PASS: 6 unit tests and 2 doctests.

5. `cargo fmt --manifest-path mgba-rollback\Cargo.toml -- --check` does not
   pass with rustfmt 1.98 because numerous untouched files differ from current
   formatting. This is documented, not hidden or auto-fixed. There is no Cargo
   bench target or CI configuration; `link_bench` is an executable example.

The pinned `mgba-rs` tree has no tracked Cargo.lock, so a separate
`cargo test --locked` correctly refuses to run; it was nevertheless compiled
and exercised through both successful `mgba-rollback` suites.

## Multi-core link construction and scheduling

`mgba_rollback::Link` owns a vector of `mgba::core::OwnedCore`, one SIO driver
per attached side, and a shared coordinator; field order deliberately preserves
safe destruction order (`mgba-rollback/src/lib.rs:80-88`). Cable mode accepts
1-4 cores, while wireless accepts up to 63 attached emulated devices
(`:45-59,174-205`).

`Link::with_options` creates each core with identical default mGBA options,
enables its video buffer, loads that side's ROM and optional independent save,
sets an optional fixed RTC, installs the selected SIO driver, and resets all
cores (`mgba-rollback/src/lib.rs:355-399`). Cable player IDs are positional;
wireless can restore stable requested seats (`:178-203,464-493`).

`Link::try_tick` first latches one `u32` key mask per player, then cooperatively
runs the cores in stable vector order. Each awake core receives one mGBA
`run_loop` slice per outer iteration; sleeping lockstep cores are skipped. The
tick ends when core 0's wrapping frame counter advances (`:576-632`). An
all-asleep link or more than 100,000 slices returns an error instead of hanging.
The architecture is single-threaded emulation even though presentation may use
the session's mutex-protected handle.

## Rollback state: included and external

`mgba_rollback::Snapshot` contains (`mgba-rollback/src/lib.rs:207-244`):

- one ordinary `mgba::state::State` per core;
- one serialized lockstep/wireless driver blob per attached player;
- raw direct-sound FIFO A/B state per core;
- raw state for all four DMA channels per core.

`Link::save` captures core states before driver/FIFO/DMA supplements
(`:634-648`). `Link::load` restores core state, then FIFO/DMA internals, then
driver state so SIO events are scheduled against the rebuilt mGBA timing list
(`:650-671`). This ordering is load-bearing.

State outside an ordinary core savestate includes lockstep coordinator/driver
queues, sleep and in-flight transfer state, some direct-sound FIFO/DMA refill
state, mixed output audio rings, save-data backing storage, host RTC source,
frameskip/presentation state, traps/callbacks, session counters/queues, digest
history, and transport state. The first four machine-state omissions are why
`Link` supplements core states; raw FIFO/DMA capture/restore is implemented at
`mgba-rollback/src/lib.rs:734-848`. The output audio ring is presentation state
and is handled by `session`, not `Snapshot`. Save data remains a significant
rollback gap, discussed below.

Boot/handoff captures are distinct from per-tick rollback snapshots.
`BootSide` contains ROM bytes, a save image, core state, and optional wireless
adapter session (`:331-341`). `capture_boot_state`, `capture_adapter_state`, and
`export_save` are at `:502-529`; `Link::from_states` rebuilds fresh cores and
drivers at `:401-500`. Raw state import checks size, but not ROM identity,
emulator ABI/version, peripheral, configuration, or provenance (`:695-731`).

At the wrapper layer, `mgba-rs/src/core.rs:114-203` exposes ROM/save loading,
savedata clone/restore, and core save/load; `src/state.rs:3-67` wraps the fixed
`GBASerializedState`; and `src/sio.rs:55-176,196-359` wraps cable/wireless
coordinators, drivers, driver blobs, and wireless adapter blobs.

## Inputs, ownership, players, video, and audio

mGBA keys are a `u32` mask set through `mgba::Core::set_keys`
(`mgba-rs/src/core.rs:203-205`). `Link::try_tick` requires a player-indexed slice
whose length equals core count. At the session layer, `LinkWorld::key_row`
places the local input at `local_player` and maps getgud remote slots around it
(`mgba-rollback/src/session.rs:170-200`). `Session` stores `local_player` and
`num_players`; `Outgoing` carries tick, keys, and signed tick advantage
(`:45-61,303-316`). Total session size is 2-4 for cable usage; generic getgud
represents it implicitly as one local FIFO plus `initial_remotes.len()` remote
FIFOs (`getgud/src/input.rs:3-31`, `getgud/src/session.rs:159-171`). There are no
generic player IDs: mgba-rollback performs the mapping.

Video for any side comes from `Link::video_buffer`; it is the wrapper-owned
240x160 framebuffer. `set_frameskip` is intentionally not serialized and is
treated as simulation-invisible (`mgba-rollback/src/lib.rs:548-573`). Session
construction disables rendering on shadow cores; callers may re-enable it
(`mgba-rollback/src/session.rs:318-365`).

Audio is read per core with `core_mut(i).audio_buffer()`; wrapper ring methods
are in `mgba-rs/src/audio.rs:8-31` and core access/configuration is at
`mgba-rs/src/core.rs:266-275`. During rollback, `LinkWorld` accounts for samples
produced per core. Loading removes only the still-queued revoked speculative
tail and records already-played revoked samples; catch-up re-simulation discards
that regenerated prefix so it is not echoed (`mgba-rollback/src/session.rs:27-43,
199-231,245-288`). Direct `Link::load` alone does not repair consumed or queued
presentation audio.

## Saves, RTC, ROM/config identity, and external assumptions

Each side has an independent optional `Vec<u8>` save passed through an in-memory
VFile before reset (`mgba-rollback/src/lib.rs:372-384,431-449`). Separate initial
saves and later `export_save(i)` therefore work cleanly per core. However,
ordinary rollback snapshots do **not** include savedata. Speculative writes to
SRAM/flash/EEPROM can survive a rollback and are a concrete determinism hazard.
Boot captures do include an exported save image, but that does not close the
per-tick rollback gap (`:212-224,502-529`).

Peers must externally agree on player order/count, peripheral type, identical
ROM bytes, per-core initial save bytes, mGBA/mgba-rs build and forced compile
configuration, BIOS choice/absence, emulator options, and a fixed RTC source.
`LinkOptions` allows a fixed `SystemTime`; its documentation says this is
mandatory for RTC-bearing games in netplay/replay (`:306-325`). The current API
does not negotiate or validate these values, and snapshots only validate player
count on load. No BIOS is passed by `Link`, so the audited path uses mGBA's
configured/default boot behavior consistently.

## `getgud` contract and session behavior

`getgud::World` requires deterministic `step(local, remotes)`, complete
`save`/`load`, optional snapshot recycling, and a predictor based on the previous
remote input (`getgud/src/world.rs:64-120`). `LinkWorld` uses `SnapshotAt`
(link snapshot + tick + per-core audio-production counters) and repeat-last
prediction (`mgba-rollback/src/session.rs:179-296`).

`getgud::Session::advance` enqueues local input, FIFO-matches complete rows,
computes the presentation target, settles/promotes or rolls back, speculates to
the target, increments its local frontier, and returns the displayed frame plus
newly confirmed rows (`getgud/src/session.rs:236-300,417-535`). Correct predicted
prefixes are promoted without replay; a first mismatch discards speculative
states, loads the settled state, and linearly re-simulates corrected rows.

`present_delay` is local presentation delay, not negotiated input delay.
Prediction occurs only when the presentation target is beyond confirmed state.
A sufficiently large delay therefore behaves like delay-style netcode **only
while all remote input latency stays within the chosen delay**; no finite delay
can guarantee no prediction under unbounded stalls (`getgud/src/session.rs:
236-257,485-501`; `mgba-rollback/src/session.rs:318-388`). The separate
`Throttler` consumes `skew()` and `speculation_balance()` and returns an FPS
slowdown for the caller to apply (`mgba-rollback/src/throttler.rs:1-103`).

`getgud` supplies no sockets, serialization, packet protocol, reliability,
authentication, ACKs, retransmission, deduplication, or reordering. Remote slots
are FIFO queues. `mgba-rollback::Session::add_remote_input` requires every
player's packets in tick order, exactly once; `Outgoing::tick` is provided so a
caller-owned transport can enforce this (`mgba-rollback/src/session.rs:413-421`).
The `netplay` test's `Wire`/mesh is only an in-process artificial-latency test
(`mgba-rollback/tests/netplay.rs:28-121`), not production transport.

`local_queue_length` and `matchable` expose stall-guard signals but enforce no
bound (`mgba-rollback/src/session.rs:445-460`). `TickObserver` sees speculative
and replayed ticks and must revoke its own effects after `on_rewind`
(`:85-103`). `drain_confirmed` supplies final 1-based player-indexed input rows
for a caller-owned replay sink (`:535-550`).

## Digests and desync detection

Generic getgud has no hash API. `mgba-rollback::Snapshot::digest` computes CRC32
over selected CPU GPR/CPSR data, WRAM/IWRAM, SIO driver blobs, FIFO state, and
DMA state (`mgba-rollback/src/lib.rs:268-304`). It does not cover the complete
serialized core, ROM/config identity, framebuffer, mixed audio ring, savedata,
RTC source, traps, or transport/session queues.

`Session` retains up to 600 observed settled `(tick, digest)` entries and exposes
`checkpoint`/`digest_at`; transmission and comparison remain the caller's job
(`mgba-rollback/src/session.rs:298-316,496-533`). If one advance settles multiple
ticks, intermediate boundaries are not retained. CRC32 is a diagnostic checksum,
not collision-resistant validation.

## Cable and wireless state

Cable and wireless drivers live in the embedded fork and are re-added to the
minimal core build by `mgba-sys/build.rs`. The Rust wrappers serialize each
driver through its C vtable (`mgba-rs/src/sio.rs:93-176,248-359`). The cable
coordinator orders emulated cores; the wireless coordinator models shared
airwaves, stable wireless IDs, host/client rosters, mailboxes, timeouts, and
scheduled events. Wireless driver and adapter layouts/serialization are in
`mgba-rs/mgba-sys/mgba/src/gba/sio/wireless.c:41-225,441-653,710-810`.

Wireless is well exercised as a direct `Link`, but it is not exercised through
`mgba_rollback::session::Session`; initial target work should therefore not infer
RFU netplay readiness from cable-session results.

## Raw `mgba-sys` usage in `mgba-rollback`

Production raw access is concentrated in `mgba-rollback/src/lib.rs:734-848`:

- cast `Core::gba_mut().as_raw()` to reach `mgba_sys::GBA` internals;
- deschedule/repair an overdue link completion event after load;
- capture and restore direct-sound FIFO buffer/read/write/size state;
- capture and restore DMA next source/destination/count/control/when fields.

These calls bypass `mgba-rs` because the higher-level wrapper exposes neither
the required timing-event repair nor these internal FIFO/DMA fields. They are
narrow workarounds for demonstrated savestate fidelity gaps, not a second core
abstraction. Wireless tests also call raw C SIO functions and fields to drive
protocol-level handshakes (`mgba-rollback/tests/wireless.rs:41-70`); probe
examples inspect CPU/SIO internals. Several regression tests rely on hard-coded
serialized driver offsets (`mgba-rollback/src/lib.rs:875-915,958-989,1033-1073`),
so embedded C layout changes require deliberate review.

## Existing test coverage and gaps

| Area | Existing evidence | Important uncovered edge |
|---|---|---|
| Link fidelity | `tests/loopback.rs:61-152`: 2/3/4-player exchange and rollback | Real commercial-game protocols |
| Snapshot/replay | `tests/fidelity.rs:42-91`: cross-instance boot and corrected 2/3-player replay | Save writes and complete-state equality |
| Artificial latency | `tests/netplay.rs:66-174`: 2/3-peer mesh, forced rollback, convergence, linear confirmed replay | Loss, duplication, reorder, jitter, disconnect/rejoin |
| Desync | settled CRC comparisons in `tests/netplay.rs:104-120` | Full-state/config/save hashing and actual digest packets |
| Audio | `tests/audio_fidelity.rs:1-105`; `tests/netplay.rs:176-341` | Input-driven SOUNDBIAS changes and nonzero Direct Sound FIFO/DMA fixture |
| Hotplug | `tests/hotplug.rs:1-74` | Session-level peer churn |
| Wireless | `tests/wireless.rs:205-1103`: protocol, host/client, 4/5-player groups, multiple groups, snapshots, leave/join, wrap, up to 10 attached | Rollback Session over RFU and advertised 63-device limit |
| Scheduling budget | `tests/slice_budget.rs:9-30` for 2/4 cores | Long-running real-game stalls |
| Clock wrap | library cable and wireless tests cover signed mGBA cycle wrap | Session/getgud `u32` tick/frontier wrap |

The built-in ROM exercises cable traffic and PSG audio without copyrighted data.
It does not visibly drive Direct Sound FIFO/DMA, so the raw snapshot supplement
lacks a nonzero end-to-end fixture. No Shining Soul II-specific automated test
exists yet, as expected for this phase.

## Long-run determinism and robustness hazards

1. Savedata is not part of per-tick rollback snapshots; speculative persistent
   writes may leak across restore.
2. Packet tick/order/dedup/loss correctness is entirely delegated to the future
   transport; a malformed FIFO insertion silently associates input with the
   wrong tick.
3. ROM, save, BIOS, peripheral, RTC, configuration, and state ABI identity are
   not negotiated or comprehensively hashed.
4. `Snapshot::digest` is deliberately partial and CRC32-based; equal digests do
   not prove total-state equality.
5. getgud queues/speculation have no built-in capacity limit. A silent peer can
   cause unbounded backlog unless the caller implements the documented stall
   guard.
6. Session/getgud counters use unchecked `u32` increments; link-level cycle-wrap
   regressions do not prove multi-day session counter wrap safety.
7. getgud advance is not transactional if `World::step/save/load` returns an
   error after queue/frontier mutation.
8. Mixed audio correction assumes deterministic regeneration; an input-driven
   sample-rate change inside the rollback window is explicitly acknowledged as
   a possible small seam (`mgba-rollback/src/session.rs:256-268`).
9. Snapshot import validates count but not origin/ABI/config; incompatible bytes
   can be accepted if sizes happen to match.
10. Raw serialized-layout offsets and internal C-field restoration are sensitive
    to future mGBA revisions and need regression tests before upgrades.
11. Tick observers and concurrent `LinkHandle` readout have no dedicated tests;
    callers can desynchronize bookkeeping if they tick/load behind Session.
12. Wireless direct-link coverage is broad, but wireless rollback/session and
    long-duration commercial-game behavior remain unproven.

## Local dependency conversion and upgrade decision

After the untouched baseline passed, `mgba-rollback/Cargo.toml` was changed
without altering names or feature behavior:

```toml
getgud = { path = "../getgud" }
mgba = { path = "../mgba-rs" }
mgba-sys = { path = "../mgba-rs/mgba-sys" }
```

The cloned `getgud` already matched the lockfile. `mgba-rs` was detached at the
locked revision and its mGBA submodule detached at the revision recorded there.
This gave an apples-to-apples PASS before considering upgrades.

`mgba-rs/origin/main` is four commits ahead of the pinned revision and changes
canonical BGR555-to-RGBA conversion, embedded mGBA revision/build invalidation,
WASM thread-sysroot handling, and logger initialization. Advancing now is not
desirable: those are real wrapper/core/build changes and would combine dependency
redirection with an upgrade. Keep the known-good detached revisions for this
phase; evaluate that upgrade later as its own change with the same full suite and
targeted framebuffer/SIO/snapshot comparisons.

## Additional repository-specific guidance discovered

- Keep `RUSTFLAGS=-l shell32` in the Windows developer command environment until
  an intentional upstream/local build-script fix is separately tested.
- Treat the pinned mGBA submodule revision as part of the reproducible dependency
  set; do not update `mgba-rs` without updating/validating the submodule.
- Preserve the `Link::load` order (core -> FIFO/DMA -> SIO driver) and Link field
  drop order.
- Do not assume `cargo fmt --check` failure is a code failure or autoformat the
  upstream tree as incidental cleanup.
- Do not feed raw network arrivals directly to `add_remote_input`; enforce
  per-player tick ordering and exactly-once delivery first.
- Before any real-game claim, add targeted proof for speculative save writes,
  fixed external configuration, and Direct Sound FIFO/DMA restore fidelity.
