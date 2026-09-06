# Diagnostic build

The reported real_log session stopped on a settled hash mismatch at boundary
86220; the prior matched boundary was 86160. Its old aggregate-only evidence
cannot identify the divergent component. No emulator correctness fix is claimed.

Each GUI connection now creates a unique private logs/capture-* directory. Its
manifest identifies ROM and executable SHA-256, fixed RTC, seat, player count,
initial aggregate hash and ordered startup save hashes or absent markers. Copies
of startup saves are immutable capture fixtures, separate from playable saves.
ROMs are never copied. Preserve the exact executable alongside collected evidence.

events.txt records microsecond timestamps, each incoming message with local
frontier, and each advance call with its keys and frontier. This preserves input
arrival/advance ordering for a future rollback replay tool. Existing session logs
still contain confirmed player-indexed input rows. No replay tool is added here.

At every settled 60-frame boundary, events.txt records component names, byte
lengths and SHA-256. The aggregate hash algorithm and wire protocol are unchanged;
compare component hashes from both peers offline. No diagnostic data is uploaded.
The observer collects the components once and derives the existing aggregate from
those exact bytes. A rewind revokes provisional observations as before.

On handled failure, numeric .bin files and an index preserve last peer-matched,
pending settled, and provisional observed checkpoints in distinctly named folders.
These canonical diagnostic components are not general-purpose loadable savestates.
The stop event includes frontier, settled boundary, matched boundary, next remote
input sequence, rollback count and maximum depth. Pending aggregate hashes include
the remote hash when available. The disconnecting peer also attempts a capture;
it may have no remote hash for the final boundary. Collect both PCs' folders.

Only the last matched and bounded pending checkpoints retain bytes in RAM; hash
and event logs grow with session duration. Disk errors stop play visibly and capture
write errors are reported in the session log. Hash checkpoints flush event buffers
roughly once per emulated second. Hard/native crashes cannot execute the failure
dumper and may lose the latest buffered events. No live state resynchronization,
transport change, or automated in-game save persistence check is included.

Validation: forced mismatch regression verifies exact boundary bytes and startup
absent markers. Existing synthetic QUIC rollback/direct-baseline tests remain the
transport/session regression; manual internet gameplay remains necessary.
