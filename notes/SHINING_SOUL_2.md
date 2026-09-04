# Shining Soul II: Phase 1 implementation audit

Audit date: 2026-09-04. Scope: answer the ten harness questions, without writing
implementation code. This supplements [ARCHITECTURE.md](ARCHITECTURE.md), whose
environment setup, dependency audit, and passing baseline results are accepted
without rerunning them. No emulator was run in this phase. No personal save was
read or changed; the local ROM was read only for identification and a save-library
marker. Compatibility expectations below are not a claim of successful linked
gameplay or rollback validation.

## Smallest harness

Use one small executable/example in `mgba-rollback`, owning **one `Link` with two
GBA cores**, constructed through `Link::with_options`. Supply the same ROM bytes
to both `SideOptions`, separate optional save bytes, an explicit fixed RTC value,
and `Peripheral::Cable`. Keep side A at index 0 and side B at index 1 throughout.

The executable needs only file loading, a tick-indexed input source for both
players, and a loop calling `Link::try_tick` with two key masks. A headless run
needs no window or audio device. To operate menus and observe gameplay, add one
240x160 presentation surface for a selected `local_player` and, optionally, one
audio output consuming that same core. Both controllers must still receive
inputs, including while joining the multiplayer game. A diagnostic option to
view both screens can help record the initial menu sequence.

`Link` already constructs and connects the cores. There is no need to launch two
mGBA processes, manually schedule each core, implement an SIO protocol, add game
traps, or change `getgud`. A frontend executable/example is the missing piece;
the generic library already provides the required interfaces.

For the first local-link proof, use `Link` directly. Later, a two-peer rollback
simulation requires **two `Session`s, each owning its own two-core `Link`**:
four cores total on the test machine, with local ownership 0 and 1 respectively.
That is a later validation harness, not the minimum needed to link two GBAs.

Suggested future argument contract (names are a proposal, not implemented flags):

| Argument | Meaning |
|---|---|
| `--rom PATH` | Required external ROM; read once and duplicate bytes per side |
| `--save-a PATH` | Optional raw battery save for side 0; absent means fresh storage |
| `--save-b PATH` | Optional raw battery save for side 1; absent means fresh storage |
| `--local-player 0\|1` | Select presentation; default 0; does not reorder cores |
| `--input-script PATH` | Optional recorded/scripted rows for both players |
| `--ticks N` | Bounded headless run for later diagnostics |

No ROM, save, or absolute personal path belongs in compiled source or a committed
fixture. Use the already documented Windows build environment when implementation
starts; no toolchain or dependency upgrade is required for this design.

## Local ROM identity

The existing `Shining_Soul_II.gba` was identified without booting it:

| Field | Observed value |
|---|---|
| Size | 16,777,216 bytes (16 MiB) |
| Header title, trimmed | `SHINING SOUL` |
| Game code / maker / software revision | `AU2P` / `8P` / `0` |
| SHA-1 | `aa1288be9257337abdfe3034898cf6c7aa778fbc` |
| SHA-256 | `e5c64a1740263e9b26120e487f3688fbbf7b3954a2c043c0d84c333df467e0b2` |
| Save-library marker | `FLASH512_V131` at ROM offset `0x366A50` |

The size and SHA-1 match **Shining Soul II (Europe) (En,Fr,De,Es,It)** in the
pinned mGBA `res/nointro.dat:16209-16212`. The header title alone is therefore
insufficient to distinguish this game from Shining Soul I. Use the hash and game
code in future run manifests, not the filename or title alone.

The marker indicates 512-kilobit flash, so the expected raw battery-save size is
**65,536 bytes (64 KiB)**, not 512 KiB. This is static evidence, not a runtime
save-type observation. The emulator defines `GBA_SIZE_FLASH512 = 0x00010000` in
`mgba-rs/mgba-sys/mgba/include/mgba/internal/gba/memory.h:72`. Do not silently
truncate, pad, or convert an unexpected supplied save format.

## 1. Supplying an external ROM path

File paths belong in the caller. Accept an explicit path, preferably through
`args_os`/`PathBuf` so Windows paths need not be valid UTF-8, read it into a
`Vec<u8>`, and pass owned bytes to each `SideOptions.rom`. Report read/load errors
with the relevant path. Relative paths should have a documented base, normally
the process working directory; record the resolved identity/hash for replay.

