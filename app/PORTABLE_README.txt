SHINING SOUL II - PORTABLE ROLLBACK TEST HARNESS
Windows 10/11, 64-bit

GET STARTED
1. Extract the whole folder to a writable location on each computer. Do not run
   the EXE from inside the ZIP. No Rust, compiler, or mGBA installation is needed.
2. Put exactly one unzipped Shining Soul II .gba file in the ROM folder on each
   computer. Both players must use identical ROM bytes and this same EXE build.
   ROMs and personal saves are not included.
3. Launch SS2-Netplay.exe on both computers.
   Choose local delay 1, 2, 3, or 4 frames before Host/Join (default 2).
   Higher delay trades response speed for fewer visible prediction corrections.
   Each player chooses independently; the setting stays fixed during the session.
4. One player clicks HOST, waits for a connect code, and clicks COPY CODE.
   Send the full code to your friend. Hosting generates a fresh code each time.
   Iroh uses its public relays for NAT traversal; no VPS or router setup is needed.
5. The other player pastes the code into CONNECT CODE with Ctrl+V and clicks JOIN.
   Allow the harness through Windows Firewall if prompted. Click CANCEL to stop
   listening/connecting and return to setup.
6. Each setup window becomes that player's game screen. Select Multiplayer and
   create/select a character on both machines. Once both players show OK, player
   1 (the person who clicked Host) presses A to enter the game.

Iroh attempts a direct peer connection and relays encrypted traffic when needed.
Public relay availability and network restrictions can affect connectivity.

DIRECT CONNECT FALLBACK
Click DIRECT CONNECT to switch to numeric IP/port setup (default UDP port 24872).
One player clicks HOST and forwards that UDP port when needed. The other enters
that host's public IP and port and clicks CONNECT. On a LAN, use the host's LAN IP;
127.0.0.1 is for two folder copies on one computer. Click USE CODE to return.
Direct mode has no automatic NAT traversal; carrier-grade NAT may prevent hosting.

DEFAULT CONTROLS (customizable independently on each computer)
Arrows = D-pad       Z = A       X = B
Enter = Start       Right Shift = Select       C = L       V = R
F1 = toggle live statistics, volume slider, and input mapper.
Escape = exit confirmation; click Yes with the mouse to quit. Escape cancels.
  The session keeps running while the prompt is open; game buttons are released.
  Closing the window ends the session immediately.
Keyboard and controller input releases when the game window loses focus.
Select the in-game language and menus normally.

LOCAL SETTINGS (F1 during gameplay)
Drag the volume slider from 0% (mute) to 100%. Only local playback changes.
Click a GBA button's mapping row, then press a new keyboard key or XInput
controller button, move a stick in one direction, or pull a trigger. The row
shows its current binding. Each GBA button has one binding; remapping replaces
the previous keyboard/controller input. F1 and Escape are reserved shortcuts.
Any connected XInput controller can be mapped. Bindings show its slot (1-4);
remap if Windows assigns a different slot after reconnecting. New controllers
are detected within about a second. Non-XInput devices need an XInput
compatibility layer. Sticks use a fixed dead zone and triggers a fixed threshold.
Release an already-held input and press it again to capture it. Escape cancels
capture. The captured input must be released before it can affect gameplay.
The game and connection NEVER pause for settings. During binding capture your
local buttons are released; with the overlay merely open you can keep playing.
Volume and mappings save automatically to netplay-settings.txt beside the EXE.
File writes run in the background; the panel reports save errors. Keep the folder
writable. Settings are local and survive restarts and portable EXE updates.

LIVE STATISTICS
Ping is QUIC's smoothed round-trip time, not one-way input travel time.
Game FPS counts forward session frames over 500 ms, not redraws or rollback work.
Rollback counts count correction events; depth is in frames. Last depth is the
most recent correction in the past minute, and max is the session maximum.
Recommended delay is advisory only: after a five-second warmup, use the last
10 seconds' 95th-percentile input lead minus two frames of allowed prediction,
clamped to 1-4. This heuristic does not guarantee smooth play or change settings.
"High lateness" means that estimate exceeds four frames. Connection spikes or
slow emulation can still cause waiting. Statistics reset on each new connection.

