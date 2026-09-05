//! Small direct-IP QUIC transport for the native two-player MVP.
//!
//! The emulation/session layer owns the meaning of Message values. This
//! module only performs the startup compatibility exchange and moves those
//! values over one encrypted, ordered, reliable QUIC bidirectional stream.
//! Using one stream for startup, input, and control is an intentional MVP
//! simplification. It is a documented deviation from the eventual protocol
//! plan's separate streams; changing it later must not change the message
//! semantics or the bounded framing rules here.
//!
//! TLS uses a self-signed host certificate. The host identity is generated
//! once and retained in pins_dir, and a guest pins the certificate by the
//! target IP/port after its first successful connection (TOFU). The first
//! connection has no identity guarantee: an active attacker can replace the
//! certificate before the guest has a pin. There are deliberately no
//! fingerprint or invitation-token fields in the UI-facing API. A future
//! release should add an explicit authenticated invitation or an equivalent
//! out-of-band certificate check before this transport is used on an
//! untrusted network.

use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use quinn::{ClientConfig, Endpoint, RecvStream, SendStream, ServerConfig};
use rustls::client::danger::{self, HandshakeSignatureValid, ServerCertVerified};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use sha2::{Digest, Sha256};

const WIRE_VERSION: u16 = 1;
const SAVE_SIZE: usize = 65_536;
const MAX_FRAME: usize = 16 * 1024;
const SAVE_CHUNK: usize = 8 * 1024;
const CHANNEL_CAPACITY: usize = 256;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SERVER_NAME: &str = "ss2-local";

/// The fixed wall clock used by the SS2 compatibility profile.
pub const RTC_SECONDS: u64 = 1_752_000_000;

const FRAME_HELLO: u8 = 1;
const FRAME_SAVE_CHUNK: u8 = 2;
const FRAME_SAVE_DONE: u8 = 3;
const FRAME_ACCEPT: u8 = 4;
const FRAME_REJECT: u8 = 5;
const FRAME_MESSAGE: u8 = 6;

/// Compatibility identity and the local player's initial battery image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    pub rom_hash: [u8; 32],
    pub build_hash: [u8; 32],
    pub save: Option<Vec<u8>>,
}

impl Hello {
    fn validate(&self) -> Result<(), String> {
        if let Some(save) = &self.save {
            if save.len() != SAVE_SIZE {
                return Err(format!(
                    "initial save must be exactly {SAVE_SIZE} bytes, got {}",
                    save.len()
                ));
            }
        }
        Ok(())
    }
}

/// Application messages exchanged after the startup barrier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// Boundary-0/readiness digest. The application sends this before its
    /// first simulation advance.
    Ready([u8; 32]),
    /// One complete local input row. advantage is the getgud hint.
    Input {
        tick: u32,
        keys: u32,
        advantage: i16,
    },
    /// Settled state hash at a state boundary.
    Hash { tick: u32, hash: [u8; 32] },
    /// Request an orderly session leave.
    Leave,
}

/// Events delivered by the transport worker.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Status(String),
    Connected {
        local_player: usize,
        /// Ordered as host/seat 0 followed by guest/seat 1.
        saves: [Option<Vec<u8>>; 2],
    },
    Message(Message),
    Error(String),
}

/// Handles for the background transport worker.
///
/// Both channels are bounded. Dropping the tx sender causes the worker to
/// wind down once the current QUIC operation completes; dropping rx causes
/// the worker to stop when it next reports an event.
pub struct Wire {
    pub tx: SyncSender<Message>,
    pub rx: Receiver<Event>,
}

impl Wire {
    /// Queue one message for the network worker.
    pub fn send(&self, message: Message) -> Result<(), mpsc::SendError<Message>> {
        self.tx.send(message)
    }

    /// Poll one worker event without waiting.
    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        self.rx.try_recv()
    }
}

