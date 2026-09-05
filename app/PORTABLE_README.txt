SHINING SOUL II - PORTABLE ROLLBACK TEST HARNESS
Windows 10/11, 64-bit

GET STARTED
1. Extract the whole folder to a writable location on each computer. Do not run
   the EXE from inside the ZIP. No Rust, compiler, or mGBA installation is needed.
2. Put exactly one unzipped Shining Soul II .gba file in the ROM folder on each
   computer. Both players must use identical ROM bytes and this same EXE build.
   ROMs and personal saves are not included.
3. Launch SS2-Netplay.exe on both computers.
4. One player clicks HOST. The default port is 24872, and it can be changed in
   the Port field before hosting. Forward that UDP port on the host's router to
   the host computer. Allow the harness through Windows Firewall if prompted.
5. The other player enters the host's public IP and port and clicks CONNECT.
   On the same LAN use the host computer's LAN IP. 127.0.0.1 is only for testing
   two separate copies of the folder on the same computer.
6. Each setup window becomes that player's game screen. Select Multiplayer and
   create/select a character on both machines. Once both players show OK, player
   1 (the person who clicked Host) presses A to enter the game.

The host waits for the guest until the window is closed. Use IPv4 for internet
hosting in this build. Port forwarding, public IP discovery, and router setup are
manual. Networks behind carrier-grade NAT need an actual reachable public IP.

CONTROLS (same on both computers)
Arrows = D-pad       Z = A       X = B
Enter = Start       Right Shift = Select       C = L       V = R
Escape or close window = end this session and quit
Keyboard input releases when the game window loses focus. There is no controller
mapping UI yet. Select the in-game language and menus normally.

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
Connect again to restart from the saved cartridges. There is no live reconnect,
host migration, automatic state replacement, matchmaking, or NAT traversal.

Audio uses the default Windows output device. If unavailable, the game continues
silently and the title says "sound unavailable". Audio-device changes require
restarting the session. Detailed session/error and final input logs are in logs.

CONNECTION IDENTITY
The first guest connection trusts the host certificate at the IP/port you entered;
later connections pin that certificate in config/pins. Host identity is retained
in config/host-identity.bin. This simple test UI has no password/invitation check:
the first compatible guest to reach a listening host occupies the second seat.
Use it with the friend you expect to connect. If the host identity changes, verify
the host before removing the corresponding pin file and retrying. Do not include
your generated config, save, or logs folders when sharing a fresh copy publicly.

PORTABILITY / DIAGNOSTICS
Keep the included DLLs beside the EXE. They are the Microsoft C/C++ runtime files;
other loaded system libraries ship with Windows 10/11. Sources.zip contains the
current emulator, wrappers, rollback engine, and harness source. licenses contains
the upstream notices and dependency license texts. Source test ROM/save fixtures
are omitted from Sources.zip; no commercial ROM or personal save is included.

Optional ROM-free self-test: run SS2-Netplay.exe --self-test from a command prompt.
It opens no game window and writes self-test.txt when finished. It starts two real
local QUIC endpoints, delays inputs, and compares both rollback worlds with a
direct baseline. This does not replace the manual internet gameplay test.
