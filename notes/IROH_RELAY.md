# Iroh relay integration

Status: implementation note for the current Windows netplay client. The app
uses `iroh = 1.2.0` and does not require a project-owned relay server.

## Current default

When the host does not have a relay override, the client builds an endpoint
with `presets::Minimal` and `RelayMode::Default`:

```rust
Endpoint::builder(presets::Minimal)
    .relay_mode(RelayMode::Default)
    .alpns(vec![ALPN.to_vec()])
    .transport_config(transport)
    .bind()
    .await?;
```

`Minimal` selects the required TLS crypto provider and does not install an
address lookup service. `RelayMode::Default` selects the public production
relay map maintained by n0. These are separate concerns: the endpoint does
not publish or resolve peer identities through DNS/Pkarr, but it still uses
the configured relays for registration, encrypted forwarding, and NAT
traversal. A relay path may later migrate to a direct path.

The client waits for `Endpoint::online()` before creating the invitation and
wraps that wait in its 30-second timeout. `online()` means that a relay
handshake has completed; it has no timeout of its own. If public relay access,
DNS, or TLS is unavailable, the Iroh attempt fails with the relay-unavailable
status. The setup screen retains **DIRECT CONNECT** mode as the fallback; that mode
uses the existing direct Quinn transport and does not depend on Iroh relays.

Public relays are shared infrastructure. Iroh's endpoint traffic remains
end-to-end encrypted, but a public relay can observe connection metadata such
as peer IP addresses, connection times, and relayed volume. This is suitable
for the current prototype; dedicated infrastructure can be considered later
if uptime, policy, or metadata requirements change.

## Override file

The host may override the public map with one relay origin in:

```text
<directory containing SS2-Netplay>/config/relay-url.txt
```

Example:

```text
https://relay.example.com/
```

The file is trimmed and must contain an HTTPS origin with a host and root path
(`path == "/"`). Userinfo, query strings, fragments, and an `/relay` path are
rejected. A port is allowed, so `https://gentlebox.org:8443/` is a valid
origin when its certificate covers `gentlebox.org`.

If the file is absent, `RelayMode::Default` is used. When joining with a
connect code, the relay carried by the host's invitation takes precedence over
the guest's local file; the guest therefore does not need the same override.
An invalid or unreadable existing file is reported as a configuration error.

## Invitation and identity behavior

The current app uses its own `ss2-1:` URL-safe base64 JSON invitation rather
than `iroh-tickets`. A host invitation contains the host endpoint ID, the
selected home relay URL, and up to eight current direct socket addresses. The
host creates a fresh random Iroh identity for each endpoint because it does
not persist an Iroh secret key. Each new Host attempt therefore needs a new
connect code; there is no public endpoint directory, matchmaking service, or
code database.

The guest reconstructs the host `EndpointAddr` from the invitation and calls
`Endpoint::connect`. Iroh can select the relay or a direct address from that
set. The app's invitation validation is intentionally separate from the
Iroh endpoint authentication and does not add a password or account system.

## Relay URL and reverse-proxy constraint

The relay URL is an origin, not the WebSocket route. Iroh 1.2.0's relay
client sets the WebSocket request path to `/relay`; the server also exposes
`/ping`, `/generate_204`, and `/healthz` at the host root. A URL prefix such
as `https://example.com/relay/` is not a supported mount point. If a future
relay is placed behind Nginx on an existing domain, keep the advertised URL
as `https://example.com/` (or `https://example.com:8443/`) and proxy the
root routes, including WebSocket upgrade for `/relay`. Do not put `/relay`
in `config/relay-url.txt`.

Iroh's `RelayConfig` created from a relay URL also enables QUIC address
discovery by default on UDP port 7842. A self-hosted relay that wants the
usual direct-path assistance must expose UDP 7842 and provide TLS for that
QUIC listener; an HTTP/WebSocket-only reverse proxy can still provide relay
forwarding but may reduce NAT traversal effectiveness. This is an optional
future deployment concern, not a requirement for the current public-relay
setup.

## Version and source references

- The app pins [`iroh = "=1.2.0"`](../app/Cargo.toml) with
  `default-features = false`, `tls-ring`, and `portmapper`. The published
  crate declares MSRV Rust 1.91 and edition 2024 in its normalized manifest:
  [iroh 1.2.0 Cargo.toml](https://docs.rs/crate/iroh/1.2.0/source/Cargo.toml).
- [Builder API](https://docs.rs/iroh/1.2.0/iroh/endpoint/struct.Builder.html)
  documents `Minimal`, relay mode, and address lookup configuration.
- [Minimal preset](https://docs.rs/iroh/1.2.0/iroh/endpoint/presets/struct.Minimal.html)
  documents that only mandatory crypto setup is applied.
- [Endpoint source](https://docs.rs/iroh/1.2.0/src/iroh/endpoint.rs.html)
  documents `RelayMode` and the semantics of `Endpoint::online()`.
- [iroh-relay client source](https://docs.rs/iroh-relay/1.2.0/src/iroh_relay/client.rs.html)
  shows the client assigning `/relay` before the WebSocket connection.
- [iroh-relay server source](https://docs.rs/iroh-relay/1.2.0/src/iroh_relay/server.rs.html)
  lists the root relay and probe routes.
- [Public relay security and privacy](https://docs.iroh.computer/deployment/security-privacy)
  describes the encryption and metadata visible to shared relays.
- The implementation that this note describes is in
  [`app/src/netplay_iroh.rs`](../app/src/netplay_iroh.rs) and
  [`app/src/netplay_transport.rs`](../app/src/netplay_transport.rs).

## Validation (2026-09-21)

- App regression suite: 35 library tests and one local-link binary test passed.
  Six UI tests passed after the final clipboard/scroll review, including a new
  scroll-position regression. The explicit external-network test remains ignored
  in the ordinary suite.
- The external `real_iroh_public_relay_only` test was run separately and passed:
  both endpoints had direct IP transports disabled, and exchanged 64 KiB saves,
  Ready/Input/Hash messages, and Leave through public relays.
- Local Iroh sockets passed invitation, handshake/save transfer, message ordering,
  and incompatible-ROM rejection tests. Listener cancellation is also covered.
- Release `--self-test-iroh` passed six settled checkpoints through frame 360
  against the direct synthetic-ROM baseline, with 85 ms artificial input delay,
  unequal local delays, rollback corrections, and live RTT on both peers.
- Setup, generated-code listening, and Direct Connect previews were rendered and
  visually inspected at 720x480.

These checks do not establish hole punching between two separate residential
networks or validate actual SS2 gameplay over Iroh. That remains a two-computer
manual test. Existing generated mGBA binding warnings remain unchanged.
