# Portable internet harness implementation

2026-09-05. The user authorized the minimum playable implementation after the
transport design and explicitly deferred SS2 in-game save persistence validation
to their manual test. No automated SS2 menu/save sequence was run for this work.

## Delivered behavior

- `dist/SS2-Netplay/` and `dist/SS2-Netplay-Windows-x64.zip` contain the same
  Windows x64 client, with mGBA compiled into the EXE, app-local Microsoft runtime
  DLLs, source archive, licenses, and empty `ROM`/`save` folders.
- A single 720x480 minifb window shows Host, IP, Port, and Connect, then becomes
  the owned player's 3x GBA display. Keyboard controls are the same on both PCs.
  IP/port support Ctrl+V; host displays the forwarded UDP port (default 24872).
- ROM and own save paths are relative to the EXE, never the current directory.
  Exactly one `.gba` is required. Save naming uses its file stem. No initial save
  is required. A local character remains locally owned when hosting roles swap.
- Each peer runs both Cable GBA cores with fixed RTC, identical ROM/build hashes,
  ordered initial save images, and matching reset-state SHA-256. The guest
  initiates the direct-IP connection. No router, firewall, NAT, matchmaking, or
  relay configuration is performed automatically.
- Inputs flow through the existing rollback Session. The caller checks contiguous
  sequence IDs and button bits before FIFO insertion, bounds prediction/backlogs,
  and applies the existing Throttler at GBA's actual reference-frame cadence.
- The Phase 4 diagnostic observer hashes every 60th boundary, revokes speculative
  observations, and publishes only after actual settlement. Matching checkpoints
  govern save export. A desync or failed connection ends the session; there is no
  state replacement or live reconnect. The title reports rollback and last match.
- Local audio uses CPAL and mGBA's resampler; remote-core mixed samples are
  discarded. Both cores render internally to retain the Phase 4 tested policy.

## Deliberate simplifications from the proposal

Startup, input, and control use one bounded reliable QUIC stream. Bulk transfer
ends before gameplay; separate traffic streams and datagram redundancy remain
future optimization work. There is no invitation/password input,
coordinated pause, gamepad mapping, or advanced lobby.

## September 18: local delay and F1 telemetry

Setup now offers local present delay 1-4 (default 2), frozen when hosting or
connecting starts. The peer need not choose the same value. No wire protocol or
shared simulation configuration changes are needed for this local setting.

F1 toggles a white/red game overlay. The worker samples Quinn's transport RTT
every 250 ms using expendable local events; these are not new wire messages.
The game reports forward-advance FPS over 500 ms, correction counts (session and
rolling 60 seconds), recent/max depth, prediction, input queue, waiting status,
and last matching hash boundary. Rollback replays and window redraws do not count
as forward FPS. Rolling histories age out even while input advancement stalls.

The final recommendation is advisory: after five seconds and at least 60 recent
samples, take the 95th percentile of pre-advance unmatched lead plus the newly
sampled local input over ten seconds, subtract two tolerated prediction frames,
and clamp to 1-4. A result above four is labelled high lateness. This is a simple
heuristic, not an optimality guarantee; it does not change the selected delay.
All session statistics reset on a new connection.

The synthetic real-QUIC self-test exercises unequal 1/4-frame delays with 85 ms
artificial input delivery delay, checking both peers against the direct baseline
and requiring rollback corrections plus live RTT telemetry. No automated SS2
in-game save-persistence check is introduced.

The minimal UI uses first-connection trust with certificate pinning by host
IP/port on later connects. Host identity persists under `config`; guest identity
is not authenticated. The first compatible guest reaching the listener gets the
second seat. The first connection therefore lacks an independently verified host
identity. This tradeoff is explained in the portable readme. Generated keys,
personal saves, and runtime logs are excluded by the ZIP packaging allowlist.

## Savedata implementation, distinct from gameplay validation

The old Snapshot omitted battery bytes. A synthetic bus-write regression reproduced
that generic gap. Snapshot now includes `Option<Vec<u8>>` per core; load restores
the bytes after core state and before FIFO/DMA/SIO supplements. Initially undetected
save state relies on mGBA's deserialize path restoring the AUTODETECT type, which
the regression also covers. Savedata is included in the snapshot CRC as well.
`Core::bus_write_8` exposes the normal bus operation used by that ROM-free test.

The frontend observes the locally owned cartridge at diagnostic boundaries and
writes only peer-matched settled bytes. Equal bytes at two matched observations
(at least 60 simulated ticks apart) allow disk export. Updates use a synced temp
file and rename; the initial existing save is backed up once per session to
`.sav.bak`. Original save paths are never sent over the network. Closing during
an unconfirmed or incomplete save does not force-write speculative state.

This is implementation plus a generic snapshot regression, **not validation of
SS2's in-game flash-save sequence**. The user will create/save/reload a character
manually. The readme asks them to wait at least three seconds after the in-game
save finishes before closing. Cross-peer agreement alone cannot prove that every
possible save protocol path is correct.

## Validation evidence

- Release build completed on the documented Windows toolchain.
- Full mGBA rollback suite: 49 tests passed (including prior RCNT and inactive
  timing-event regressions plus the new synthetic savedata regression).
- App suite: 7 tests passed. Includes real QUIC ROM-mismatch rejection before
  session creation, invalid message/button parsing, FIFO-adapter skip/duplicate
  rejection, the previous deterministic tests, and the transport rollback test.
- Real QUIC synthetic smoke test: two endpoints and two replicated worlds,
  application-delayed inputs, confirmed input rows checked against their schedule,
  both peers matching a direct baseline at six checkpoints through frame 360,
  with nonzero correction events. Counts vary with host scheduling.
- Extracted portable ZIP tested with only Windows system directories in PATH and
  an unrelated working directory. The packaged executable passed the same test.
  PE import inspection found Windows system libraries plus VCRUNTIME140, supplied
  alongside the executable. No separate mGBA DLL or compiler runtime is needed.
- Setup pixels visually inspected; status wrapping fixed. This is not a claim
  of a two-computer WAN test, audible SS2 validation, or manual game-save success.

Build/test logs live in ignored `app/*-netplay.log`; portable smoke artifacts are
under ignored `recordings/portable-smoke-*`. User ROMs and personal saves were not
read, modified, or included in the package for these tests. Package creation
preserves any existing user data in the folder and uses an allowlist for the ZIP.

## September 21: Iroh Host/Join and direct fallback

The default setup now uses Host/Join connect codes with Iroh 1.2.0 and n0 public
relays. Direct Connect switches to the original numeric IP/port Quinn transport.
Both paths share the existing compatibility/save handshake, bounded frames,
ordered game messages, and rollback logic. No emulator/getgud changes were needed.
See [Iroh integration and validation](IROH_RELAY.md) for code format, optional
relay configuration, tests, and remaining two-network manual validation.
