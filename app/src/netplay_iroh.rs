//! Self-contained invitations: no public endpoint discovery or code database.
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::mpsc::{Receiver, SyncSender},
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use iroh::{
    endpoint::{presets, QuicTransportConfig},
    Endpoint, EndpointAddr, RelayMode, RelayUrl, TransportAddr,
};
use serde::{Deserialize, Serialize};

use crate::{
    netplay_transport::{Connection, RecvStream, SendStream},
    netplay_wire::{self, Event, Hello, Message},
};

const ALPN: &[u8] = b"ss2/netplay/1";
const PREFIX: &str = "ss2-1:";
pub const MAX_CODE_LEN: usize = 2048;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Invitation {
    id: String,
    relay: Option<String>,
    ips: Vec<SocketAddr>,
}

fn encode(addr: &EndpointAddr) -> Result<String, String> {
    let invitation = Invitation {
        id: addr.id.to_string(),
        relay: addr.relay_urls().next().map(ToString::to_string),
        ips: addr.ip_addrs().copied().take(8).collect(),
    };
    let bytes = serde_json::to_vec(&invitation).map_err(|e| e.to_string())?;
    let code = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    if code.len() > MAX_CODE_LEN {
        return Err("Connection address is too large for an invitation".into());
    }
    Ok(code)
}

fn relay_url(value: &str) -> Result<RelayUrl, String> {
    let url: RelayUrl = value
        .parse()
        .map_err(|e| format!("Invalid relay URL: {e}"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Relay must be an HTTPS origin, such as https://relay.example.com".into());
    }
    Ok(url)
}

fn decode(code: &str) -> Result<EndpointAddr, String> {
    let code = code.trim();
    if code.len() > MAX_CODE_LEN {
        return Err("Connect code is too long".into());
    }
    let payload = code
        .strip_prefix(PREFIX)
        .ok_or("Expected an ss2-1: connect code from Host")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "Invalid connect-code encoding")?;
    let invitation: Invitation =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid connect-code contents")?;
    if invitation.ips.len() > 8
        || invitation
            .ips
            .iter()
            .any(|a| a.port() == 0 || a.ip().is_unspecified() || a.ip().is_multicast())
    {
        return Err("Invalid connect-code IP addresses".into());
    }
    let id = invitation
        .id
        .parse()
        .map_err(|_| "Invalid host identity in connect code")?;
    let mut addresses: Vec<_> = invitation.ips.into_iter().map(TransportAddr::Ip).collect();
    if let Some(relay) = invitation.relay {
        addresses.push(TransportAddr::Relay(relay_url(&relay)?));
    }
    if addresses.is_empty() {
        return Err("Connect code has no reachable addresses".into());
    }
    Ok(EndpointAddr::from_parts(id, addresses))
}

pub fn validate_code(code: &str) -> Result<(), String> {
    decode(code).map(|_| ())
}

