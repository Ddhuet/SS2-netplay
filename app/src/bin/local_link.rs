use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use mgba::input::keys;
use mgba_rollback::{Link, LinkOptions, Peripheral, SideOptions};
use minifb::{Key, Scale, Window, WindowOptions};
use ss2_rollback_harness::determinism::{self, Recorder};

const WIDTH: usize = 240;
const HEIGHT: usize = 160;
const FPS: u32 = 60;
const DEFAULT_RTC_SECONDS: u64 = 1_752_000_000;

#[derive(Debug)]
struct Config {
    rom: PathBuf,
    saves: [Option<PathBuf>; 2],
    rtc_seconds: u64,
    record: Option<PathBuf>,
    verify: Option<PathBuf>,
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
    if let Some(dir) = &config.verify {
        return determinism::verify(dir, &rom);
    }
    let saves = [
        config.saves[0]
            .as_deref()
            .map(|path| read_file(path, "player 1 save"))
            .transpose()?,
        config.saves[1]
            .as_deref()
            .map(|path| read_file(path, "player 2 save"))
            .transpose()?,
    ];
    let rtc = UNIX_EPOCH
        .checked_add(Duration::from_secs(config.rtc_seconds))
        .ok_or_else(|| "--rtc-seconds is outside SystemTime's range".to_owned())?;

    println!("SS2 local cable link");
    println!(
        "ROM: {} ({} bytes, loaded independently by both cores)",
        config.rom.display(),
        rom.len()
    );
    for (player, (path, save)) in config.saves.iter().zip(&saves).enumerate() {
        match (path, save) {
            (Some(path), Some(bytes)) => println!(
                "Player {} save: {} ({} bytes, private in-memory copy)",
                player + 1,
                path.display(),
                bytes.len()
            ),
            _ => println!("Player {} save: <fresh private cartridge>", player + 1),
        }
    }

    let mut link = Link::with_options(LinkOptions {
        sides: vec![
            SideOptions {
                rom: rom.clone(),
                save: saves[0].clone(),
            },
            SideOptions {
                rom: rom.clone(),
                save: saves[1].clone(),
            },
        ],
        rtc: Some(rtc),
        peripheral: Peripheral::Cable,
    })
    .map_err(|error| format!("failed to boot the linked GBA cores: {error}"))?;

    let mut recorder = config
        .record
        .as_ref()
        .map(|dir| Recorder::new(dir, &rom, &saves, config.rtc_seconds, &mut link))
        .transpose()?;

    let options = WindowOptions {
        resize: true,
        scale: Scale::X2,
        ..WindowOptions::default()
    };
    let mut windows = [
        Window::new(
            "Shining Soul II - Player 1 (Cable Host)",
            WIDTH,
            HEIGHT,
            options,
        )
        .map_err(|error| format!("failed to open player 1 window: {error}"))?,
        Window::new("Shining Soul II - Player 2", WIDTH, HEIGHT, options)
            .map_err(|error| format!("failed to open player 2 window: {error}"))?,
    ];
    for window in &mut windows {
        window.set_target_fps(0);
    }

    if recorder.is_some() {
        println!("RECORDING: F9 finishes and closes both windows. Esc/window close also finalizes. Headless rollback verification starts automatically.");
    }
    println!("Both GBAs are connected from reset. Close either window or press Esc to quit.");
    println!("P1: arrows, Z=A, X=B, Enter=Start, Right Shift=Select, C=L, V=R");
    println!("P2: W/A/S/D, N=A, M=B, Space=Start, B=Select, Q=L, E=R");

    let frame_time = Duration::from_secs_f64(1.0 / f64::from(FPS));
    let mut frames = [vec![0_u32; WIDTH * HEIGHT], vec![0_u32; WIDTH * HEIGHT]];
    let mut deadline = Instant::now();

    while windows.iter().all(Window::is_open)
        && !windows
            .iter()
            .any(|window| window.is_key_down(Key::Escape) || window.is_key_down(Key::F9))
    {
        let active_window = windows.iter_mut().position(|window| window.is_active());
        let masks = active_window
            .map(|active| {
                [
                    player_1_keys(&windows[active]),
                    player_2_keys(&windows[active]),
                ]
            })
            .unwrap_or([0, 0]);
        link.try_tick(&masks)
            .map_err(|error| format!("linked emulation failed: {error}"))?;

        for player in 0..2 {
            let native = link
                .video_buffer(player)
                .ok_or_else(|| format!("player {} has no video buffer", player + 1))?;
            convert_xbgr1555(native, &mut frames[player])?;
            windows[player]
                .update_with_buffer(&frames[player], WIDTH, HEIGHT)
                .map_err(|error| format!("failed to present player {}: {error}", player + 1))?;
            link.core_mut(player).audio_buffer().clear();
        }

        if let Some(recorder) = &mut recorder {
            recorder.record(&masks, &mut link)?;
        }

        deadline += frame_time;
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        } else if now.duration_since(deadline) > frame_time.saturating_mul(4) {
            // Do not run a large catch-up burst after dragging or pausing a window.
            deadline = now;
        }
    }

    drop(windows);
    if let Some(recorder) = recorder {
        recorder.finish(&mut link)?;
        drop(link);
        determinism::verify(config.record.as_ref().unwrap(), &rom)?;
    }
    Ok(())
}