/// Start one direct-IP host or guest.
///
/// address is the host's listen address for a host and the destination
/// address for a guest. pins_dir is normally the executable's config
/// directory. The host certificate/key are retained there; guest certificate
/// pins are stored below pins/ and are keyed by IP and port.
/// Startup and transport failures are reported as Event::Error because this
/// function returns immediately while the worker binds/connects in the
/// background.
pub fn spawn(host: bool, address: SocketAddr, hello: Hello, pins_dir: PathBuf) -> Wire {
    let (tx, command_rx) = mpsc::sync_channel(CHANNEL_CAPACITY);
    let (event_tx, rx) = mpsc::sync_channel(CHANNEL_CAPACITY);
    let thread_event_tx = event_tx.clone();

    let worker_name = if host {
        "ss2-netplay-host"
    } else {
        "ss2-netplay-guest"
    };
    let thread_result = thread::Builder::new()
        .name(worker_name.to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    report_error(
                        &event_tx,
                        format!("failed to create network runtime: {error}"),
                    );
                    return;
                }
            };

            let result = runtime.block_on(run_worker(
                host,
                address,
                hello,
                pins_dir,
                command_rx,
                event_tx.clone(),
            ));
            if let Err(error) = result {
                report_error(&event_tx, error);
            }
        });

    if let Err(error) = thread_result {
        report_error(
            &thread_event_tx,
            format!("failed to start network worker: {error}"),
        );
    }

    Wire { tx, rx }
}

fn report_error(event_tx: &SyncSender<Event>, error: String) {
    let _ = event_tx.try_send(Event::Error(error));
}

async fn run_worker(
    host: bool,
    address: SocketAddr,
    hello: Hello,
    pins_dir: PathBuf,
    command_rx: Receiver<Message>,
    event_tx: SyncSender<Event>,
) -> Result<(), String> {
    hello.validate()?;

    if host {
        report_status(&event_tx, format!("starting host on {address}"));
        let (cert_der, key_der) = load_or_create_identity(&pins_dir)?;
        let server_config = make_server_config(cert_der, key_der)?;
        let endpoint = Endpoint::server(server_config, address)
            .map_err(|error| format!("failed to bind QUIC host on {address}: {error}"))?;
        let actual = endpoint
            .local_addr()
            .map_err(|error| format!("failed to query host address: {error}"))?;
        report_status(&event_tx, format!("Listening on UDP port {}. Forward this UDP port to this computer. Waiting for your friend to connect.", actual.port()));

        let incoming = loop {
            tokio::select! {
                incoming = endpoint.accept() => break incoming.ok_or("Host endpoint stopped")?,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    match command_rx.try_recv() {
                        Err(TryRecvError::Empty) => {},
                        _ => return Ok(()),
                    }
                }
            }
        };
        report_status(
            &event_tx,
            format!(
                "Received connection attempt from {}. Checking QUIC handshake...",
                incoming.remote_address()
            ),
        );
        let connection = tokio::time::timeout(STARTUP_TIMEOUT, incoming)
            .await
            .map_err(|_| "timed out during QUIC host handshake".to_owned())?
            .map_err(|error| format!("QUIC host handshake failed: {error} [{error:?}]"))?;
        report_status(
            &event_tx,
            format!("QUIC connected to {}", connection.remote_address()),
        );
        let (mut send, mut recv) = tokio::time::timeout(STARTUP_TIMEOUT, connection.accept_bi())
            .await
            .map_err(|_| "timed out waiting for guest control stream".to_owned())?
            .map_err(|error| format!("failed to accept guest control stream: {error}"))?;

        let remote_hello = tokio::time::timeout(
            STARTUP_TIMEOUT,
            handshake_host(&mut send, &mut recv, &hello),
        )
        .await
        .map_err(|_| "Startup data exchange timed out")??;
        let saves = [hello.save.clone(), remote_hello.save.clone()];
        report_connected(&event_tx, 0, saves)?;
        run_stream(connection, send, recv, command_rx, event_tx).await
    } else {
        report_status(&event_tx, format!("connecting to {address}"));
        let pin_path = certificate_pin_path(&pins_dir, address);
        let expected_pin = load_pin(&pin_path)?;
        let observed_pin = Arc::new(Mutex::new(None));
        let client_config = make_client_config(expected_pin, Arc::clone(&observed_pin))?;
        let bind_addr = match address.ip() {
            IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        };
        let mut endpoint = Endpoint::client(bind_addr)
            .map_err(|error| format!("failed to bind guest QUIC endpoint: {error}"))?;
        endpoint.set_default_client_config(client_config);
        let connecting = endpoint
            .connect(address, SERVER_NAME)
            .map_err(|error| format!("failed to start QUIC connection to {address}: {error}"))?;
        let connection = tokio::time::timeout(STARTUP_TIMEOUT, connecting)
            .await
            .map_err(|_| format!("timed out connecting to {address}"))?
            .map_err(|error| {
                let hint = if matches!(error, quinn::ConnectionError::TimedOut) {
                    " Check that the host is listening and that forwarding/firewall rules allow UDP, not only TCP."
                } else { "" };
                format!("QUIC guest handshake failed: {error}.{hint} Detail: {error:?}")
            })?;
        save_observed_pin(&pin_path, &observed_pin)?;
        report_status(
            &event_tx,
            format!("QUIC connected to {}", connection.remote_address()),
        );
        let (mut send, mut recv) = tokio::time::timeout(STARTUP_TIMEOUT, connection.open_bi())
            .await
            .map_err(|_| "timed out opening guest control stream".to_owned())?
            .map_err(|error| format!("failed to open guest control stream: {error}"))?;

        let remote_hello = tokio::time::timeout(
            STARTUP_TIMEOUT,
            handshake_guest(&mut send, &mut recv, &hello),
        )
        .await
        .map_err(|_| "Startup data exchange timed out")??;
        let saves = [remote_hello.save.clone(), hello.save.clone()];
        report_connected(&event_tx, 1, saves)?;
        run_stream(connection, send, recv, command_rx, event_tx).await
    }
}