fn configured_relay(config_dir: &std::path::Path) -> Result<RelayMode, String> {
    let path = config_dir.join("relay-url.txt");
    match fs::read_to_string(&path) {
        Ok(value) => Ok(RelayMode::Custom(
            [relay_url(value.trim())?].into_iter().collect(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RelayMode::Default),
        Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
    }
}

async fn bind(relay_mode: RelayMode, allow_ip: bool) -> Result<Endpoint, String> {
    let transport = QuicTransportConfig::builder()
        .max_concurrent_uni_streams(0u32.into())
        .max_concurrent_bidi_streams(1u32.into())
        .stream_receive_window((256u32 * 1024).into())
        .receive_window((512u32 * 1024).into())
        .send_window(512 * 1024)
        .keep_alive_interval(Duration::from_secs(1))
        .max_idle_timeout(Some(Duration::from_secs(10).try_into().unwrap()))
        .build();
    let builder = Endpoint::builder(presets::Minimal)
        .relay_mode(relay_mode)
        .alpns(vec![ALPN.to_vec()])
        .transport_config(transport);
    let builder = if allow_ip {
        builder
    } else {
        builder.clear_ip_transports()
    };
    builder
        .bind()
        .await
        .map_err(|e| format!("Cannot start Iroh endpoint: {e}"))
}

pub(crate) async fn run_worker(
    code: Option<String>,
    hello: Hello,
    config_dir: PathBuf,
    command_rx: Receiver<Message>,
    event_tx: SyncSender<Event>,
) -> Result<(), String> {
    hello.validate()?;
    let peer = code.as_deref().map(decode).transpose()?;
    // The invitation carries the host's relay, so the guest needs no configuration.
    let relay_mode = match &peer {
        Some(addr) => RelayMode::Custom(addr.relay_urls().cloned().collect()),
        None => configured_relay(&config_dir)?,
    };
    netplay_wire::report_status(&event_tx, "Connecting to relay...".into());
    let endpoint = bind(relay_mode, true).await?;
    tokio::time::timeout(TIMEOUT, endpoint.online())
        .await
        .map_err(|_| "Relay is unavailable. Check config/relay-url.txt or use Direct connect.")?;
    let result = run_endpoint(&endpoint, peer, hello, command_rx, event_tx).await;
    endpoint.close().await;
    result
}

async fn run_endpoint(
    endpoint: &Endpoint,
    peer: Option<EndpointAddr>,
    hello: Hello,
    command_rx: Receiver<Message>,
    event_tx: SyncSender<Event>,
) -> Result<(), String> {
    let host = peer.is_none();
    let connection = if let Some(peer) = peer {
        netplay_wire::report_status(&event_tx, "Connecting to host...".into());
        tokio::time::timeout(TIMEOUT, endpoint.connect(peer, ALPN))
            .await
            .map_err(|_| "Timed out connecting. Ask the host for a fresh connect code.")?
            .map_err(|e| format!("Iroh connection failed: {e}"))?
    } else {
        let code = encode(&endpoint.addr())?;
        event_tx
            .try_send(Event::ConnectCode(code))
            .map_err(|_| "Cannot deliver host invitation")?;
        netplay_wire::report_status(
            &event_tx,
            "Listening. Copy your connect code and send it to your friend.".into(),
        );
        let incoming = endpoint.accept().await.ok_or("Iroh listener stopped")?;
        tokio::time::timeout(TIMEOUT, incoming)
            .await
            .map_err(|_| "Host connection handshake timed out")?
            .map_err(|e| format!("Iroh host handshake failed: {e}"))?
    };
    let (send, recv) = tokio::time::timeout(TIMEOUT, async {
        if host {
            connection.accept_bi().await
        } else {
            connection.open_bi().await
        }
    })
    .await
    .map_err(|_| "Timed out opening game stream")?
    .map_err(|e| format!("Cannot open game stream: {e}"))?;
    let mut send = SendStream::Iroh(send);
    let mut recv = RecvStream::Iroh(recv);
    let remote = tokio::time::timeout(TIMEOUT, async {
        if host {
            netplay_wire::handshake_host(&mut send, &mut recv, &hello).await
        } else {
            netplay_wire::handshake_guest(&mut send, &mut recv, &hello).await
        }
    })
    .await
    .map_err(|_| "Startup data exchange timed out")??;
    let saves = if host {
        [hello.save, remote.save]
    } else {
        [remote.save, hello.save]
    };
    netplay_wire::report_connected(&event_tx, usize::from(!host), saves)?;
    netplay_wire::run_stream(
        Connection::Iroh(connection),
        send,
        recv,
        command_rx,
        event_tx,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn invitations_validate_identity_addresses_version_and_size() {
        let id = iroh::SecretKey::generate().public();
        let addr = EndpointAddr::from_parts(
            id,
            [
                TransportAddr::Ip("127.0.0.1:1234".parse().unwrap()),
                TransportAddr::Relay(relay_url("https://gentlebox.org:8443").unwrap()),
            ],
        );
        let code = encode(&addr).unwrap();
        assert_eq!(decode(&format!("  {code}\n")).unwrap(), addr);
        for bad in ["", "ss2-2:abc", "ss2-1:!", &"a".repeat(MAX_CODE_LEN + 1)] {
            assert!(decode(bad).is_err());
        }
        for url in [
            "http://gentlebox.org",
            "https://gentlebox.org/relay",
            "https://user:pass@gentlebox.org",
            "https://gentlebox.org/?token=x",
        ] {
            assert!(relay_url(url).is_err(), "{url}");
        }
        let mut bad = Invitation {
            id: id.to_string(),
            relay: None,
            ips: vec![],
        };
        let encode_bad = |value: &Invitation| {
            format!(
                "{PREFIX}{}",
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).unwrap())
            )
        };
        assert!(decode(&encode_bad(&bad)).is_err());
        bad.ips.push("0.0.0.0:0".parse().unwrap());
        assert!(decode(&encode_bad(&bad)).is_err());
        bad.ips = vec!["127.0.0.1:1234".parse().unwrap(); 9];
        assert!(decode(&encode_bad(&bad)).is_err());
        bad.ips.truncate(1);
        bad.id = "not-a-key".into();
        assert!(decode(&encode_bad(&bad)).is_err());
    }

    async fn next_event(rx: &Receiver<Event>) -> Event {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match rx.try_recv() {
                    Ok(Event::Status(_)) => {}
                    Ok(event) => return event,
                    Err(mpsc::TryRecvError::Empty) => {
                        tokio::time::sleep(Duration::from_millis(2)).await
                    }
                    Err(e) => panic!("worker stopped: {e}"),
                }
            }
        })
        .await
        .expect("worker event timeout")
    }

    #[tokio::test]
    async fn real_iroh_invitation_handshake_save_transfer_and_messages() {
        let host = bind(RelayMode::Disabled, true).await.unwrap();
        let guest = bind(RelayMode::Disabled, true).await.unwrap();
        tokio::task::LocalSet::new()
            .run_until(exchange(host, guest))
            .await;
    }

    #[tokio::test]
    async fn incompatible_iroh_peer_is_rejected_before_connection_event() {
        let host = bind(RelayMode::Disabled, true).await.unwrap();
        let guest = bind(RelayMode::Disabled, true).await.unwrap();
        let hello = Hello {
            rom_hash: [1; 32],
            build_hash: [2; 32],
            save: None,
        };
        let mismatch = Hello {
            rom_hash: [3; 32],
            ..hello.clone()
        };
        let (_commands, command_rx) = mpsc::sync_channel(256);
        let (events, event_rx) = mpsc::sync_channel(256);
        let results = tokio::time::timeout(TIMEOUT, async {
            tokio::join!(
                run_endpoint(&host, None, hello, command_rx, events),
                async {
                    let connection = guest.connect(host.addr(), ALPN).await.unwrap();
                    let (send, recv) = connection.open_bi().await.unwrap();
                    netplay_wire::handshake_guest(
                        &mut SendStream::Iroh(send),
                        &mut RecvStream::Iroh(recv),
                        &mismatch,
                    )
                    .await
                }
            )
        })
        .await
        .unwrap();
        assert!(results.0.unwrap_err().contains("ROM hash mismatch"));
        assert!(results.1.unwrap_err().contains("ROM hash mismatch"));
        assert!(!event_rx
            .try_iter()
            .any(|event| matches!(event, Event::Connected { .. })));
        host.close().await;
        guest.close().await;
    }

    #[tokio::test]
    #[ignore = "requires internet and the public Iroh relays"]
    async fn real_iroh_public_relay_only() {
        let host = bind(RelayMode::Default, false).await.unwrap();
        let guest = bind(RelayMode::Default, false).await.unwrap();
        tokio::time::timeout(TIMEOUT, host.online()).await.unwrap();
        tokio::time::timeout(TIMEOUT, guest.online()).await.unwrap();
        assert!(host.addr().ip_addrs().next().is_none());
        assert!(guest.addr().ip_addrs().next().is_none());
        tokio::task::LocalSet::new()
            .run_until(exchange(host, guest))
            .await;
    }

    async fn exchange(host: Endpoint, guest: Endpoint) {
        let (host_tx, host_commands) = mpsc::sync_channel(256);
        let (host_events, host_rx) = mpsc::sync_channel(256);
        let (guest_tx, guest_commands) = mpsc::sync_channel(256);
        let (guest_events, guest_rx) = mpsc::sync_channel(256);
        let hello = Hello {
            rom_hash: [1; 32],
            build_hash: [2; 32],
            save: Some(vec![3; 65536]),
        };
        let guest_hello = Hello {
            save: Some(vec![4; 65536]),
            ..hello.clone()
        };
        let saves = [hello.save.clone(), guest_hello.save.clone()];
        let host_endpoint = host.clone();
        let host_task = tokio::task::spawn_local(async move {
            run_endpoint(&host_endpoint, None, hello, host_commands, host_events).await
        });
        let Event::ConnectCode(code) = next_event(&host_rx).await else {
            panic!("expected invitation")
        };
        let peer = decode(&code).unwrap();
        let guest_endpoint = guest.clone();
        let guest_task = tokio::task::spawn_local(async move {
            run_endpoint(
                &guest_endpoint,
                Some(peer),
                guest_hello,
                guest_commands,
                guest_events,
            )
            .await
        });
        assert_eq!(
            next_event(&host_rx).await,
            Event::Connected {
                local_player: 0,
                saves: saves.clone()
            }
        );
        assert_eq!(
            next_event(&guest_rx).await,
            Event::Connected {
                local_player: 1,
                saves
            }
        );
        let messages = [
            Message::Ready([8; 32]),
            Message::Input {
                tick: 7,
                keys: 3,
                advantage: -2,
            },
            Message::Hash {
                tick: 60,
                hash: [9; 32],
            },
        ];
        for message in &messages {
            host_tx.send(message.clone()).unwrap();
            guest_tx.send(message.clone()).unwrap();
        }
        for rx in [&host_rx, &guest_rx] {
            for expected in &messages {
                loop {
                    match next_event(rx).await {
                        Event::Rtt(rtt) => assert!(rtt > Duration::ZERO),
                        Event::Message(message) => {
                            assert_eq!(&message, expected);
                            break;
                        }
                        event => panic!("unexpected {event:?}"),
                    }
                }
            }
        }
        host_tx.send(Message::Leave).unwrap();
        loop {
            match next_event(&guest_rx).await {
                Event::Rtt(_) => {}
                Event::Message(Message::Leave) => break,
                event => panic!("unexpected {event:?}"),
            }
        }
        host_task.await.unwrap().unwrap();
        drop(guest_tx);
        let _ = guest_task.await.unwrap();
        host.close().await;
        guest.close().await;
    }
}
