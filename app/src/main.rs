use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, UNIX_EPOCH};

use mgba_rollback::session::{Outgoing, Report, Session};
use mgba_rollback::{Link, LinkOptions, Peripheral, SideOptions, MAX_CABLE_PLAYERS};

const DEFAULT_PLAYERS: usize = 2;
const DEFAULT_LATENCY: u32 = 5;
const DEFAULT_DELAY: u32 = 2;
const DEFAULT_TICKS: u32 = 600;
const DEFAULT_REPORT_EVERY: u32 = 60;
const DEFAULT_RTC_SECONDS: u64 = 1_752_000_000;

#[derive(Debug)]
struct Config {
    rom: PathBuf,
    saves: Vec<Option<PathBuf>>,
    players: usize,
    latency: u32,
    delay: u32,
    ticks: Option<u32>,
    report_every: u32,
    rtc_seconds: u64,
    input_script: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct Delivery {
    at: u32,
    packet: Outgoing,
}

#[derive(Default)]
struct PeerMetrics {
    rollback_events: u64,
    rollback_ticks: u64,
    max_rollback_depth: u32,
    max_speculative_depth: u32,
    max_slices_per_tick: u32,
    last: Report,
}

impl PeerMetrics {
    fn observe(&mut self, report: Report) {
        if report.rolled_back > 0 {
            self.rollback_events += 1;
            self.rollback_ticks += u64::from(report.rolled_back);
            self.max_rollback_depth = self.max_rollback_depth.max(report.rolled_back);
        }
        self.max_speculative_depth = self
            .max_speculative_depth
            .max(report.presented.saturating_sub(report.confirmed));
        self.max_slices_per_tick = self.max_slices_per_tick.max(report.slices_peak);
        self.last = report;
    }
}

struct DigestMetrics {
    by_tick: HashMap<u32, (u32, Vec<bool>)>,
    comparisons: u64,
    desyncs: u64,
}

impl DigestMetrics {
    fn new() -> Self {
        Self {
            by_tick: HashMap::new(),
            comparisons: 0,
            desyncs: 0,
        }
    }