fn report_status(event_tx: &SyncSender<Event>, status: String) {
    // Status is advisory. A full event queue must never make a bind or
    // connect operation deadlock, so it is safe to discard an old status.
    let _ = event_tx.try_send(Event::Status(status));
}

fn report_connected(
    event_tx: &SyncSender<Event>,
    local_player: usize,
    saves: [Option<Vec<u8>>; 2],
) -> Result<(), String> {
    event_tx
        .send(Event::Connected {
            local_player,
            saves,
        })
        .map_err(|_| "event receiver closed during connection setup".to_owned())
}

async fn handshake_host(
    send: &mut SendStream,
    recv: &mut RecvStream,
    local: &Hello,
) -> Result<Hello, String> {
    let remote = recv_hello(recv).await?;
    if let Err(reason) = validate_peer(local, &remote) {
        let _ = send_reject(send, &reason).await;
        return Err(reason);
    }

    send_hello(send, local).await?;
    match read_frame(recv).await? {
        frame if frame.first() == Some(&FRAME_ACCEPT) && frame.len() == 1 => Ok(remote),
        frame if frame.first() == Some(&FRAME_REJECT) => Err(parse_reject(&frame)
            .unwrap_or_else(|_| "guest rejected compatibility manifest".to_owned())),
        _ => {
            let reason = "expected guest handshake acceptance".to_owned();
            let _ = send_reject(send, &reason).await;
            Err(reason)
        }
    }
}

async fn handshake_guest(
    send: &mut SendStream,
    recv: &mut RecvStream,
    local: &Hello,
) -> Result<Hello, String> {
    send_hello(send, local).await?;
    let frame = read_frame(recv).await?;
    if frame.first() == Some(&FRAME_REJECT) {
        return Err(parse_reject(&frame)
            .unwrap_or_else(|_| "host rejected compatibility manifest".to_owned()));
    }
    if frame.first() != Some(&FRAME_HELLO) {
        return Err("expected host compatibility manifest".to_owned());
    }
    let remote = recv_hello_after_first(recv, frame).await?;
    if let Err(reason) = validate_peer(local, &remote) {
        let _ = send_reject(send, &reason).await;
        return Err(reason);
    }
    write_frame(send, &[FRAME_ACCEPT]).await?;
    Ok(remote)
}

