#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use sha2::{Digest, Sha256};
use ss2_rollback_harness::{
    netplay_audio::Audio,
    netplay_game::{Game, FRAME_RATE},
    netplay_stats::Stats,
    netplay_ui::{Action, Ui},
    netplay_wire::{self, Event, Hello, Message, Wire},
};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct LocalFiles {
    rom: Vec<u8>,
    hello: Hello,
    save_path: PathBuf,
    saved: Option<Vec<u8>>,
    backup_done: bool,
}
impl LocalFiles {
    fn load(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root.join("ROM")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join("save")).map_err(|e| e.to_string())?;
        let mut roms = Vec::new();
        for entry in fs::read_dir(root.join("ROM")).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("gba"))
            {
                roms.push(path);
            }
        }
        if roms.len() != 1 {
            return Err("Put exactly one Shining Soul II .gba file in the ROM folder beside this EXE, then click Host or Connect again.".into());
        }
        let rom_path = &roms[0];
        let size = fs::metadata(rom_path).map_err(|e| e.to_string())?.len();
        if !(192..=32 * 1024 * 1024).contains(&size) {
            return Err("Invalid GBA ROM size".into());
        }
        let rom = fs::read(rom_path).map_err(|e| e.to_string())?;
        let save_path = root
            .join("save")
            .join(rom_path.file_stem().ok_or("Invalid ROM filename")?)
            .with_extension("sav");
        let saved = match fs::read(&save_path) {
            Ok(bytes) if bytes.len() == 65536 => Some(bytes),
            Ok(_) => {
                return Err(format!(
                    "{} must be a raw 64 KiB battery save",
                    save_path.display()
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Cannot load save: {e}")),
        };
        let exe = fs::read(std::env::current_exe().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let hello = Hello {
            rom_hash: Sha256::digest(&rom).into(),
            build_hash: Sha256::digest(exe).into(),
            save: saved.clone(),
        };
        Ok(Self {
            rom,
            hello,
            save_path,
            saved,
            backup_done: false,
        })
    }

    fn persist(&mut self, bytes: Vec<u8>) -> Result<bool, String> {
        if bytes.len() != 65536 {
            return Err("Unexpected battery-save size; existing file was left intact".into());
        }
        if self.saved.as_ref() == Some(&bytes)
            || (self.saved.is_none() && bytes.iter().all(|b| *b == 0xff))
        {
            return Ok(false);
        }
        if !self.backup_done && self.save_path.exists() {
            fs::copy(&self.save_path, self.save_path.with_extension("sav.bak"))
                .map_err(|e| format!("Cannot back up save: {e}"))?;
            self.backup_done = true;
        }
        let temp = self.save_path.with_extension("sav.tmp");
        let mut file = File::create(&temp).map_err(|e| format!("Cannot write save: {e}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("Cannot flush save: {e}"))?;
        drop(file);
        fs::rename(&temp, &self.save_path).map_err(|e| format!("Cannot replace save: {e}"))?;
        self.saved = Some(bytes);
        Ok(true)
    }
}

fn send(wire: &Wire, message: Message) -> Result<(), String> {
    wire.tx
        .try_send(message)
        .map_err(|e| format!("Network send queue stopped or full: {e}"))
}

fn main() {
    let root = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let iroh_test = std::env::args().any(|arg| arg == "--self-test-iroh");
    if iroh_test || std::env::args().any(|arg| arg == "--self-test") {
        let result = if iroh_test {
            ss2_rollback_harness::netplay_selftest::run_iroh(&root)
        } else {
            ss2_rollback_harness::netplay_selftest::run(&root)
        };
        let text = match &result {
            Ok(s) => s.clone(),
            Err(e) => format!("FAIL: {e}\n"),
        };
        let filename = if iroh_test {
            "self-test-iroh.txt"
        } else {
            "self-test.txt"
        };
        let _ = fs::write(root.join(filename), text);
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if let Err(e) = run(&root) {
        let _ = fs::write(root.join("error.txt"), e);
    }
}

fn run(root: &Path) -> Result<(), String> {
    let mut ui = Ui::new(root)?;
    if std::env::args().any(|arg| arg == "--ui-preview") {
        ui.show_setup(
            "Choose Host to share a connect code, or paste your friend's code and Join.",
            false,
        )?;
        ui.write_preview(&root.join("setup-preview.bmp"))?;
        ui.set_connect_code(format!("ss2-1:{}", "ABCdef0123456789_-".repeat(32)));
        ui.show_setup(
            "Listening. Copy your connect code and send it to your friend.",
            true,
        )?;
        ui.write_preview(&root.join("host-code-preview.bmp"))?;
        ui.show_setup("Ready.", false)?;
        ui.set_direct_mode(true);
        ui.show_setup(
            "Host a UDP port or connect to your friend's numeric IP address.",
            false,
        )?;
        ui.write_preview(&root.join("direct-preview.bmp"))?;
        ui.write_exit_preview(&root.join("exit-preview.bmp"))?;
        ui.write_debug_preview(&root.join("debug-preview.bmp"))?;
        return Ok(());
    }
    fs::create_dir_all(root.join("ROM")).map_err(|e| e.to_string())?;
    fs::create_dir_all(root.join("save")).map_err(|e| e.to_string())?;
    fs::create_dir_all(root.join("logs")).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut log = BufWriter::new(
        File::create(root.join("logs").join(format!("session-{stamp}.txt")))
            .map_err(|e| e.to_string())?,
    );
    let mut status = "Put your .gba in ROM. Choose Host to share a connect code, or paste your friend's code and Join.".to_string();
    let mut wire: Option<Wire> = None;
    let mut files: Option<LocalFiles> = None;
    let mut game: Option<Game> = None;
    let mut audio: Option<Audio> = None;
    let mut audio_note = String::new();
    let mut throttle = mgba_rollback::throttler::Throttler::new();
    let mut deadline = Instant::now();
    let mut last_title = Instant::now();
    let mut title = String::new();
    let mut save_candidate: Option<(Vec<u8>, u32)> = None;
    let mut last_saved = false;
    let mut stats = Stats::new(Instant::now());
    loop {
        if let Some(action) = ui.poll() {
            match action {
                Action::Quit => break,
                Action::Cancel if game.is_none() => {
                    wire = None;
                    files = None;
                    ui.set_connect_code(String::new());
                    status = "Connection cancelled. Choose Host or Join.".into();
                }
                action @ (Action::HostCode | Action::JoinCode(_)) if wire.is_none() => {
                    let code = match action {
                        Action::JoinCode(code) => Some(code),
                        _ => None,
                    };
                    let validated = code
                        .as_deref()
                        .map(ss2_rollback_harness::netplay_iroh::validate_code)
                        .transpose();
                    match validated.and_then(|_| LocalFiles::load(root)) {
                        Ok(local) => {
                            ui.set_connect_code(String::new());
                            wire = Some(netplay_wire::spawn_iroh(
                                code,
                                local.hello.clone(),
                                root.join("config"),
                            ));
                            files = Some(local);
                            status = "Starting connection...".into();
                        }
                        Err(e) => status = e,
                    }
                }
                Action::Host(port) if wire.is_none() => match LocalFiles::load(root) {
                    Ok(local) => {
                        wire = Some(netplay_wire::spawn(
                            true,
                            ([0, 0, 0, 0], port).into(),
                            local.hello.clone(),
                            root.join("config"),
                        ));
                        files = Some(local);
                        status = format!("Listening on UDP port {port}. Forward this UDP port to this computer, then have your friend connect.");
                    }
                    Err(e) => status = e,
                },
                Action::Connect(address) if wire.is_none() => match LocalFiles::load(root) {
                    Ok(local) => {
                        wire = Some(netplay_wire::spawn(
                            false,
                            address,
                            local.hello.clone(),
                            root.join("config"),
                        ));
                        files = Some(local);
                        status = format!("Connecting to {address}...");
                    }
                    Err(e) => status = e,
                },
                _ => {}
            }
        }
        if !ui.is_open() {
            break;
        }
        let step = (|| -> Result<(), String> {
            if let Some(network) = &wire {
                for _ in 0..256 {
                    match network.rx.try_recv() {
                        Ok(Event::ConnectCode(code)) => ui.set_connect_code(code),
                        Ok(Event::Status(s)) => {
                            writeln!(log, "STATUS {s}").map_err(|e| e.to_string())?;
                            log.flush().map_err(|e| e.to_string())?;
                            status = s;
                        }
                        Ok(Event::Connected {
                            local_player,
                            saves,
                        }) => {
                            if game.is_some() {
                                return Err("Duplicate connection".into());
                            }
                            let mut active = Game::new(
                                &files.as_ref().ok_or("No local files")?.rom,
                                &saves,
                                local_player,
                            )?;
                            active.session.set_present_delay(ui.selected_delay());
                            stats = Stats::new(Instant::now());
                            writeln!(log, "LOCAL present_delay={}", ui.selected_delay())
                                .map_err(|e| e.to_string())?;
                            active.enable_diagnostics(
                                root,
                                &files.as_ref().ok_or("No local files")?.hello,
                                &saves,
                            )?;
                            send(network, Message::Ready(active.initial_hash))?;
                            writeln!(
                                log,
                                "CONNECTED seat={} initial={:x?}",
                                local_player, active.initial_hash
                            )
                            .map_err(|e| e.to_string())?;
                            match Audio::new() {
                                Ok(a) => {
                                    audio = Some(a);
                                    audio_note.clear();
                                }
                                Err(e) => {
                                    audio_note = " | sound unavailable".into();
                                    writeln!(log, "AUDIO {e}").map_err(|e| e.to_string())?;
                                }
                            }
                            game = Some(active);
                            status = "Checking initial state with other player...".into();
                            throttle = mgba_rollback::throttler::Throttler::new();
                            deadline = Instant::now();
                            last_title = Instant::now() - Duration::from_secs(1);
                        }
                        Ok(Event::Message(message)) => {
                            game.as_mut()
                                .ok_or("Message before connection")?
                                .receive(message)?;
                        }
                        Ok(Event::Error(e)) => return Err(e),
                        Ok(Event::Rtt(rtt)) => stats.rtt = Some(rtt),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return Err("Connection closed".into()),
                    }
                }
                if let Some(active) = &mut game {
                    if active.can_advance() && Instant::now() >= deadline {
                        let slowdown = throttle
                            .step(active.session.skew(), active.session.speculation_balance());
                        let lead = active
                            .session
                            .local_queue_length()
                            .saturating_sub(active.session.matchable())
                            + 1;
                        let message = active.advance(ui.keys())?;
                        stats.advance(Instant::now(), active.last_depth, lead);
                        send(network, message)?;
                        for (tick, row) in active.session.drain_confirmed() {
                            writeln!(log, "INPUT {tick} {} {}", row[0], row[1])
                                .map_err(|e| e.to_string())?;
                        }
                        let frame = Duration::from_secs_f64(1.0 / (FRAME_RATE - slowdown as f64));
                        deadline += frame;
                        if Instant::now().saturating_duration_since(deadline) > frame * 4 {
                            deadline = Instant::now();
                        }
                    }
                    let (messages, save) = active.synchronization()?;
                    for msg in messages {
                        send(network, msg)?;
                    }
                    if let Some(bytes) = save {
                        match &mut save_candidate {
                            Some((old, count)) if old == &bytes => *count = count.saturating_add(1),
                            _ => save_candidate = Some((bytes, 1)),
                        }
                    }
                    // Require equal save bytes at two peer-matched checkpoints
                    // a simulated second apart, rather than treating delayed
                    // network confirmation as evidence of stable flash bytes.
                    if let Some((bytes, matching_checkpoints)) = &save_candidate {
                        if *matching_checkpoints >= 2 {
                            if files
                                .as_mut()
                                .ok_or("No local save path")?
                                .persist(bytes.clone())?
                            {
                                last_saved = true;
                                writeln!(log, "SAVE matched_boundary={}", active.matched_hash_tick)
                                    .map_err(|e| e.to_string())?;
                                log.flush().map_err(|e| e.to_string())?;
                            }
                        }
                    }
                    if let Some(output) = &mut audio {
                        output.set_volume(ui.volume());
                        output.pump(&active.session, active.player);
                    } else {
                        active.session.with_link(|link| {
                            for p in 0..2 {
                                link.core_mut(p).audio_buffer().clear();
                            }
                        });
                    }
                    if active.ready {
                        if last_title.elapsed() >= Duration::from_millis(250) {
                            ui.set_debug_lines(stats.lines(Instant::now(), active));
                            title = format!(
                                "SS2 | Player {} | {} | rollback {} | synced {}{}{}",
                                active.player + 1,
                                if active.can_advance() {
                                    "Connected"
                                } else {
                                    "Waiting for player"
                                },
                                active.last_depth,
                                active.matched_hash_tick,
                                if last_saved { " | save written" } else { "" },
                                audio_note
                            );
                            last_title = Instant::now();
                            log.flush().map_err(|e| e.to_string())?;
                        }
                        let pixels = active.session.with_link(|link| {
                            link.video_buffer(active.player)
                                .map(|b| b.to_vec())
                                .ok_or("Missing video buffer")
                        })?;
                        ui.show_game(&pixels, &title)?;
                    } else {
                        ui.show_setup(&status, true)?;
                    }
                } else {
                    ui.show_setup(&status, true)?;
                }
            } else {
                ui.show_setup(&status, false)?;
            }
            Ok(())
        })();
        if let Err(e) = step {
            if let Some(active) = &mut game {
                if let Err(capture_error) = active.capture_failure(&e) {
                    writeln!(log, "CAPTURE FAILED {capture_error}")
                        .map_err(|err| err.to_string())?;
                }
            }
            writeln!(log, "STOP {e}").map_err(|err| err.to_string())?;
            log.flush().map_err(|err| err.to_string())?;
            // Keep the complete reason accessible even when it is longer than
            // the small status panel. Do not replace it with a generic message.
            let _ = fs::write(root.join("last-error.txt"), format!("{e}\n"));
            if let Some(network) = wire.take() {
                let _ = send(&network, Message::Leave);
            }
            game = None;
            audio = None;
            files = None;
            save_candidate = None;
            last_saved = false;
            status = e;
            ui.set_connect_code(String::new());
            ui.show_setup(&status, false)?;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if let Some(network) = wire {
        let _ = send(&network, Message::Leave);
    }
    log.flush().map_err(|e| e.to_string())?;
    Ok(())
}
