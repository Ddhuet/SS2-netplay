# Internet netplay: transport and MVP protocol proposal

Date: 2026-09-05. Status: documentation proposal; no implementation or network
benchmark has been performed. Read alongside [ARCHITECTURE.md](ARCHITECTURE.md),
[SHINING_SOUL_2.md](SHINING_SOUL_2.md), and
[PHASE4_DETERMINISM.md](PHASE4_DETERMINISM.md). All three were read for this design.
The later Phase 4 results supersede the early audits' untested-game statements.

## Recommendation and scope

Use **one direct QUIC connection, implemented with Quinn, between two native
clients**, with reliable ordered input delivery in the first MVP. Separate input,
control, and startup-save traffic into streams. The host forwards one configurable
UDP port and shares its public IP, port, certificate fingerprint, and a private
session invitation token. The guest initiates the connection. No matchmaking,
STUN, TURN, ICE, UPnP, relay, or external service is required or proposed.

Each computer runs one rollback Session containing **both GBA cores** in the same
order. Host owns side 0; guest owns side 1. Only inputs, compatibility/setup data,
battery-save images, and synchronization diagnostics cross the network. GBA SIO
transactions, cable timing, video, and audio remain local. The host coordinates
startup; it is not an authoritative game-state server.

This is the best initial engineering tradeoff for this repository, not a claim
that QUIC is fastest on measured SS2 internet workloads. Favor correct bounded
delivery using existing transport machinery before implementing application-level
loss recovery. Keep message semantics independent of Quinn so this decision can
be revisited without changing getgud or the emulated cable.

## What getgud actually abstracts

There is **no transport trait, socket implementation, or wire protocol** in getgud.
Its abstraction is a deterministic `World` with input/state types and
`step`, `save`, `load`, and `predict`. `input::Queue` appends local and remote
inputs to parallel FIFOs and matches rows by position. It does not inspect ticks.
`Session::add_remote_input` also updates the remote tick-advantage hint.

The GBA wrapper supplies the concrete integration:

| Existing API | Network client's obligation |
|---|---|
| `Session::advance(local_keys)` returns `Outgoing { tick, keys, tick_advantage }` | Send every successful advance's output promptly, even unchanged held masks |
| `Session::add_remote_input(player, keys, tick_advantage)` | Validate ownership and deliver exactly once, in contiguous per-player tick order |
| `skew()` and `speculation_balance()` | Read before advance and feed the existing `Throttler`; implement pacing outside getgud |
| `local_queue_length()` and `matchable()` | Bound stalls without preventing available remote inputs from being settled |
| `checkpoint()` / `digest_at(tick)` | Compare the same observed settled boundary; missing digest means unavailable |
| `drain_confirmed()` | Record final player-ordered inputs, not predictions or raw arrival order |
| `TickObserver` | Revoke speculative observations on rewind before publishing settled diagnostics |

Network input sequence 0 produces state boundary 1. State boundary 0 is reset.
`drain_confirmed()` labels that first row 1. Wire fields must distinguish
`input_seq` from `state_boundary`; never silently interchange them.
Matched/confirmed input frontiers can precede actual settlement into simulated
state. An input receipt or `confirmed()` value is not a state-hash checkpoint.

`present_delay` is a local presentation setting. It is not a network-negotiated
input delay and peers need not use the same value. Keep it fixed during the first
MVP session to simplify diagnosis. Repeat-last prediction already lives in
`LinkWorld`; the transport must never manufacture missing authoritative inputs.

Evidence: `getgud/src/world.rs`, `getgud/src/input.rs`,
`getgud/src/session.rs`, `mgba-rollback/src/session.rs`,
`mgba-rollback/src/throttler.rs`, and `app/src/determinism.rs`.

## Transport alternatives considered