fn validate_peer(local: &Hello, remote: &Hello) -> Result<(), String> {
    local.validate()?;
    remote.validate()?;
    // The wire's protocol/profile and RTC are encoded in every Hello. They
    // are constants for this MVP, but retaining explicit checks makes a
    // future incompatible change fail before any emulation starts.
    if WIRE_VERSION != 1 {
        return Err(format!("unsupported local wire version {WIRE_VERSION}"));
    }
    if RTC_SECONDS != 1_752_000_000 {
        return Err(format!("unsupported local RTC profile {RTC_SECONDS}"));
    }
    if local.rom_hash != remote.rom_hash {
        return Err(format!(
            "ROM hash mismatch (local {}, remote {})",
            short_hash(&local.rom_hash),
            short_hash(&remote.rom_hash)
        ));
    }
    if local.build_hash != remote.build_hash {
        return Err(format!(
            "executable build hash mismatch (local {}, remote {})",
            short_hash(&local.build_hash),
            short_hash(&remote.build_hash)
        ));
    }
    Ok(())
}

async fn send_hello(send: &mut SendStream, hello: &Hello) -> Result<(), String> {
    hello.validate()?;
    let (save_len, save_hash) = match &hello.save {
        Some(save) => (SAVE_SIZE as u32, sha256(save)),
        None => (0, [0; 32]),
    };
    let mut frame = Vec::with_capacity(1 + 2 + 8 + 32 + 32 + 4 + 32);
    frame.push(FRAME_HELLO);
    frame.extend_from_slice(&WIRE_VERSION.to_be_bytes());
    frame.extend_from_slice(&RTC_SECONDS.to_be_bytes());
    frame.extend_from_slice(&hello.rom_hash);
    frame.extend_from_slice(&hello.build_hash);
    frame.extend_from_slice(&save_len.to_be_bytes());
    frame.extend_from_slice(&save_hash);
    write_frame(send, &frame).await?;

    if let Some(save) = &hello.save {
        for (chunk_index, chunk) in save.chunks(SAVE_CHUNK).enumerate() {
            let offset = chunk_index
                .checked_mul(SAVE_CHUNK)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| "save chunk offset overflow".to_owned())?;
            let chunk_len =
                u16::try_from(chunk.len()).map_err(|_| "save chunk is too large".to_owned())?;
            let mut chunk_frame = Vec::with_capacity(1 + 4 + 4 + 2 + chunk.len());
            chunk_frame.push(FRAME_SAVE_CHUNK);
            chunk_frame.extend_from_slice(&offset.to_be_bytes());
            chunk_frame.extend_from_slice(&(SAVE_SIZE as u32).to_be_bytes());
            chunk_frame.extend_from_slice(&chunk_len.to_be_bytes());
            chunk_frame.extend_from_slice(chunk);
            write_frame(send, &chunk_frame).await?;
        }
    }
    write_frame(send, &[FRAME_SAVE_DONE]).await
}

async fn recv_hello(recv: &mut RecvStream) -> Result<Hello, String> {
    let frame = read_frame(recv).await?;
    if frame.first() == Some(&FRAME_REJECT) {
        return Err(parse_reject(&frame).unwrap_or_else(|_| "peer rejected handshake".to_owned()));
    }
    if frame.first() != Some(&FRAME_HELLO) {
        return Err("expected compatibility manifest".to_owned());
    }
    recv_hello_after_first(recv, frame).await
}