The existing `examples/link_bench.rs:80-100` demonstrates external file loading;
its contract is `link_bench [players] [rom.gba [save.sav]]`. It is useful as an API
example, but its single save is duplicated across sides and its A/B/Start input
mashing is not a Shining Soul II multiplayer-start script. Merely running that
benchmark would not establish that the game entered co-op.

Evidence: `mgba-rollback/src/lib.rs:307-328,355-391` and
`mgba-rs/src/core.rs:114-125`. The library takes bytes, not paths, and never needs
to know where the user's ROM is stored.

## 2. Same ROM, completely separate saves

**Yes.** Each side receives a separate `OwnedCore`, a separate ROM `VFile`, and,
when supplied, its own save `VFile`. `VFile::from_vec` owns a separate
`Cursor<Vec<u8>>`; writes remain in memory. Reading the ROM once and cloning its
bytes does not share save storage. Even two clones of the same initial save are
independent mutable copies, although different characters/saves make isolation
easier to verify later.

With `save: None`, each core performs its own normal save-type detection and
initialization. An absent argument should mean a fresh cartridge; a supplied but
unreadable save path should be an error, not a silent fresh start. The ROM bytes
are loaded through the byte API, so there is no frontend rule that automatically
reuses a single adjacent `.sav` file for both sides.

Evidence: `mgba-rollback/src/lib.rs:368-391`; `mgba-rs/src/vfile.rs`, especially
the `Cursor<Vec<u8>>` implementation and `VFile::from_vec`.

## 3. Supplying save A and save B

Read each optional raw battery-save file separately and assign A to
`sides[0].save`, B to `sides[1].save`. These are battery images, not emulator
savestates or frontend-specific state containers. Supply the whole cartridge
image, not one character slot: Sega's [FAQ, Q12](https://backup.segakore.fr/shining-world.jp/soul2/faq.html)
describes eight in-game save slots. For repeatable tests, retain
immutable original save bytes and hashes; use the same A/B ordering every run.
If no character exists, the future input script must include character creation
and any language/menu selection required by this European release.

Use `SideOptions.save` at boot. Do not replace it with an early
`savedata_restore`: before save-type detection there may be no storage to restore
into. `Link::with_options` specifically uses `load_save` before reset to handle
this ordering (`src/lib.rs:375-383`).

`Link::export_save(0)` and `export_save(1)` retrieve the independent current
images. Persistence belongs to the caller: diagnostic runs should leave input
files untouched, and any later export should use distinct explicit output paths.
`None` from export means save storage has not been detected, not that an empty
file should replace an existing save (`src/lib.rs:523-528`).

Separate backing stores solve isolation, **not rollback of flash writes**.
As established in `ARCHITECTURE.md`, `Snapshot` omits battery-save contents and
`Snapshot::digest` does not cover them. That gap matters directly to this game:
rewinding CPU/RAM/SIO state can leave a speculative flash modification in place.
The later determinism phase must exercise an actual save operation and compare
exported save bytes in addition to machine state. A boot-state capture carrying
a save image does not repair the ordinary per-tick snapshot gap.

Sega's [multiplayer instructions](https://backup.segakore.fr/shining-world.jp/soul2/multi.html)
say only the host can initiate saving. Trigger the future save regression from
side 0 and inspect both cartridges afterward; that UI rule does not establish
which cartridges write flash during the coordinated save.

For eventual peers, “same initial world” means both peers have ROM + save A at
side 0 and ROM + save B at side 1. Each peer needs both initial saves in its
simulated world, even though it presents only its locally owned side.

## 4. Displaying only the locally owned framebuffer

Read only `Link::video_buffer(local_player)` after a successful tick and upload
that buffer to one window/texture. Local ownership is a frontend/session choice,
not the GBA cable master ID; selecting side 1 must not swap the side order.

In this pinned build the buffer is 240x160, 16-bit BGR555/XBGR1555, 76,800 bytes,
with red in bits 0-4, green 5-9, and blue 10-14. It is not an RGBA byte array.
Convert explicitly if the presentation backend requires RGBA, using native
16-bit reads appropriate to this Windows build. The pinned wrapper has no RGBA
conversion helper; the newer wrapper change mentioned in `ARCHITECTURE.md` is
not a reason to upgrade dependencies during this phase.