| Option | Fit for this contract | Decision |
|---|---|---|
| TCP with TLS | Reliable ordered bytes directly satisfy FIFO delivery; simple direct IP. One stream couples control/bulk loss to input delivery. Separate connections can reduce that coupling but add setup. Disable Nagle for small prompt input writes. | Viable simpler fallback, especially if UDP availability becomes a requirement; do not build a second transport now |
| Raw UDP with a custom protocol | Redundant recent inputs can repair isolated loss before a retransmission timeout. Requires authentication, encryption, congestion behavior, ACKs, retransmission, deduplication, reorder bounds, and reliable startup transfer. | Too much new transport correctness work for this MVP |
| QUIC reliable streams | Reliable ordering per input stream, separate control/setup streams, built-in encrypted transport and recovery. Input-stream loss still blocks later inputs on that stream. | Recommended first implementation |
| QUIC DATAGRAM plus reliable streams | Supports redundant input bundles while retaining reliable setup/control. Datagrams themselves do not guarantee delivery; missing input history still needs explicit recovery. | Candidate optimization after measured loss tests, not an MVP dependency |
| WebRTC data channels | Can express ordered/reliable and other delivery policies, but brings a peer-connection/signaling/ICE integration surface whose browser/traversal benefits are outside this scope. | Defer |
| Steam Networking / open-source GameNetworkingSockets | Reliable/unreliable game messages and direct-IP support are relevant. Open-source GNS is distinct from Steam relay services and need not require Steam. Adds a native integration layer to this Rust client. | Credible alternative if Steam distribution or GNS tooling becomes a concrete priority |

