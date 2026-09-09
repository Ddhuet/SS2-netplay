# Recovering without discarding desync evidence

Automatic recovery is a usability feature, not a reason to suppress a desync.
Keep the diagnostic capture and report the recovery in the session log/UI. A
development setting can stop immediately; a player setting can attempt a bounded
recovery. Neither requires making the user repeat a long playthrough to motivate
correctness fixes.

The current MVP only exchanges inputs and hashes. Its local Snapshot is an
in-process representation with private supplementary state, not a validated wire
format. The canonical diagnostic component dumps are not loadable snapshots.
Session also has input/prediction queues and boundary bookkeeping which cannot
be left running unchanged while replacing the Link underneath it. Consequently,
automatic recovery needs implementation and tests; it is not a safe one-line
change from stop to continue.

## Proposed bounded recovery protocol (not implemented)

1. Preserve both local failure captures. Pause normal advances and save-file
   promotion. Agree on a recovery generation and a precise emulation boundary.
   Do not mix speculative presentation frames with settled machine state.
2. Pick a declared source of truth. A host-authoritative policy gives a consistent
   answer but does not prove the host was correct. For this captured incident the
   host matches the direct baseline; that is not a universal guarantee. A retained
   last-agreed checkpoint is an alternative, followed by replay of agreed inputs.
3. Send a versioned, bounded full linked-world checkpoint: both GBA cores, their
   cartridge memory, FIFO/DMA supplements, link/coordinator/driver state, fixed
   configuration and boundary identity. Use reliable framed chunks with exact
   lengths and a payload hash. Never copy raw host pointers. Validate ROM/build/
   schema identity and every length/index before loading. The existing 16 KiB
   message limit requires explicit chunking for these much larger payloads.
4. Restore into a fresh/rebased session, retaining seat ownership. Reset/reconcile
   prediction queues, input sequence coordinates, held buttons and audio/video
   presentation. Tag later traffic with the new generation so old messages cannot
   enter the recovered simulation. Handle already-received inputs explicitly.
5. Both peers compute and acknowledge an identical restored state hash before
   resuming. Continue checkpoint checks and preserve a recovery record. Do not
   overwrite playable save files merely because a checkpoint was downloaded;
   retain the existing peer-matched settled save promotion policy.
6. Stop with the diagnostic location if validation fails or another desync occurs
   repeatedly in a small window. Repeatedly loading the same incomplete state
   representation can reproduce the same underlying restore bug indefinitely.

Test with injected desyncs, truncated/oversized chunks, stale-generation inputs,
disconnect during recovery, held buttons, and both seat assignments. Recovery
must preserve failure evidence and must not silently declare the cause fixed.

The immediate correctness work remains: isolate the earliest divergent operation
in the recorded guest rollback execution, add a focused failing regression, patch
the demonstrated state/restore problem, and verify both the reproducer and the
existing deterministic/rollback tests. No GBA link transactions need to be sent
over the network for either that fix or this proposed recovery protocol.