async fn recv_hello_after_first(recv: &mut RecvStream, frame: Vec<u8>) -> Result<Hello, String> {
    let meta = parse_hello(&frame)?;
    let mut save = if meta.save_len == 0 {
        None
    } else {
        Some(Vec::with_capacity(meta.save_len as usize))
    };
    let mut next_offset = 0usize;

    loop {
        let frame = read_frame(recv).await?;
        match frame.first() {
            Some(&FRAME_SAVE_CHUNK) => {
                let (offset, total, chunk) = parse_save_chunk(&frame)?;
                if meta.save_len == 0 {
                    return Err("peer sent save data for a fresh cartridge".to_owned());
                }
                if total != meta.save_len {
                    return Err(format!(
                        "save chunk total {total} disagrees with manifest {}",
                        meta.save_len
                    ));
                }
                if offset as usize != next_offset {
                    return Err(format!(
                        "save chunks are not contiguous (expected offset {next_offset}, got {offset})"
                    ));
                }
                let end = next_offset
                    .checked_add(chunk.len())
                    .ok_or_else(|| "save chunk offset overflow".to_owned())?;
                if end > meta.save_len as usize {
                    return Err("save chunks exceed the declared save length".to_owned());
                }
                save.as_mut()
                    .expect("nonzero manifest creates save buffer")
                    .extend_from_slice(&chunk);
                next_offset = end;
            }
            Some(&FRAME_SAVE_DONE) if frame.len() == 1 => {
                if next_offset != meta.save_len as usize {
                    return Err(format!(
                        "save transfer ended at {next_offset} bytes, expected {}",
                        meta.save_len
                    ));
                }
                if let Some(bytes) = &save {
                    if sha256(bytes) != meta.save_hash {
                        return Err("initial save SHA-256 mismatch".to_owned());
                    }
                }
                return Ok(Hello {
                    rom_hash: meta.rom_hash,
                    build_hash: meta.build_hash,
                    save,
                });
            }
            Some(&FRAME_REJECT) => {
                return Err(
                    parse_reject(&frame).unwrap_or_else(|_| "peer rejected handshake".to_owned())
                );
            }
            _ => return Err("unexpected frame while receiving initial save".to_owned()),
        }
    }
}

struct HelloMeta {
    rom_hash: [u8; 32],
    build_hash: [u8; 32],
    save_len: u32,
    save_hash: [u8; 32],
}

fn parse_hello(frame: &[u8]) -> Result<HelloMeta, String> {
    const SIZE: usize = 1 + 2 + 8 + 32 + 32 + 4 + 32;
    if frame.len() != SIZE || frame[0] != FRAME_HELLO {
        return Err("malformed compatibility manifest".to_owned());
    }
    let version = u16::from_be_bytes([frame[1], frame[2]]);
    if version != WIRE_VERSION {
        return Err(format!(
            "wire protocol version mismatch (local {WIRE_VERSION}, remote {version})"
        ));
    }
    let mut rtc_bytes = [0; 8];
    rtc_bytes.copy_from_slice(&frame[3..11]);
    let rtc = u64::from_be_bytes(rtc_bytes);
    if rtc != RTC_SECONDS {
        return Err(format!(
            "RTC profile mismatch (local {RTC_SECONDS}, remote {rtc})"
        ));
    }
    let mut rom_hash = [0; 32];
    rom_hash.copy_from_slice(&frame[11..43]);
    let mut build_hash = [0; 32];
    build_hash.copy_from_slice(&frame[43..75]);
    let mut save_len_bytes = [0; 4];
    save_len_bytes.copy_from_slice(&frame[75..79]);
    let save_len = u32::from_be_bytes(save_len_bytes);
    if save_len != 0 && save_len as usize != SAVE_SIZE {
        return Err(format!(
            "initial save must be exactly {SAVE_SIZE} bytes, got {save_len}"
        ));
    }
    let mut save_hash = [0; 32];
    save_hash.copy_from_slice(&frame[79..111]);
    if save_len == 0 && save_hash != [0; 32] {
        return Err("fresh cartridge manifest has a nonzero save hash".to_owned());
    }
    Ok(HelloMeta {
        rom_hash,
        build_hash,
        save_len,
        save_hash,
    })
}

fn parse_save_chunk(frame: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if frame.len() < 1 + 4 + 4 + 2 || frame[0] != FRAME_SAVE_CHUNK {
        return Err("malformed save chunk".to_owned());
    }
    let mut offset_bytes = [0; 4];
    offset_bytes.copy_from_slice(&frame[1..5]);
    let offset = u32::from_be_bytes(offset_bytes);
    let mut total_bytes = [0; 4];
    total_bytes.copy_from_slice(&frame[5..9]);
    let total = u32::from_be_bytes(total_bytes);
    let mut length_bytes = [0; 2];
    length_bytes.copy_from_slice(&frame[9..11]);
    let length = u16::from_be_bytes(length_bytes) as usize;
    if frame.len() != 11 + length || length > SAVE_CHUNK {
        return Err("malformed or oversized save chunk".to_owned());
    }
    Ok((offset, total, frame[11..].to_vec()))
}