Protocol basis: [TCP specification](https://www.rfc-editor.org/rfc/rfc9293.html),
[QUIC streams](https://www.rfc-editor.org/rfc/rfc9000.html),
[QUIC DATAGRAM](https://www.rfc-editor.org/rfc/rfc9221.html),
[WebRTC peer connections](https://webrtc.org/getting-started/peer-connections),
and [Valve GameNetworkingSockets](https://github.com/ValveSoftware/GameNetworkingSockets).
Quinn provides Rust APIs, Windows support, streams, datagrams, and TLS integration;
see its [official introduction](https://quinn-rs.github.io/quinn/quinn.html).

The important latency limitation is structural: getgud cannot use input 105 before
missing input 104 has been inserted. Datagrams do not eliminate that dependency.
Their potential benefit is proactively recovering 104 from a redundant bundle,
reducing how long the gap persists. QUIC stream separation removes cross-stream
delivery ordering, not shared congestion or bandwidth constraints. Do not send
bulk artifacts during gameplay. Do not open a new stream per frame.

## Wire organization and message requirements

Use an explicit versioned binary encoding with length-prefixed stream messages,
fixed-width integers, a defined byte order, and bounded lengths. Do not serialize
Rust structs by memory layout. Every post-hello message identifies the session
and startup generation. Unknown mandatory types, malformed values, and messages
illegal in the current phase are protocol errors.

Use one bidirectional control stream, one input stream in each direction, and
bounded startup transfer streams. Only one authenticated guest occupies the
second seat. All streams belong to the same authenticated connection. There is
no total ordering across streams: dependencies use manifest hashes, transfer IDs,
and explicit acknowledgments.

| Message | Principal fields | Reliability and ordering | Latency sensitivity |
|---|---|---|---|
| `Hello` / `HelloAccept` / `Reject` | Protocol version, build identity, nonce, invitation proof/token, supported profile; accepted session ID and seat or reason | Reliable control, first application exchange | Startup only |
| `SaveOffer` | Owned seat, fresh/image marker, length, SHA-256 | Reliable control before transfer | Startup only |
| `Manifest` / `ManifestAccept` | Canonical compatibility/configuration and ordered save identities; manifest SHA-256 | Reliable control; both accept exact same bytes/hash | Must finish before boot |
| `SaveData` / `SaveReceived` | Transfer ID, seat, bounded bytes; verified hash acknowledgment | Reliable transfer stream; acknowledgment on control after validation | Startup only; never competes with live inputs |
| `Ready` | Manifest hash, boundary-0 diagnostic schema and SHA-256 | Reliable control after successful local construction | Startup barrier |
| `Start` / `StartAck` / `Run` | Session/generation, manifest hash, first input sequence 0 | Reliable control in this order | Startup barrier, no synchronized wall clock required |
| `Input` | Owned player, `input_seq: u32`, full `keys: u32`, `tick_advantage: i16` | Reliable input stream; strict contiguous sequence; immutable once produced | Highest; enqueue immediately each successful advance |
| `Progress` | Highest contiguous received input as an exclusive count, matched count, actual settled boundary | Reliable control, monotonically interpreted | Roughly every 250 ms; detects lack of application progress |
| `Ping` / `Pong` | Nonce and echoed sender monotonic timestamp | Reliable control; expire old samples | Approximately once per second; informational, not simulation time |
| `StateHash` / `HashAck` | Boundary, manifest/schema IDs, component aggregate SHA-256; acknowledgment identifies matching boundary/hash | Reliable control, compare by boundary rather than arrival order | Every 60 settled ticks initially; never blocks input delivery |
| `Desync` / `Abort` / `Leave` | Reason, relevant boundary, local frontiers, optional component hashes | Reliable control while connection survives; local stop cannot depend on remote receipt | Immediate |

`Progress` is an application-health acknowledgment, not a second reliable
transport. QUIC ACKs and loss recovery remain transport-owned. Pong RTT includes
control-stream delay and should be labeled accordingly; transport RTT is preferable
for the main latency display. No heartbeat should fabricate a gameplay input.

Suggested initial hard limits, to be validated rather than advertised as tuned:
control message 16 KiB; save image exactly 65,536 bytes for this SS2 profile;
two startup images maximum; at most 120 queued remote input records ahead of the
local frontier; at most 1 MiB of application receive buffers per connection.
Bound transport stream counts and flow-control windows too. Reject excessive
lengths before allocation. A duplicate/out-of-order `Input` on the designated
reliable stream is a protocol violation; do not feed it into getgud. Reject
unknown players, invalid button bits outside `0x3ff`, and unexpected generations.
End long sessions before `u32` sequence wrap; widening the wire alone would not
fix the engine's counters.

## Handshake and compatibility

1. Host listens on the selected UDP port and creates a new session invitation.
   Guest connects to the explicit IP and port. Pin the host certificate using
   the fingerprint shared with the invitation; do not disable verification.
   Use a cryptographically random invitation token (at least 128 bits) to admit
   the guest over the encrypted connection. Do not log tokens or accept replayed
   invitations for a new session. Disable 0-RTT application actions in the MVP.
2. Exchange protocol/build/profile identity before sending personal saves.
   Fail with a field-specific explanation on incompatibility.
3. Agree host=side 0 and guest=side 1. Each offers its own initial battery save
   or explicit fresh-cartridge status. Host assembles the manifest; guest must
   validate it against its own files and accepted settings.
4. Exchange the two required save images, verify lengths/hashes, and acknowledge
   completion. Each peer now has the same ordered A/B cartridge data.
5. Both construct fresh two-core Cable Links using their own ROM bytes and the
   agreed configuration, then construct Sessions without advancing them. Compare
   canonical boundary-0 diagnostics and send `Ready`.
6. Host sends `Start` only after both Ready hashes match. Guest returns `StartAck`
   without advancing; host then sends `Run` and may begin. Guest begins on `Run`.
   Inputs arriving on another stream before Run are bounded and buffered, never
   inserted early. Start skew is handled by pacing and the stall guard. There is
   no claim of simultaneous execution, and no dependency on synchronized clocks.

The compatibility manifest must include:

- Wire protocol, SS2 profile, diagnostic schema, and state format identifiers.
- SHA-256 and size of the actual loaded ROM, with header region/revision for
  readable errors. The initial supported profile is the European `AU2P` revision
  0 identified in SHINING_SOUL_2.md. A filename or game title is insufficient.
- Exact executable SHA-256 for the initial same-Windows-build MVP. Also record
  getgud, mgba-rollback, mgba-rs, embedded mGBA revisions and local patch/build
  identity for diagnostics; Git revision alone misses current uncommitted fixes.
- Two players, fixed side order and ownership, Cable peripheral, reset startup,
  initial neutral prediction, input-mask and tick conventions.
- Ordered initial save hashes/lengths or explicit fresh markers. Save A and B
  need not equal each other; each peer's copy of A must equal the other's A.
- Fixed RTC seconds and source behavior; BIOS mode/absence and BIOS hash if a
  future profile permits external BIOS; effective emulator options and hardware
  overrides, cheats disabled, deterministic core/build feature configuration.

Local controller bindings, window size, volume, and present delay do not require
equality. Rendering policy needs its own validation: Phase 4 rendered both cores.
For the first network proof, render both internally while presenting only the
owned side. Enable shadow rendering suppression only after SS2 equivalence tests.

Reject mismatches before emulation rather than offering a force-join mode. ROM
and BIOS bytes are never transmitted. The selected local save is explicitly
shared with the other player as part of joining; show that fact in setup.

## Initial state, save ownership, and persistence

Start from reset with the agreed ROM/config and ordered battery saves. Navigate
the game's multiplayer menus using ordinary networked inputs. Do not import a
host's arbitrary active-link savestate. Initial save exchange is approximately
64 KiB per supplied cartridge and occurs only before play.

Existing `BootSide`/`from_states` facilities are not a reviewed internet state
format: BootSide includes ROM bytes, and a core state alone is not a complete
active linked world. No raw Rust/C snapshot is accepted from the network in v1.

Both peers emulate both cartridges, but a player's file ownership stays local.
Keep originals immutable and record private initial copies under ignored
`recordings/`. Never overwrite the remote player's original save or copy session
data into an implicit adjacent ROM save path.

Battery data is still absent from ordinary rollback snapshots. The passing
4,588-tick capture does not prove speculative flash writes are safe. Deliberately
exercise coordinated in-game saving across rollback before promising durable
progress. Hashing save memory detects differences but cannot undo leaked writes.

For an experimental MVP, preserve diagnostics and original saves; do not
automatically promote the live speculative cartridge image to the user's save.
A durable-progress release needs a validated export of the locally owned save
from an agreed settled boundary, with original backup and atomic separate output.
This may be produced by fresh deterministic replay of final input rows, but that
path also requires validation. On disconnect/desync, mark exports unverified and
leave the original intact. Do not describe a matched input count as a safe live
save-export boundary.

## Pacing, outages, and reconnect behavior

Sample the owned controller once per successful simulation advance and preserve
the complete held mask. Do not batch merely to reduce packet count. Process
network events even while simulation stalls; keep networking outside the emulation
mutex and use bounded queues. Drain confirmed recording rows regularly.

Start with present delay 2 as a user-adjustable pre-session default (0 remains
available for diagnostics). Use the existing Throttler with the actual GBA frame
cadence, not a hardcoded assumption of exactly 60 Hz. These are initial settings,
not measured optimal values.

Proposed stall guard: when local unmatched queue length reaches 10 and
`matchable() == 0`, stop calling advance and show Waiting for player. Ten reflects
the largest artificial input delay already tested, not a universal safety proof.
If matchable inputs exist, allow bounded advances to consume them, subject to
available send-queue capacity. Every such advance produces and sends another real
local input. Never block recovery simply because the unmatched queue is full.
Independently bound incoming records and network send backlog; the unmatched
queue guard alone cannot detect an application that receives but stops consuming.

Transient packet loss on a still-live QUIC connection is recovered by QUIC. Resume
advancing when input progress permits, without resetting sequences or a Session.
Show a stalled connection after 2 seconds without useful input progress; terminate
after 10 seconds without input progress while waiting, or immediately on a terminal
transport/protocol/emulation error. Also enforce a startup timeout, initially
30 seconds. Timers are local monotonic timers; they do not enter emulated state.

**No reconnect into an existing game in v1.** After a failed connection, both
clients end that session (the remote may learn through timeout), retain logs, and
return to setup. Retry creates a new session ID/generation, rechecks everything,
and boots anew. No host migration, seat reassignment, neutral-input substitution,
or mid-session joining. A process crash has the same restart policy. Do not reuse
an old getgud FIFO with a new transport connection.

## Desync detection and resynchronization

Use the Phase 4 canonical diagnostic components with SHA-256: serialized machine
ranges, SIO/coordinator state, FIFO/DMA, save bytes, scheduled timing events,
interrupt/sleep, RTC/GPIO, and player identity. Preserve both established
canonicalization rules and the inactive-event-deadline regression. Preserve the
RCNT restore fix; read-only pin differences are real machine-state differences.
Hash a versioned encoding of component names, lengths, and bytes in stable order.
Keep per-component hashes locally to isolate a mismatch.

Capture boundary 0 and every 60th tick through an observer. Store observations
tentatively, revoke those past a rewind boundary, and transmit only once the
actual settled boundary has reached them. The Phase 4 observer plus
`Session::checkpoint()` settled tick provides an existing pattern; do not use
`confirmed()` as the publication gate. Retain until comparison is acknowledged,
with a bounded history (initially 600 ticks). If history expires or settlement
advances without an expected diagnostic, report a monitoring failure and stop;
do not silently treat it as equality. Future hashes wait for local settlement.

The wrapper's existing CRC32 `Snapshot::digest` is useful supplemental telemetry,
but is partial, excludes battery saves, and may skip intermediate boundaries.
`digest_at == None` is not evidence of desync. SHA-256 equality likewise means
agreement over the defined diagnostic coverage, not proof of all hidden state or
protection against a cheating peer.

On a differing hash at the same settled boundary, stop further advances, send
`Desync`, preserve the earliest observed mismatch, manifest, final input log,
component hashes, recent sequence/frontier history, and local diagnostics under
ignored recordings. Do not automatically send state dumps or personal artifacts.
Use replay to isolate the first divergent tick; periodic hashes identify an
interval, not necessarily the originating tick.

**No automatic live state resynchronization in v1.** Loading the host's Link
behind Session would leave getgud queues/counters, prediction snapshots, observer
history, and audio bookkeeping inconsistent. Neither host authority nor complete
portable snapshot serialization has been established, and save-memory rollback
is incomplete. Fail visibly and restart from agreed initial data.

A later resync feature would need a demonstrated complete world image (all cores,
SIO/coordinator, FIFO/DMA, save storage, RTC/config), a mutually agreed settled
boundary and input suffix, integrity/ABI checks, and an explicit Session rebuild
or restore contract including counters and audio. Both peers must verify the
post-restore hash before a new generation starts. That is separate work, not
something inferred from `Link::load` existing.

## MVP client features and acceptance gates

The first client needs Host/Join with explicit IP and port, copyable invitation,
local ROM/save selection or fresh start, compatibility errors, transfer/readiness
status, fixed seat display, local keyboard/controller input, one owned framebuffer,
and Quit. Release all buttons on focus loss via the next real input sample.
Neither window focus nor closing a menu should silently pause only one emulator.
Defer coordinated pause, save-state hotkeys, rewind, speed changes, cheats, chat,
spectators, four-player support, and host migration.

Show RTT, rollback depth, unmatched queue depth, Waiting/Connected/Desynced status,
and last matched state-hash boundary. Provide an explicit diagnostic recording
option with private artifact paths. Local audio playback from only the owned
core is part of a usable client; shadow audio remains emulated but is discarded.
The existing manual frontend clears audio rings, so audible output is new frontend
work, not an already completed feature. Test the Session's rollback audio handling
through a bounded host-device queue.

Before calling the implementation internet-ready:

1. Preserve upstream and RCNT/timing regressions. Run two processes with the
   synthetic ROM and compare received/final rows against a direct baseline.
2. Inject loss, duplication, reordering, jitter, burst loss, asymmetric delays,
   send backpressure, slow application consumption, and blackholes. Verify no
   input omission/duplication, bounded memory, stall recovery without deadlock,
   and terminal-error handling. QUIC should absorb packet-level disorder; the
   session adapter must enforce its own message and phase contract.
3. Test wrong ROM/build/config/save identities, corrupt/truncated transfers,
   wrong seat, stale session data, oversized messages, and failure at every
   startup barrier. Neither peer may enter gameplay with an unaccepted world.
4. Repeat the validated SS2 recording through the real transport and compare
   settled diagnostics. Then conduct actual two-machine, port-forwarded play,
   including menus, combat, transitions, local audio, and both owned screens.
5. Deliberately test rollback across flash writes before durable save export.
   Separately validate asymmetric rendering before enabling shadow frameskip.
6. Force desync and disconnect; verify clear status, preserved originals/logs,
   no accidental live-state repair, and a clean fresh-session retry.

Measure input gap duration, correction depth, stall time, CPU, and memory as well
as average ping. Revisit QUIC DATAGRAM redundancy only if reliable-stream loss
recovery materially harms these measurements. Any datagram revision must retain
all missing inputs until application acknowledgment, repair gaps, deduplicate and
reorder before FIFO insertion, and remain bounded and congestion-aware. Latest
input only is never sufficient for this engine.

No source, dependency, emulator, ROM, or save changes were made for this proposal.
Existing dirty repository changes were left intact. No test results beyond the
previously documented validation are claimed here.
