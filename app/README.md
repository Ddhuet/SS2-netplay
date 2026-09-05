# SS2 harnesses

## Portable internet test client

`SS2-Netplay` is the native Host/Connect client. It embeds mGBA, uses direct-IP
QUIC over UDP (default port 24872), and loads `ROM/` and `save/` relative to its
EXE. See [PORTABLE_README.txt](PORTABLE_README.txt) for player instructions.

Build with `./build-netplay.ps1`; run the app suite with
`./build-netplay.ps1 -Test`; create the portable folder and ZIP with
`./package-netplay.ps1`. These scripts are for development only. Players copy or
extract the package and launch its EXE without compiling or installing mGBA.

`SS2-Netplay.exe --self-test` runs a ROM-free real-QUIC rollback smoke test and
writes `self-test.txt` beside the EXE. It does not test SS2 save menus. The user
requested that actual in-game save/reload behavior remain a manual check.

## Phase 3 local cable frontend

`ss2-local-link` opens two 2x GBA windows backed by one two-core cable `Link`.
Both cores boot the same ROM bytes from reset and each receives its own private
save backing store. Audio output is intentionally omitted, but both emulated
audio rings are drained so they cannot fill while you play.

From this directory:

```powershell
cargo run --release --bin ss2-local-link -- `
  --rom "..\Shining_Soul_II.gba" `
  --save1 "C:\path\to\alice.sav" `
  --save2 "C:\path\to\bob.sav"
```

Omit either save option for a fresh private cartridge. Input files are loaded
once and never overwritten or exported. Keep the console open for boot/link
errors. Close either window or press Escape to stop both linked systems.

Controls:

| GBA input | Player 1 | Player 2 |
|---|---|---|
| D-pad | Arrow keys | W/A/S/D |
| A / B | Z / X | N / M |
| Start / Select | Enter / Right Shift | Space / B |
| L / R | C / V | Q / E |

The two keyboard maps are disjoint. Keep either game window focused; its keyboard
events drive both players, including simultaneous key holds, and both full key
masks are latched before every linked emulation tick. The documented game flow
is: enter multiplayer and select a character on each side, wait until both show
`OK`, then press A on player 1 (the cable host).

## Rollback observability harness

This is a headless, in-process observability harness. It creates one complete
linked-GBA rollback `Session` per simulated network peer. For two players this
means two sessions and four mGBA cores. Every session receives the same ROM and
the same ordered set of initial saves, while each session owns a different local
player. Packets cross deterministic in-memory queues after the requested latency.

It does not enter Shining Soul II multiplayer automatically, create real sockets,
play audio, display video, or write save data to disk. With no input script it
uses a deterministic changing button schedule whose purpose is to force rollback
corrections and exercise the generic machinery. Use a recorded input script for
the game's actual menu and join sequence.

From this directory:

```powershell
cargo run --release -- `
  --rom "..\Shining_Soul_II.gba" `
  --save1 "C:\path\to\alice.sav" `
  --save2 "C:\path\to\bob.sav" `
  --players 2 `
  --latency 5 `
  --delay 2 `
  --ticks 600
```

Omit a save option to boot that player with fresh in-memory savedata. Input save
files are read once and never overwritten. `--rtc-seconds` fixes the emulated
clock identically in every core; its default matches the existing link benchmark.

An input script is UTF-8 text with one full held-button row per tick. Commas and
ASCII whitespace are accepted, `#` starts a comment, and ticks must be contiguous
from zero:

```text
# tick, player 1, player 2
0, 0x000, 0x000
1, 0x008, 0x000
2, 0x000, 0x001
```

The periodic table reports the host frame, peer, local input frontier, confirmed
frontier, presented state, current speculative presentation depth, this advance's
rollback depth, cumulative rollback event count, maximum rollback depth, and the
latest settled-state digest. The final summary also reports total re-simulated
rollback ticks, maximum speculative depth, maximum mGBA scheduling slices for one
tick, common digest comparisons, detected desyncs, elapsed time, and throughput.

“Rollback events” counts advances whose public `Report::rolled_back` value is
nonzero. “Rollback ticks” sums that value. “Speculative depth” is
`presented.saturating_sub(confirmed)`. Digests are the library's partial CRC32
diagnostic described in `../notes/ARCHITECTURE.md`; equality does not prove complete
state identity. A mismatch at a commonly observed settled tick is a desync and
makes the process exit unsuccessfully.

The library does not expose an exact cumulative count of predictor calls. This
harness therefore does not print a “prediction count.” It reports the observable
speculative depth instead.

On the audited Windows environment, retain the established build variables:
project-local `CARGO_HOME`, `LIBCLANG_PATH`, the Visual Studio x64 environment,
and `RUSTFLAGS=-l shell32` until the documented build-script omission is fixed.

## Phase 4 recording and rollback verification

Use `ss2-local-link --record DIR` to capture exact emulation-tick inputs. Press F9
when finished; the windows close and recorded-input determinism, artificial-latency
rollback, and snapshot replay checks start automatically. `--verify DIR` repeats
those checks headlessly using the same executable and ROM. See
[`notes/PHASE4_DETERMINISM.md`](../notes/PHASE4_DETERMINISM.md) for the test strategy.