fn player_1_keys(window: &Window) -> u32 {
    key_mask(
        window,
        [
            (Key::Z, keys::A),
            (Key::X, keys::B),
            (Key::RightShift, keys::SELECT),
            (Key::Enter, keys::START),
            (Key::Right, keys::RIGHT),
            (Key::Left, keys::LEFT),
            (Key::Up, keys::UP),
            (Key::Down, keys::DOWN),
            (Key::V, keys::R),
            (Key::C, keys::L),
        ],
    )
}

fn player_2_keys(window: &Window) -> u32 {
    key_mask(
        window,
        [
            (Key::N, keys::A),
            (Key::M, keys::B),
            (Key::B, keys::SELECT),
            (Key::Space, keys::START),
            (Key::D, keys::RIGHT),
            (Key::A, keys::LEFT),
            (Key::W, keys::UP),
            (Key::S, keys::DOWN),
            (Key::E, keys::R),
            (Key::Q, keys::L),
        ],
    )
}

fn key_mask(window: &Window, bindings: [(Key, u32); 10]) -> u32 {
    bindings
        .into_iter()
        .filter(|(key, _)| window.is_key_down(*key))
        .fold(0, |mask, (_, button)| mask | button)
}

fn convert_xbgr1555(source: &[u8], destination: &mut [u32]) -> Result<(), String> {
    if source.len() != WIDTH * HEIGHT * 2 || destination.len() != WIDTH * HEIGHT {
        return Err(format!(
            "unexpected framebuffer size: {} bytes and {} destination pixels",
            source.len(),
            destination.len()
        ));
    }
    for (bytes, pixel) in source.chunks_exact(2).zip(destination) {
        let native = u16::from_ne_bytes([bytes[0], bytes[1]]);
        let red5 = u32::from(native & 0x1f);
        let green5 = u32::from((native >> 5) & 0x1f);
        let blue5 = u32::from((native >> 10) & 0x1f);
        let red8 = (red5 << 3) | (red5 >> 2);
        let green8 = (green5 << 3) | (green5 >> 2);
        let blue8 = (blue5 << 3) | (blue5 >> 2);
        *pixel = (red8 << 16) | (green8 << 8) | blue8;
    }
    Ok(())
}

fn read_file(path: &Path, kind: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("cannot read {kind} {}: {error}", path.display()))
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<Config>, String> {
    let mut rom = None;
    let mut saves = [None, None];
    let mut rtc_seconds = DEFAULT_RTC_SECONDS;
    let mut record = None;
    let mut verify = None;
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
            "--record" => record = Some(PathBuf::from(value)),
            "--verify" => verify = Some(PathBuf::from(value)),
            "--rom" => rom = Some(PathBuf::from(value)),
            "--save1" => saves[0] = Some(PathBuf::from(value)),
            "--save2" => saves[1] = Some(PathBuf::from(value)),
            "--rtc-seconds" => {
                rtc_seconds = value
                    .to_str()
                    .ok_or_else(|| "--rtc-seconds is not valid UTF-8".to_owned())?
                    .parse()
                    .map_err(|_| {
                        format!("invalid --rtc-seconds value: {}", value.to_string_lossy())
                    })?;
            }
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if record.is_some() && verify.is_some() {
        return Err("--record and --verify are mutually exclusive".into());
    }
    Ok(Some(Config {
        rom: rom.ok_or_else(|| "--rom is required".to_owned())?,
        saves,
        rtc_seconds,
        record,
        verify,
    }))
}

fn print_help() {
    println!(
        "ss2-local-link --rom PATH [options]\n\
         \n\
         Opens two linked GBA windows driven by one deterministic local cable Link.\n\
         Input save files are read into separate memory and are never overwritten.\n\
         \n\
         Options:\n\
           --record DIR          record latched inputs; F9 finishes (new directory)\n\
           --verify DIR          headless replay + snapshot verification\n\
           --save1 PATH          raw battery save for player 1 (default: fresh)\n\
           --save2 PATH          raw battery save for player 2 (default: fresh)\n\
           --rtc-seconds N       fixed Unix clock (default: {DEFAULT_RTC_SECONDS})\n\
           -h, --help            show this help\n\
         \n\
         P1: arrows, Z=A, X=B, Enter=Start, Right Shift=Select, C=L, V=R\n\
         P2: W/A/S/D, N=A, M=B, Space=Start, B=Select, Q=L, E=R"
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn xbgr1555_primary_colors_convert_to_rgb() {
        let source = [0x1f, 0x00, 0xe0, 0x03, 0x00, 0x7c];
        let mut destination = [0; 3];
        // Use a tiny equivalent loop because the production converter enforces
        // the exact GBA framebuffer size.
        for (bytes, pixel) in source.chunks_exact(2).zip(&mut destination) {
            let native = u16::from_ne_bytes([bytes[0], bytes[1]]);
            let expand = |value: u32| (value << 3) | (value >> 2);
            *pixel = (expand(u32::from(native & 0x1f)) << 16)
                | (expand(u32::from((native >> 5) & 0x1f)) << 8)
                | expand(u32::from((native >> 10) & 0x1f));
        }
        assert_eq!(destination, [0xff0000, 0x00ff00, 0x0000ff]);
    }
}