async fn send_reject(send: &mut SendStream, reason: &str) -> Result<(), String> {
    let reason = reason.as_bytes();
    if reason.len() > 512 {
        return Err("handshake rejection reason is too long".to_owned());
    }
    let mut frame = Vec::with_capacity(3 + reason.len());
    frame.push(FRAME_REJECT);
    frame.extend_from_slice(&(reason.len() as u16).to_be_bytes());
    frame.extend_from_slice(reason);
    write_frame(send, &frame).await?;
    let _ = send.finish();
    let _ = tokio::time::timeout(Duration::from_secs(1), send.stopped()).await;
    Ok(())
}

fn parse_reject(frame: &[u8]) -> Result<String, String> {
    if frame.len() < 3 || frame[0] != FRAME_REJECT {
        return Err("malformed handshake rejection".to_owned());
    }
    let length = u16::from_be_bytes([frame[1], frame[2]]) as usize;
    if length > 512 || frame.len() != 3 + length {
        return Err("malformed handshake rejection length".to_owned());
    }
    String::from_utf8(frame[3..].to_vec())
        .map_err(|_| "handshake rejection was not UTF-8".to_owned())
}

async fn run_stream(
    connection: quinn::Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    command_rx: Receiver<Message>,
    event_tx: SyncSender<Event>,
) -> Result<(), String> {
    let reader_events = event_tx.clone();
    let mut reader = tokio::spawn(async move {
        loop {
            let frame = read_frame(&mut recv).await?;
            if frame.first() != Some(&FRAME_MESSAGE) {
                return Err("unexpected non-message frame after handshake".to_owned());
            }
            let message = decode_message(&frame[1..])?;
            reader_events
                .try_send(Event::Message(message))
                .map_err(|_| "Application receive queue full or closed".to_owned())?;
        }
        #[allow(unreachable_code)]
        Ok::<(), String>(())
    });

    let result = async {
        loop {
            tokio::select! {
                result = &mut reader => return result.map_err(|e| e.to_string())?,
                _ = tokio::time::sleep(Duration::from_millis(1)) => {
                    for _ in 0..64 {
                        let message = match command_rx.try_recv() {
                            Ok(m) => m,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return Ok(()),
                        };
                        let leave = matches!(message, Message::Leave);
                        let mut frame = vec![FRAME_MESSAGE];
                        frame.extend(encode_message(&message)?);
                        tokio::time::timeout(Duration::from_secs(5), write_frame(&mut send, &frame)).await.map_err(|_| "Network send stalled")??;
                        if leave {
                            let _ = send.finish();
                            let _ = tokio::time::timeout(Duration::from_millis(500), send.stopped()).await;
                            return Ok(());
                        }
                    }
                }
            }
        }
    }.await;
    reader.abort();
    connection.close(0u32.into(), b"netplay stream closed");
    result.map_err(|error| format!("network connection closed: {error}"))
}

fn encode_message(message: &Message) -> Result<Vec<u8>, String> {
    let mut payload = Vec::with_capacity(1 + 4 + 4 + 2 + 32);
    match message {
        Message::Ready(hash) => {
            payload.push(1);
            payload.extend_from_slice(hash);
        }
        Message::Input {
            tick,
            keys,
            advantage,
        } => {
            if keys & !0x03ff != 0 {
                return Err(format!(
                    "input key mask {keys:#x} sets unsupported button bits"
                ));
            }
            payload.push(2);
            payload.extend_from_slice(&tick.to_be_bytes());
            payload.extend_from_slice(&keys.to_be_bytes());
            payload.extend_from_slice(&advantage.to_be_bytes());
        }
        Message::Hash { tick, hash } => {
            payload.push(3);
            payload.extend_from_slice(&tick.to_be_bytes());
            payload.extend_from_slice(hash);
        }
        Message::Leave => payload.push(4),
    }
    Ok(payload)
}

