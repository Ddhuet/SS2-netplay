# Phase 4: recorded-input determinism and rollback

The manual two-window frontend is the input capture device. It records the two
full masks actually passed to `Link::try_tick`, indexed by emulation tick, rather
than Windows event timestamps. F9 (or Escape/window close) finalizes the recording,
closes the windows, and automatically starts headless verification in the same
executable. A recording requires a new directory under ignored `recordings/`.

Example from the project root:

```powershell
app/target/release/ss2-local-link.exe --rom Shining_Soul_II.gba --save1 saves/player1_save/Shining_Soul_II.sav --save2 saves/player2_save/Shining_Soul_II.sav --record recordings/session-name
app/target/release/ss2-local-link.exe --rom Shining_Soul_II.gba --verify recordings/session-name
```

Record the multiplayer entry sequence, then movement, combat, transitions and
other representative gameplay. Capture the entire sequence from reset. Initial
battery saves are privately copied into the recording; original files are never
modified. Neither ROMs nor personal recording artifacts belong in Git.

The starting-state recipe fixes ROM SHA-256, ordered initial save SHA-256 values,
RTC seconds, cable topology and frontend build SHA-256. Replay requires the same
executable. The completed input stream also has a SHA-256 integrity check. Each
checkpoint boundary means that exactly that many input rows have executed.

Verification stages:

1. Reboot with identical configuration and reproduce recorded subsystem SHA-256
   values at boundary zero, every 60 ticks, and the final boundary. Collect a
   baseline fingerprint for every tick.
2. Reboot again and compare every tick with that baseline.
3. Run two independent rollback Sessions (four emulated GBAs) at latency 0, 2, 5,
   and 10 ticks, with presentation delay zero. Deliver inputs in sender order;
   observe state at every emulated tick and revoke observations on rewind. Only
   settled observations are compared against the baseline. Drain final packets
   with explicitly additional neutral inputs, comparing only the original capture
   range. Require every captured tick to settle and nonzero correction events for
   nonzero latency. Report correction counts and maximum depths. Render both cores
   to match the capture configuration; this test does not test frameskip invariance.
4. Save each 60-tick segment's starting boundary, execute its inputs, load that
   snapshot using the unmodified production `Link::load`, then replay the segment
   and compare every later tick. The final shorter segment is included.

The direct baseline deliberately does not patch or supplement production snapshots
with private save-memory restoration. Speculative save writes that survive a load
must surface as failures. Audio output rings are consumed by the frontend and
headless runner; emulated audio machinery is part of the diagnostics.

`verification.txt` records completed checks and the first failing boundary and
component hashes. Direct replay mismatches also dump actual component bytes.
Rollback mismatches identify the earliest differing settled tick, not whichever
speculative state happens to be presented when the mismatch is discovered.
For diagnosis, reproduce the named latency/peer and tick, capture both component
byte arrays there, diff field offsets, then instrument that subsystem's events
before changing emulator behavior. Do not mask a field solely because it differs.

Equality is evidence for the captured workload and explicitly instrumented state,
not a proof for all possible game behavior. Validate long gameplay, save writes,
menu transitions, and a variety of active-link snapshot boundaries before trusting
rollback. Network transport, audio-device playback quality, and arbitrary build
compatibility remain separate work.

Diagnostic coverage is implemented in `Link::diagnostic_components`: zero-initialized
full mGBA serialized state divided into format ranges, SIO driver/coordinator blobs,
live FIFO and DMA fields, exported save memory, event queues (ordered names,
priorities and relative deadlines), interrupt/sleep state, RTC/GPIO and wrapper
player identity. Addresses and output video/audio rings are excluded. Custom RTC
source objects and frontend/session audio playback bookkeeping are not full-state
hashed. The runner validates session outcomes and confirmed input ordering through
public APIs; it does not serialize getgud internals.