When later using `Session`, obtain readout through `Session::with_link` or
`LinkHandle::with_link`, copy the needed pixels while holding the lock, then
present outside it. Do not tick or load the link behind the session's back.

Evidence: `mgba-rollback/src/lib.rs:548-552`;
`mgba-rs/src/core.rs:344-359`;
`mgba-rs/mgba-sys/mgba/include/mgba-util/image.h:14-29`;
`mgba-rollback/src/session.rs:149-157,397-410`.

## 5. Running the shadow without rendering/presenting

**Yes.** Do not present its buffer, and set the shadow's frameskip to
`i32::MAX` through `Link::set_frameskip`. Set the local side to 0 for normal
rendering. `Session::new` already disables rendering for every non-local side.
`Link` itself initially enables video buffers/rendering for all sides.

This skips software drawing while the shadow still executes CPU, PPU timing,
interrupts, DMA, audio hardware, and link traffic. Its framebuffer allocation
still exists and may contain stale pixels; “no rendering” is not “no GBA video
hardware.” There is no need to remove its buffer or stop advancing its core.

Frameskip and its counter are set together by the wrapper and are presentation
state outside savestates. Existing generic evidence includes
`tests/audio_fidelity.rs:89-104`, which compares local audio with shadow rendering
disabled. The game-specific rendered-versus-skipped equivalence check remains
for the later executable phase.

Evidence: `mgba-rollback/src/lib.rs:566-573`;
`mgba-rollback/src/session.rs:323-333`; `mgba-rs/src/gba.rs::set_frameskip`;
embedded `src/gba/video.c:183-213`.

## 6. Audio ownership and rollback presentation

Use one host audio output sourced only from
`link.core_mut(local_player).audio_buffer()`. Keep the selected core's complete
stereo mix; “local audio” means the sound heard by that GBA, including sounds of
other characters that the game itself mixes. Do not mix both cores into the
speakers or create an output device for each shadow.

Game-specific expectation: Sega's [FAQ, Q05](https://backup.segakore.fr/shining-world.jp/soul2/faq.html)
says multiplayer generally plays sound effects only, with background music in
some locations. Missing music in co-op is therefore not by itself an audio bug.
This is archived Japanese documentation; verify the local European release's
presentation during the later gameplay run.

For a headless harness, omit the host device and discard mixed output. For a
playable harness, consume the local ring regularly and discard shadow rings at
safe tick/advance boundaries. With `Session`, do so through its locked link
access, outside the `advance` operation. Drain/discard only presentation samples;
do not disable emulated sound channels, sound timers, or sound DMA on shadows.

The wrapper provides signed `i16` samples. `available()` and the `read` count
are sample-frame counts; allocate `count * channels()` sample elements. Query
`core.audio_sample_rate()` and resample to the host device's rate when needed;
the default option value of 48 kHz is not sufficient evidence of the ring's
runtime rate. `mgba::audio::AudioResampler` is available. Keep the external host
queue modest because samples already handed off cannot be revoked.

The mixed rings are bounded; writes can be dropped when full. Leaving shadows
unread does not save emulation work, and regular discard avoids stale backlog.
Evidence: `mgba-rs/src/audio.rs`; `mgba-rs/src/core.rs:110-111,266-275`;
embedded `src/util/audio-buffer.c:52-62`.

For later rollback, use the existing `Session` audio accounting: it removes
revoked queued samples and suppresses regenerated samples already consumed.
Do not routinely clear the local ring during rollback; that would discard valid
backlog. Direct `Link::load` does not perform this correction. Headless direct
save/load tests can discard all presentation audio; audible rollback tests need
the session path. Existing tests cover generic rollback audio, but they do not
establish Shining Soul II Direct Sound/DMA fidelity. See
`src/session.rs:199-288`, `tests/audio_fidelity.rs`, and
`tests/netplay.rs:176-341`, plus the limitations already recorded in
`ARCHITECTURE.md`.

## 7. Link mode and unusual SIO requirements