fn decode_message(payload: &[u8]) -> Result<Message, String> {
    let kind = *payload
        .first()
        .ok_or_else(|| "empty application message".to_owned())?;
    match kind {
        1 if payload.len() == 33 => {
            let mut hash = [0; 32];
            hash.copy_from_slice(&payload[1..]);
            Ok(Message::Ready(hash))
        }
        2 if payload.len() == 11 => {
            let tick = u32::from_be_bytes(payload[1..5].try_into().expect("fixed input tick"));
            let keys = u32::from_be_bytes(payload[5..9].try_into().expect("fixed input mask"));
            if keys & !0x03ff != 0 {
                return Err(format!(
                    "input key mask {keys:#x} sets unsupported button bits"
                ));
            }
            let advantage = i16::from_be_bytes(payload[9..11].try_into().expect("fixed advantage"));
            Ok(Message::Input {
                tick,
                keys,
                advantage,
            })
        }
        3 if payload.len() == 37 => {
            let tick = u32::from_be_bytes(payload[1..5].try_into().expect("fixed hash tick"));
            let mut hash = [0; 32];
            hash.copy_from_slice(&payload[5..]);
            Ok(Message::Hash { tick, hash })
        }
        4 if payload.len() == 1 => Ok(Message::Leave),
        _ => Err("unknown or malformed application message".to_owned()),
    }
}

async fn read_frame(recv: &mut RecvStream) -> Result<Vec<u8>, String> {
    let mut length_bytes = [0; 4];
    recv.read_exact(&mut length_bytes)
        .await
        .map_err(|error| format!("failed reading frame length: {error}"))?;
    let length = u32::from_be_bytes(length_bytes) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(format!(
            "frame length {length} exceeds the {MAX_FRAME}-byte limit"
        ));
    }
    let mut frame = vec![0; length];
    recv.read_exact(&mut frame)
        .await
        .map_err(|error| format!("failed reading {length}-byte frame: {error}"))?;
    Ok(frame)
}

async fn write_frame(send: &mut SendStream, frame: &[u8]) -> Result<(), String> {
    if frame.is_empty() || frame.len() > MAX_FRAME {
        return Err(format!(
            "frame length {} exceeds the {MAX_FRAME}-byte limit",
            frame.len()
        ));
    }
    let length = u32::try_from(frame.len()).map_err(|_| "frame length overflow".to_owned())?;
    send.write_all(&length.to_be_bytes())
        .await
        .map_err(|error| format!("failed writing frame length: {error}"))?;
    send.write_all(frame)
        .await
        .map_err(|error| format!("failed writing frame: {error}"))?;
    Ok(())
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn short_hash(hash: &[u8; 32]) -> String {
    hash[..8].iter().map(|b| format!("{b:02x}")).collect()
}

fn load_or_create_identity(
    dir: &Path,
) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create config folder: {e}"))?;
    let path = dir.join("host-identity.bin");
    match fs::read(&path) {
        Ok(data) => {
            if data.len() < 8 || data.len() > 16384 {
                return Err("Invalid host identity file".into());
            }
            let len = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
            if len == 0 || len + 4 >= data.len() {
                return Err("Invalid host certificate length".into());
            }
            Ok((
                CertificateDer::from(data[4..4 + len].to_vec()),
                PrivatePkcs8KeyDer::from(data[4 + len..].to_vec()).into(),
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let identity = rcgen::generate_simple_self_signed(vec![SERVER_NAME.into()])
                .map_err(|e| e.to_string())?;
            let cert = identity.cert.der().clone();
            let key = identity.key_pair.serialize_der();
            let mut data = (cert.len() as u32).to_be_bytes().to_vec();
            data.extend_from_slice(&cert);
            data.extend_from_slice(&key);
            fs::write(&path, data).map_err(|e| format!("Cannot save host identity: {e}"))?;
            Ok((cert, PrivatePkcs8KeyDer::from(key).into()))
        }
        Err(e) => Err(format!("Cannot load host identity: {e}")),
    }
}

fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut config = quinn::TransportConfig::default();
    config.max_concurrent_uni_streams(0u32.into());
    config.max_concurrent_bidi_streams(1u32.into());
    config.stream_receive_window((256u32 * 1024).into());
    config.receive_window((512u32 * 1024).into());
    config.send_window(512 * 1024);
    config.keep_alive_interval(Some(Duration::from_secs(1)));
    config.max_idle_timeout(Some(Duration::from_secs(10).try_into().unwrap()));
    Arc::new(config)
}

fn make_server_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<ServerConfig, String> {
    let mut config = ServerConfig::with_single_cert(vec![cert], key).map_err(|e| e.to_string())?;
    config.transport_config(transport_config());
    Ok(config)
}

#[derive(Debug)]
struct PinVerifier {
    expected: Option<[u8; 32]>,
    observed: Arc<Mutex<Option<[u8; 32]>>>,
    provider: Arc<CryptoProvider>,
}
impl danger::ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let actual = sha256(cert.as_ref());
        if self.expected.is_some_and(|expected| expected != actual) {
            return Err(RustlsError::General("Host certificate changed. Verify the host, then remove this address's pin in config/pins to trust it again.".into()));
        }
        *self.observed.lock().unwrap() = Some(actual);
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn make_client_config(
    expected: Option<[u8; 32]>,
    observed: Arc<Mutex<Option<[u8; 32]>>>,
) -> Result<ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier {
            expected,
            observed,
            provider,
        }))
        .with_no_client_auth();
    let crypto =
        quinn::crypto::rustls::QuicClientConfig::try_from(tls).map_err(|e| e.to_string())?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport_config());
    Ok(config)
}