Canonicalization has two explicit rules: equivalent timing origins are compared
using absolute current/global time and relative event deadlines; the hardware
`HW_NO_OVERRIDE` (0x8000) configuration sentinel is excluded from live device flags.
The synthetic rollback test demonstrated that mGBA's byte-sized serialized device
field drops that marker on load. Source inspection found its uses in override
configuration/reporting, while actual device flags remain included. This initial synthetic-test canonicalization did not change emulator or rollback
restore behavior; the subsequently discovered RCNT restore fix is described below.

Validation before manual capture: the ROM-free synthetic cable workload uses 125
ticks with changing masks, spanning two full snapshot segments and a short final
segment. It checks fresh replay, the latency matrix, snapshot replay, corrupt
checkpoint detection and input-integrity rejection. Shining Soul II multiplayer
validation still requires the user's capture and its actual verification result.

## First real-game capture (2026-09-04)

The 4,588-tick user capture reproduced all recorded checkpoints and every tick of
a second independent fresh run. Both rollback peers matched all 4,588 baseline
ticks at latency 0/2/5/10. Each nonzero-latency run produced 68 corrections on peer
0 and 78 on peer 1; maximum depths were 1/4/9 ticks respectively.

The long snapshot test exposed two separate differences:

- Restoring boundary 0 initially differed only in the old deadline of an
  **unscheduled** IRQ event. The event was absent from the active timing queue.
  Diagnostics now encode the deadline only when scheduled, with a regression
  verifying that changing an inactive deadline is ignored but scheduling the
  event remains detectable. The original failure report is preserved.
- Restoring boundary 1,140 after running through 1,200 left player 0 RCNT at
  `0x0003` rather than its saved `0x0001`. The first replayed boundary, 1,141,
  therefore differed in serialized IO offset `0x134` and the live SIO shadow.
  `GBAIODeserialize` used `GBASIOWriteRCNT`, whose normal CPU-write semantics
  preserve read-only pin bits from the current live state. Snapshot restoration
  needs those bits from the saved state instead. This is a restore bug, not a
  field to exclude from the digest.

`app/examples/diagnose_restore.rs` reproduces full 60-tick snapshot segments from
an existing capture, validates its ROM/save/input hashes, and dumps first differing
bytes. Its optional output-directory argument re-executes the exact same input
text under the current build and runs the entire verification matrix, preserving
the original recording and executable for comparison.

## Final validated result

The minimal core fix assigns the saved RCNT shadow after its mode-specific write
in `GBAIODeserialize`. Ordinary CPU writes retain their existing behavior. Build
glue now tracks this C source explicitly. The ROM-free RCNT regression fails
without this assignment (actual 3, expected 1) and passes with it; the test also
checks driver state remains unchanged after initial attach events have settled.

Re-executing the exact original input file on the fixed build passed:

| Check | Result |
|---|---|
| Fresh replay | Two runs, all 4,588 ticks |
| Rollback latency 0 | Both peers, all 4,588 settled ticks |
| Rollback latency 2 | Both peers, 68/78 corrections, maximum depth 1 |
| Rollback latency 5 | Both peers, 68/78 corrections, maximum depth 4 |
| Rollback latency 10 | Both peers, 68/78 corrections, maximum depth 9 |
| Snapshot replay | 77 segments, every replayed tick compared |
| Library suite | 48 tests passed (46 upstream plus two regressions) |
| App suite | Three tests passed |

The original `phase4-visible-20260904-165206` recording remains untouched except
for additional diagnostic artifacts. The fixed-build reproduction is under
`recordings/phase4-recertified-rcnt`, whose `provenance.txt` records its source and
input hash. The runner checks byte-for-byte input equality before verification.
Final results are in that directory's `verification.txt`. Original failure logs
and binary remain available; nothing was committed or distributed.

This validates the captured workload and instrumented state, not every possible
save-writing or gameplay path. Battery-save rollback completeness remains a known
general risk until a workload deliberately exercises writes across a rollback.
