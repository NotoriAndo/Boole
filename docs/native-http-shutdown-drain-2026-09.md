# Native HTTP shutdown drain — 2026-09-23

Status: **REPRODUCTION PREPARED — no runtime change or successful result yet.**

## Selected boundary

Static review found that native HTTP awaits Axum's complete graceful drain,
including incomplete request bodies and slow response receivers, before crossing
the existing durable-mutation shutdown barrier. Its ten-second request deadline
does not cover a response after the handler returns. Axum 0.8.9 spawns connection
tasks; merely dropping the outer server future does not demonstrate that those
tasks released connections and the node's state ownership.

The selected requirement is five seconds of graceful HTTP drain after normal
shutdown, followed by closure of this server's remaining client I/O. The server
must still wait for already admitted durable mutations to finish; this is not
permission to interrupt validation or disk publication. New mutation admission
and peer I/O must close when shutdown is requested, as before. No normal request,
body, connection, peer, validation or storage limit may be raised or bypassed.

## Reproduction and verification plan

1. Through the real public native server, admit eight partial `/native/chain`
   bodies and receive their actual HTTP 100 responses. Keep all clients open,
   request normal shutdown and require server completion within seven seconds
   (five-second drain plus two-second scheduling margin). The current node has
   no running ledger mutation. Verify actual client closure, listener reuse,
   immediate fresh state ownership and exact original canonical/manifest bytes.
   Always close test clients and join the server before reporting a RED result.
2. After that behavior is corrected, exercise an actual large block response
   with a deliberately small TCP receive/send window and an unread body. Do not
   infer socket cleanup merely from a completed server future. Reopen state while
   the old client remains held, then compare exact head/accounting/journal bytes.
3. Verify that prompt normal requests still drain without a mandatory five-second
   sleep, and that already admitted delayed mutations keep ownership/admission
   until completion even if their caller is disconnected. Replay exact IDs and
   accounting afterward. Existing peer shutdown and storage fencing stay intact.

Only disposable numeric-loopback sockets, development identities and temporary
state are in scope. No public listener, external transmission, operator key/fund,
model/VM/paid run, signature shortcut or release is involved. Keep each observed
failure, cause and corrective result below. A local pass does not close R1/R2/R3.