SAVES
Start with an empty save folder. Creating a character and using the game's own
save function creates/updates save/<ROM filename without .gba>.sav automatically.
Each computer owns its local save, regardless of which person hosts next time.
The original local save is loaded automatically at the next connection. Keep the
same ROM filename when restarting if you want the same automatically loaded save.

Only the locally owned cartridge is written to this computer's save folder.
Both starting cartridge saves are shared with the other connected player because
both machines simulate both GBAs. In-game save changes are written after their
settled state agrees between peers and has stayed stable for one second. Wait
at least THREE SECONDS after an in-game save finishes before closing the game.
The title bar shows "save written" after a disk update. An existing save is backed
up once per session to .sav.bak; updates use a temporary file and replacement.
Do not run two EXEs from the same folder simultaneously: use separate folder
copies so they do not share one local character file.

The real game's save/reload behavior is intentionally left for the user's manual
check. The ROM-free automated test exercises networking and rollback, not SS2's
save menus. This is an experimental gameplay test build.

HOW IT WORKS / STATUS
mGBA is compiled into SS2-Netplay.exe. There is no separate mGBA.exe to launch.
Each computer emulates both linked GBAs but displays/plays sound only for its
owned player. Network messages carry joypad inputs and state checks, not individual
link-cable transfers, screens, or audio. The title shows player number, connection
status, recent rollback depth, and the last matching settled state boundary.

There is a small local presentation delay and a bounded prediction window. During
a network stall, "Waiting for player" is expected; the session can recover while
the connection remains alive. A timeout or desync ends the session. Click Host or
Join again to restart from the saved cartridges. There is no live reconnect,
host migration, automatic state replacement, or matchmaking.

Audio uses the default Windows output device. If unavailable, the game continues
silently and the title says "sound unavailable". Audio-device changes require
restarting the session. Detailed session/error and final input logs are in logs.

CONNECTION IDENTITY
Connect codes contain a fresh host public key and connection addresses. Iroh
checks that key during the encrypted connection. Share the complete code through
an authentic conversation with your friend; anyone with it can attempt to join.
Codes are copy/paste invitations, not short numeric room IDs. The first compatible
peer gets the guest seat. Relays carry encrypted packets, not unencrypted saves.

Direct IP mode retains first-connection certificate trust and later pinning by
IP/port. A first direct connection does not independently verify host identity.
After a legitimate host-key change, verify with the host before removing the
matching pin under config/pins. Do not share generated config, saves, or logs.

No relay configuration is required. Advanced users may put an HTTPS relay origin
in config/relay-url.txt before hosting; guests obtain it from the code. Remove
that file to return to the public relays. There is no silent public-relay fallback
when a custom relay has been explicitly configured.

PORTABILITY / DIAGNOSTICS
This build preserves video-memory contention timing across rollback restores,
fixing the reproduced shop-session desync. Both players must use the same EXE.
Automatic recovery is not implemented; a desync still preserves evidence and
stops the session.

Each connection creates logs/capture-<time>-p<seat>. It contains both initial
save images (or explicit absent markers), ROM/build identity, timed receive and
advance events, and component hashes at settled 60-frame checkpoints. On a
handled session failure it also writes the last matching and pending checkpoint
component bytes. These are diagnostic bytes, not loadable mGBA savestates.
If another failure occurs, keep the capture folders AND session text logs from
BOTH PCs, plus the exact EXE used. Captures contain private save data; share them
only with whoever is investigating. No ROM is copied. A native process crash
cannot run the failure dumper; regularly flushed hashes/events remain available.

Keep the included DLLs beside the EXE. They are the Microsoft C/C++ runtime files;
other loaded system libraries ship with Windows 10/11. Sources.zip contains the
current emulator, wrappers, rollback engine, and harness source. licenses contains
the upstream notices and dependency license texts. Source test ROM/save fixtures
are omitted from Sources.zip; no commercial ROM or personal save is included.

Optional ROM-free self-test: run SS2-Netplay.exe --self-test from a command prompt.
It opens no game window and writes self-test.txt when finished. It starts two real
local QUIC endpoints, delays inputs, and compares both rollback worlds with a
direct baseline. This does not replace the manual internet gameplay test.

Optional online Iroh test: SS2-Netplay.exe --self-test-iroh writes self-test-iroh.txt.
It uses the configured/public relay, generated connect code, and the same synthetic
rollback comparison. This contacts external relays but sends no personal ROM/save.