    fn observe(&mut self, peer: usize, checkpoint: Option<(u32, u32)>, players: usize) {
        let Some((tick, digest)) = checkpoint else {
            return;
        };
        let entry = self
            .by_tick
            .entry(tick)
            .or_insert_with(|| (digest, vec![false; players]));
        if entry.1[peer] {
            return;
        }
        if entry.0 != digest {
            self.desyncs += 1;
            eprintln!(
                "DESYNC tick={tick} peer={peer} expected={:08x} actual={digest:08x}",
                entry.0
            );
        }
        entry.1[peer] = true;
        if entry.1.iter().all(|seen| *seen) {
            self.comparisons += 1;
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("run with --help for usage");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let Some(config) = parse_args(std::env::args_os().skip(1))? else {
        print_help();
        return Ok(());
    };

    mgba::log::install_default_logger();

    let rom = read_file(&config.rom, "ROM")?;
    let save_bytes = config
        .saves
        .iter()
        .enumerate()
        .map(|(i, path)| {
            path.as_deref()
                .map(|path| read_file(path, &format!("save{}", i + 1)))
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let scripted = config
        .input_script
        .as_deref()
        .map(|path| read_input_script(path, config.players))
        .transpose()?;
    let ticks = match (&scripted, config.ticks) {
        (Some(rows), Some(ticks)) if ticks as usize > rows.len() => {
            return Err(format!(
                "--ticks {ticks} exceeds the {} rows in --input-script",
                rows.len()
            ));
        }
        (Some(rows), Some(ticks)) => ticks.min(rows.len() as u32),
        (Some(rows), None) => u32::try_from(rows.len()).map_err(|_| "input script is too long")?,
        (None, Some(ticks)) => ticks,
        (None, None) => DEFAULT_TICKS,
    };
    if ticks == 0 {
        return Err("the run must contain at least one tick".to_owned());
    }

    let rtc = UNIX_EPOCH
        .checked_add(Duration::from_secs(config.rtc_seconds))
        .ok_or_else(|| "--rtc-seconds is outside SystemTime's range".to_owned())?;

    println!("SS2 rollback harness");
    println!("rom={} bytes={}", config.rom.display(), rom.len());
    for (i, (path, bytes)) in config.saves.iter().zip(&save_bytes).enumerate() {
        match (path, bytes) {
            (Some(path), Some(bytes)) => {
                println!("save{}={} bytes={}", i + 1, path.display(), bytes.len())
            }
            _ => println!("save{}=<fresh>", i + 1),
        }
    }
    println!(
        "players={} ticks={} simulated_latency={} presentation_delay={} rtc_seconds={} input={}",
        config.players,
        ticks,
        config.latency,
        config.delay,
        config.rtc_seconds,
        config
            .input_script
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "deterministic-generated".to_owned())
    );

    let mut peers = (0..config.players)
        .map(|local_player| {
            let sides = (0..config.players)
                .map(|player| SideOptions {
                    rom: rom.clone(),
                    save: save_bytes[player].clone(),
                })
                .collect();
            let link = Link::with_options(LinkOptions {
                sides,
                rtc: Some(rtc),
                peripheral: Peripheral::Cable,
            })
            .map_err(|error| format!("failed to boot peer {local_player}: {error}"))?;
            Session::new(link, local_player, config.delay)
                .map_err(|error| format!("failed to create peer {local_player}: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut wires = vec![vec![VecDeque::<Delivery>::new(); config.players]; config.players];
    let mut metrics = (0..config.players)
        .map(|_| PeerMetrics::default())
        .collect::<Vec<_>>();
    let mut digests = DigestMetrics::new();
    let started = Instant::now();

    println!(
        "frame peer frontier confirmed presented speculative rollback_depth rollback_events max_rollback digest"
    );

    for frame in 0..ticks {
        let keys = scripted
            .as_ref()
            .map(|rows| rows[frame as usize].as_slice())
            .unwrap_or_else(|| &[]);

        for player in 0..config.players {
            let keys = if scripted.is_some() {
                keys[player]
            } else {
                generated_keys(player, frame)
            };
            let (packet, report) = peers[player]
                .advance(keys)
                .map_err(|error| format!("peer {player} failed at frame {frame}: {error}"))?;
            if packet.tick != frame {
                return Err(format!(
                    "peer {player} emitted tick {}, expected {frame}",
                    packet.tick
                ));
            }
            metrics[player].observe(report);
            for (to, queues) in wires.iter_mut().enumerate() {
                if to != player {
                    queues[player].push_back(Delivery {
                        at: frame.saturating_add(config.latency),
                        packet,
                    });
                }
            }
        }

        for (to, queues) in wires.iter_mut().enumerate() {
            for (from, queue) in queues.iter_mut().enumerate() {
                if to == from {
                    continue;
                }
                while queue.front().is_some_and(|delivery| delivery.at <= frame) {
                    let delivery = queue.pop_front().expect("front just checked");
                    peers[to].add_remote_input(
                        from,
                        delivery.packet.keys,
                        delivery.packet.tick_advantage,
                    );
                }
            }
        }

        for (peer, session) in peers.iter_mut().enumerate() {
            digests.observe(peer, session.checkpoint(), config.players);
            // This executable has no audio device. Discard all presentation rings
            // so bounded mGBA buffers cannot hide whether new samples were made.
            session.with_link(|link| {
                for core in 0..link.num_players() {
                    link.core_mut(core).audio_buffer().clear();
                }
            });
        }

        if should_report(frame, ticks, config.report_every) {
            for (peer, (session, peer_metrics)) in peers.iter().zip(&metrics).enumerate() {
                print_peer(frame, peer, peer_metrics, session.checkpoint());
            }
        }
    }

    let elapsed = started.elapsed();
    println!("summary");
    for (peer, (session, peer_metrics)) in peers.iter().zip(&metrics).enumerate() {
        let checkpoint = session
            .checkpoint()
            .map(|(tick, digest)| format!("{tick}:{digest:08x}"))
            .unwrap_or_else(|| "none".to_owned());
        println!(
            "peer={} frontier={} confirmed={} presented={} rollback_events={} rollback_ticks={} max_rollback_depth={} max_speculative_depth={} max_slices_per_tick={} checkpoint={}",
            peer,
            peer_metrics.last.frontier,
            peer_metrics.last.confirmed,
            peer_metrics.last.presented,
            peer_metrics.rollback_events,
            peer_metrics.rollback_ticks,
            peer_metrics.max_rollback_depth,
            peer_metrics.max_speculative_depth,
            peer_metrics.max_slices_per_tick,
            checkpoint
        );
    }
    println!(
        "digest_comparisons={} detected_desyncs={} elapsed_seconds={:.3} effective_ticks_per_second={:.1}",
        digests.comparisons,
        digests.desyncs,
        elapsed.as_secs_f64(),
        f64::from(ticks) / elapsed.as_secs_f64()
    );

    if digests.desyncs > 0 {
        return Err(format!(
            "detected {} settled-state desync(s)",
            digests.desyncs
        ));
    }
    Ok(())
}

fn should_report(frame: u32, ticks: u32, every: u32) -> bool {
    frame == 0 || frame + 1 == ticks || (frame + 1).is_multiple_of(every)
}

fn print_peer(frame: u32, peer: usize, metrics: &PeerMetrics, checkpoint: Option<(u32, u32)>) {
    let digest = checkpoint
        .map(|(_, digest)| format!("{digest:08x}"))
        .unwrap_or_else(|| "--------".to_owned());
    let speculative = metrics
        .last
        .presented
        .saturating_sub(metrics.last.confirmed);
    println!(
        "{} {} {} {} {} {} {} {} {} {}",
        frame + 1,
        peer,
        metrics.last.frontier,
        metrics.last.confirmed,
        metrics.last.presented,
        speculative,
        metrics.last.rolled_back,
        metrics.rollback_events,
        metrics.max_rollback_depth,
        digest
    );
}

fn generated_keys(player: usize, frame: u32) -> u32 {
    let phase = frame / 7 + player as u32;
    phase.wrapping_mul(2_654_435_761) & 0x03ff
}

fn read_file(path: &Path, kind: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("cannot read {kind} {}: {error}", path.display()))
}

fn read_input_script(path: &Path, players: usize) -> Result<Vec<Vec<u32>>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read input script {}: {error}", path.display()))?;
    let mut rows = Vec::new();
    for (line_index, raw) in text.lines().enumerate() {
        let body = raw.split('#').next().unwrap_or_default().trim();
        if body.is_empty() {
            continue;
        }
        let fields = body
            .split(|character: char| character == ',' || character.is_ascii_whitespace())
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        if fields.len() != players + 1 {
            return Err(format!(
                "{}:{}: expected tick plus {players} key masks, found {} fields",
                path.display(),
                line_index + 1,
                fields.len()
            ));
        }
        let tick = parse_u32(fields[0], "script tick")?;
        let expected = rows.len() as u32;
        if tick != expected {
            return Err(format!(
                "{}:{}: expected tick {expected}, found {tick}",
                path.display(),
                line_index + 1
            ));
        }
        let masks = fields[1..]
            .iter()
            .map(|field| parse_key_mask(field))
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(masks);
    }
    if rows.is_empty() {
        return Err(format!("input script {} contains no rows", path.display()));
    }
    Ok(rows)
}

fn parse_key_mask(value: &str) -> Result<u32, String> {
    let mask = parse_u32(value, "key mask")?;
    if mask & !0x03ff != 0 {
        return Err(format!(
            "key mask {value} sets bits outside the GBA's 10 buttons"
        ));
    }
    Ok(mask)
}

fn parse_u32(value: &str, name: &str) -> Result<u32, String> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).map_err(|_| format!("invalid {name}: {value}"))
    } else {
        value
            .parse()
            .map_err(|_| format!("invalid {name}: {value}"))
    }
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<Config>, String> {
    let mut rom = None;
    let mut saves: [Option<PathBuf>; MAX_CABLE_PLAYERS] = std::array::from_fn(|_| None);
    let mut players = DEFAULT_PLAYERS;
    let mut latency = DEFAULT_LATENCY;
    let mut delay = DEFAULT_DELAY;
    let mut ticks = None;
    let mut report_every = DEFAULT_REPORT_EVERY;
    let mut rtc_seconds = DEFAULT_RTC_SECONDS;
    let mut input_script = None;
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        let flag = argument
            .to_str()
            .ok_or_else(|| format!("option name is not valid UTF-8: {argument:?}"))?;
        if flag == "--help" || flag == "-h" {
            return Ok(None);
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value after {flag}"))?;
        match flag {
            "--rom" => rom = Some(PathBuf::from(value)),
            "--save1" => saves[0] = Some(PathBuf::from(value)),
            "--save2" => saves[1] = Some(PathBuf::from(value)),
            "--save3" => saves[2] = Some(PathBuf::from(value)),
            "--save4" => saves[3] = Some(PathBuf::from(value)),
            "--input-script" => input_script = Some(PathBuf::from(value)),
            "--players" => players = parse_os_usize(&value, flag)?,
            "--latency" => latency = parse_os_u32(&value, flag)?,
            "--delay" => delay = parse_os_u32(&value, flag)?,
            "--ticks" => ticks = Some(parse_os_u32(&value, flag)?),
            "--report-every" => report_every = parse_os_u32(&value, flag)?,
            "--rtc-seconds" => rtc_seconds = parse_os_u64(&value, flag)?,
            _ => return Err(format!("unknown option: {flag}")),
        }
    }

    if !(2..=MAX_CABLE_PLAYERS).contains(&players) {
        return Err(format!(
            "--players must be between 2 and {MAX_CABLE_PLAYERS}"
        ));
    }
    if report_every == 0 {
        return Err("--report-every must be greater than zero".to_owned());
    }
    for (index, save) in saves.iter().enumerate().skip(players) {
        if save.is_some() {
            return Err(format!(
                "--save{} was supplied for a {players}-player run",
                index + 1
            ));
        }
    }
    Ok(Some(Config {
        rom: rom.ok_or_else(|| "--rom is required".to_owned())?,
        saves: saves.into_iter().take(players).collect(),
        players,
        latency,
        delay,
        ticks,
        report_every,
        rtc_seconds,
        input_script,
    }))
}

fn parse_os_usize(value: &OsString, flag: &str) -> Result<usize, String> {
    value
        .to_str()
        .ok_or_else(|| format!("{flag} value is not valid UTF-8"))?
        .parse()
        .map_err(|_| format!("invalid value for {flag}: {}", value.to_string_lossy()))
}

fn parse_os_u32(value: &OsString, flag: &str) -> Result<u32, String> {
    value
        .to_str()
        .ok_or_else(|| format!("{flag} value is not valid UTF-8"))?
        .parse()
        .map_err(|_| format!("invalid value for {flag}: {}", value.to_string_lossy()))
}

fn parse_os_u64(value: &OsString, flag: &str) -> Result<u64, String> {
    value
        .to_str()
        .ok_or_else(|| format!("{flag} value is not valid UTF-8"))?
        .parse()
        .map_err(|_| format!("invalid value for {flag}: {}", value.to_string_lossy()))
}

fn print_help() {
    println!(
        "ss2-rollback-harness --rom PATH [options]\n\
         \n\
         Options:\n\
           --save1 PATH          raw battery save for player 1 (default: fresh)\n\
           --save2 PATH          raw battery save for player 2 (default: fresh)\n\
           --save3/--save4 PATH  saves for optional players 3 and 4\n\
           --players N           cable players, 2-4 (default: {DEFAULT_PLAYERS})\n\
           --latency N           one-way packet latency in harness frames (default: {DEFAULT_LATENCY})\n\
           --delay N             presentation delay in ticks (default: {DEFAULT_DELAY})\n\
           --ticks N             host frames to run (default: script length or {DEFAULT_TICKS})\n\
           --report-every N      diagnostic interval (default: {DEFAULT_REPORT_EVERY})\n\
           --rtc-seconds N       fixed Unix clock for all cores (default: {DEFAULT_RTC_SECONDS})\n\
           --input-script PATH   rows: tick,key1,key2[,key3,key4]\n\
           -h, --help            show this help\n\
         \n\
         Key masks are decimal or 0x-prefixed hexadecimal held-button states.\n\
         Valid bits: A=001 B=002 Select=004 Start=008 Right=010 Left=020\n\
                     Up=040 Down=080 R=100 L=200 (hexadecimal)."
    );
}