fn certificate_pin_path(dir: &Path, address: SocketAddr) -> PathBuf {
    dir.join("pins")
        .join(format!("{}.sha256", address.to_string().replace(':', "_")))
}
fn load_pin(path: &Path) -> Result<Option<[u8; 32]>, String> {
    match fs::read(path) {
        Ok(data) => Ok(Some(
            data.try_into()
                .map_err(|_| "Invalid certificate pin file")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot read certificate pin: {e}")),
    }
}
fn save_observed_pin(path: &Path, observed: &Mutex<Option<[u8; 32]>>) -> Result<(), String> {
    let hash = observed
        .lock()
        .unwrap()
        .ok_or("No host certificate observed")?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(path, hash).map_err(|e| format!("Cannot save certificate pin: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_and_invalid_button_messages() {
        for input in [vec![], vec![255], vec![1; 32], vec![4, 0], vec![3; 38]] {
            assert!(decode_message(&input).is_err());
        }
        let mut bad_mask = vec![2];
        bad_mask.extend_from_slice(&0u32.to_be_bytes());
        bad_mask.extend_from_slice(&0x400u32.to_be_bytes());
        bad_mask.extend_from_slice(&0i16.to_be_bytes());
        assert!(decode_message(&bad_mask)
            .unwrap_err()
            .contains("unsupported button"));
        assert!(parse_save_chunk(&[FRAME_SAVE_CHUNK; 10]).is_err());
        assert!(parse_hello(&[FRAME_HELLO; 111]).is_err());
    }

    #[test]
    fn real_connection_rejects_rom_mismatch_before_session() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        drop(socket);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ss2-wire-reject-{stamp}"));
        let host = spawn(
            true,
            address,
            Hello {
                rom_hash: [1; 32],
                build_hash: [2; 32],
                save: None,
            },
            dir.join("host"),
        );
        loop {
            match host.rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Status(s) if s.starts_with("Listening") => break,
                Event::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        let guest = spawn(
            false,
            address,
            Hello {
                rom_hash: [3; 32],
                build_hash: [2; 32],
                save: None,
            },
            dir.join("guest"),
        );
        for wire in [&host, &guest] {
            loop {
                match wire.rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                    Event::Error(e) => {
                        assert!(e.contains("ROM hash mismatch"), "{e}");
                        break;
                    }
                    Event::Connected { .. } => panic!("Mismatched ROM was accepted"),
                    _ => {}
                }
            }
        }
    }
}