**Use ordinary GBA Game Link Cable multiplayer**, with two to four full game
instances, using `Peripheral::Cable`. An [archived copy of Sega's official
Shining Soul II multiplayer page](https://backup.segakore.fr/shining-world.jp/soul2/multi.html)
specifies one GBA and one SS2 cartridge per player, supporting up to four players.
It calls for one GBA link cable for two players, two for three, and three for
four, connected before power-on, with unused systems disconnected. This supports
booting exactly two cable-connected cores; no single-cartridge multiboot or RFU
adapter is called for. The page is preserved on a third-party mirror and covers
the Japanese release.

The expected hardware transfer mode is 16-bit GBA multiplayer. A register-level
trace of this ROM's startup and active co-op has not been captured in this phase,
so exclusive use of `GBA_SIO_MULTI` throughout the game's lifetime is not proven.
Hardware multiplayer support alone does not establish every mode transition.

The pinned generic cable driver implements multiplayer transfers and normal
8-bit/32-bit transfers. UART, GPIO, and Joybus data transfers are marked
unsupported in its data path. If future diagnostics encounter one of those,
first distinguish an actual required transfer from an idle/reset mode selection.
No Shining Soul II-specific requirement for RFU, Joybus, UART, or a custom cable
protocol has been established by this audit.

Evidence: embedded `src/gba/sio/lockstep.c:596-659,845-863`. The known
Shining Soul II BIOS issue described below is separate from SIO handling.

## 8. Expected support from the generic driver

**Yes as the implementation hypothesis; not yet a verified compatibility claim.**
Start with the existing cable driver and the unmodified game. The game executes
its own multiplayer protocol while the driver exchanges emulated serial data;
the harness supplies joypads, not decoded game packets. No game-specific
protocol replacement or game-address hooks are justified by current evidence.

Generic two-, three-, and four-side exchange tests and snapshot tests already
passed in the previous audit. They demonstrate reusable infrastructure, not this
commercial game's complete handshake, timing, save behavior, or gameplay.
Future errors must be isolated using tick/key history, core PCs, SIOCNT/RCNT,
player IDs, transfer activity, and slice counts before considering any C-driver
change. Keep those diagnostics separate from generic behavior.

## 9. Existing tests, compatibility notes, and Tango references

### Local repository evidence

- No Shining Soul II-specific test, script, or game-code override was found in
  the checked-out `mgba-rollback`, `mgba-rs`, or `getgud` source. Searches for the
  game name and `AU2` codes found the mGBA ROM database identification entries,
  not a compatibility patch. A ROM database entry is not a multiplayer test.
- Reusable harness patterns: `tests/loopback.rs` (cable exchange),
  `tests/fidelity.rs` (fresh-instance and corrected replay comparisons),
  `tests/netplay.rs:68-174` (multiple local peer worlds and confirmed-row replay),
  and `tests/audio_fidelity.rs`. Existing BN3/BN6 probe examples are game-specific
  diagnostics for other games and do not prove SS2 support.
- `mgba-rollback/README.md` explicitly describes generic local simulation of all
  linked GBAs instead of per-game traps. That is the relevant Tango-derived
  integration for this project; its experimental label should be preserved.

### Upstream findings

1. The mGBA maintainer's [Bugfixes and Regressions note](https://forums.mgba.io/showthread.php?tid=341)
   identifies a Shining Soul II soft lock caused by HLE BIOS ArcTan2 behavior.
   It names [fix 415298e](https://github.com/mgba-emu/mgba/commit/415298ebcd0d702581d9b7d4244c4a69cc0dd54f),
   committed 2016-06-16. Local Git history confirms that full commit is an
   ancestor of pinned mGBA `89e7fd7ff21e369f325fba9ef8b486510a1eebc9`.
   The current implementation is now integer-based (`src/gba/bios.c:319-353,466`),
   so this is confirmation that the fix entered this lineage, not a claim that
   the historical diff is unchanged or a new gameplay regression test. No BIOS
   workaround or external BIOS requirement follows from that old report.
2. A [2023 mGBA forum thread](https://forums.mgba.io/showthread.php?tid=5632)
   reports difficulty with Shining Soul 2 multiplayer. Its reply describes the
   standalone frontend's multiplayer-window setup. There is no reproducible
   SIO diagnosis, tested revision, or confirmation of success in that thread.
   Treat it as a reason to validate the actual join flow, not as proof that the
   pinned driver either works or fails.
3. The [main Tango repository](https://github.com/tangobattle/tango) advertises
   Mega Man Battle Network netplay and exposes game-support modules for that
   family. Its existence does not imply a ready-made SS2 adapter. The relevant
   reusable code here is the already checked-out
   [generic mgba-rollback repository](https://github.com/tangobattle/mgba-rollback).
   No SS2-specific Tango integration or pinned-fork multiplayer success report
   was located in this bounded search. This is a search result, not a claim that
   no such work exists anywhere.

The Sega sources provide a useful join procedure for the later script: select
multiplayer and a character on each core, wait for all connected players to show
OK, then have host/1P press A. Translate that documented flow into measured
European-ROM input ticks rather than assuming equal menus or timing across
regions. See the [multiplayer page](https://backup.segakore.fr/shining-world.jp/soul2/multi.html).
The [FAQ, Q04](https://backup.segakore.fr/shining-world.jp/soul2/faq.html) also
explicitly rules out linking Shining Soul I with II.

## 10. Scripted and recorded controller input

The cleanest injection point is **one player-indexed key row per `Link` tick**.
Use `mgba::input::keys` constants and pass exactly two masks to `try_tick`.
All keys are latched before any core runs for that tick. Do not use OS key events,
wall-clock sleeps, game-memory writes, or instruction traps as the replay format.

Define state at reset as tick 0 and row 1 as the masks producing state at tick 1.
Each row gives the full held-button state for both players; zero releases all
buttons. Holds repeat the same mask across rows, and press/release edges require
separate rows. Missing rows should be rejected or expanded by a documented rule,
not guessed during replay. An optional run-length form can expand to this same
canonical sequence before execution.

| Button | Mask | Button | Mask |
|---|---|---|---|
| A | `0x001` | B | `0x002` |
| Select | `0x004` | Start | `0x008` |
| Right | `0x010` | Left | `0x020` |
| Up | `0x040` | Down | `0x080` |
| R | `0x100` | L | `0x200` |

Masks use pressed=1; do not invert them to match the hardware KEYINPUT register.
A link tick is one reference-core frame boundary, not a guarantee that both
cores finish on the same scanline. Record the harness tick, not host frame time.
For live recording, sample each controller once at this boundary and store the
actual latched masks. The initial recording should include the complete route
from the chosen boot/save fixture into co-op; exact menu timings are not yet
measured and should not be invented from the benchmark's button mashing.

A replay manifest should identify ROM hash, initial A/B save hashes or explicit
fresh-save status, fixed RTC, core/wrapper revisions, BIOS/default configuration,
peripheral, player order, and input tick convention. The immutable external ROM
and save fixtures remain outside version control. A script by itself does not
define a repeatable initial machine state.

When testing `Session` later, feed the local mask through `advance` and each
other player's input through `add_remote_input` in strict per-player order,
exactly once. Use `drain_confirmed()` for authoritative recordings: its rows are
already in global player order and 1-based. `Outgoing.tick` begins at 0, so do
not confuse network input sequence numbers with resulting-state tick numbers.
Do not record predictions or observer callbacks as final input: they may be
revoked and replayed. `tests/netplay.rs:146-174` shows confirmed rows replayed
directly through a fresh `Link`.

Evidence: `mgba-rollback/src/lib.rs:576-631`;
`mgba-rs/src/input.rs`; `mgba-rollback/src/session.rs:182-189,413-420,472-493,535-549`.

## Boundaries for the next phase

The design can proceed with the existing API, but the following results are
still unmeasured and should become separate, explicit evidence:

1. Boot the identified ROM twice with independent saves, reach the same co-op
   session, and demonstrate both characters responding to their own inputs.
   Record the exact menu/join sequence, language selection, and initial saves.
2. Observe SIO mode transitions during joining and active gameplay, and retain
   useful diagnostics on any stall. A booted title screen is not a linked-game
   success criterion.
3. Compare fresh-run/replay and save/load/replay outcomes, including nonzero
   Direct Sound/FIFO/DMA state and exported flash bytes across a game save.
   Equal existing snapshot digests alone are insufficient.
4. Check local presentation/audio ownership and shadow-rendering equivalence,
   then test the two-session artificial-latency configuration. Four-player
   gameplay remains a separate expansion after the two-player proof.

No source, dependencies, tests, ROMs, or saves were changed for this audit.
Repository revisions still match `ARCHITECTURE.md`; the existing modifications
to `mgba-rollback/Cargo.toml` and `Cargo.lock` are the earlier local-path setup.
Only this notes file was updated. No build or test suite was rerun.
