//! Recheck captured inputs across production snapshot boundaries with current diagnostics.
//! Usage: diagnose_restore ROM RECORDING_DIR
use ss2_rollback_harness::determinism::{boot, fingerprint, hash, parse_inputs, step, INTERVAL};
use std::{fs, io::Write, path::PathBuf};
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    mgba::log::install_default_logger();
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 && args.len() != 4 {
        return Err("usage: diagnose_restore ROM RECORDING_DIR [RECERTIFICATION_DIR]".into());
    }
    let dir = PathBuf::from(&args[2]);
    let read = |name: &str| fs::read(dir.join(name)).map_err(|e| e.to_string());
    let manifest = String::from_utf8(read("manifest.txt")?).map_err(|e| e.to_string())?;
    let value = |name: &str| {
        manifest
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{name}=")))
            .ok_or_else(|| format!("missing {name}"))
    };
    let rom = fs::read(&args[1]).map_err(|e| e.to_string())?;
    if hash(&rom) != value("rom_sha256")? {
        return Err("ROM mismatch".into());
    }
    let mut saves = [None, None];
    for p in 0..2 {
        let expected = value(if p == 0 {
            "save1_sha256"
        } else {
            "save2_sha256"
        })?;
        if expected != "fresh" {
            let bytes = read(&format!("initial-p{}.sav", p + 1))?;
            if hash(&bytes) != expected {
                return Err("initial save mismatch".into());
            }
            saves[p] = Some(bytes);
        }
    }
    let inputs = read("inputs.txt")?;
    if hash(&inputs) != String::from_utf8(read("inputs.sha256")?).map_err(|e| e.to_string())? {
        return Err("input integrity mismatch".into());
    }
    let rows = parse_inputs(std::str::from_utf8(&inputs).map_err(|e| e.to_string())?)?;
    if read("complete.txt")? != rows.len().to_string().as_bytes() {
        return Err("incomplete recording".into());
    }
    let rtc = value("rtc_seconds")?.parse().map_err(|_| "invalid RTC")?;
    let mut reference = boot(&rom, &saves, rtc)?;
    let mut candidate = boot(&rom, &saves, rtc)?;
    let mut report = fs::File::create(dir.join("snapshot-verification-current.txt"))
        .map_err(|e| e.to_string())?;
    writeln!(
        report,
        "Current diagnostics recheck; original capture and verification.txt preserved."
    )
    .map_err(|e| e.to_string())?;
    for start in (0..rows.len()).step_by(INTERVAL) {
        let end = (start + INTERVAL).min(rows.len());
        let snapshot = candidate.save().map_err(|e| e.to_string())?;
        let mut expected = Vec::new();
        for input in &rows[start..end] {
            step(&mut reference, input)?;
            expected.push(fingerprint(&mut reference)?);
            step(&mut candidate, input)?;
            if fingerprint(&mut candidate)? != *expected.last().unwrap() {
                return Err(format!("forward state mismatch in segment {start}"));
            }
        }
        candidate.load(&snapshot).map_err(|e| e.to_string())?;
        ss2_rollback_harness::determinism::drain(&mut candidate);
        for (offset, input) in rows[start..end].iter().enumerate() {
            step(&mut candidate, input)?;
            let actual = fingerprint(&mut candidate)?;
            if actual != expected[offset] {
                writeln!(
                    report,
                    "DESYNC restore-from-{start} boundary={}",
                    start + offset + 1
                )
                .map_err(|e| e.to_string())?;
                for ((name, a), (_, b)) in expected[offset].iter().zip(&actual) {
                    if a != b {
                        writeln!(report, "{name}: {a} -> {b}").map_err(|e| e.to_string())?;
                    }
                }
                let mut diagnostic_reference = boot(&rom, &saves, rtc)?;
                for input in &rows[..start + offset + 1] {
                    step(&mut diagnostic_reference, input)?;
                }
                let expected_bytes = diagnostic_reference
                    .diagnostic_components()
                    .map_err(|e| e.to_string())?;
                let actual_bytes = candidate
                    .diagnostic_components()
                    .map_err(|e| e.to_string())?;
                for ((name, a), (_, b)) in expected_bytes.iter().zip(&actual_bytes) {
                    if a == b {
                        continue;
                    }
                    let safe = name.replace('/', "_");
                    fs::write(dir.join(format!("snapshot-{start}-{safe}-expected.bin")), a)
                        .map_err(|e| e.to_string())?;
                    fs::write(dir.join(format!("snapshot-{start}-{safe}-actual.bin")), b)
                        .map_err(|e| e.to_string())?;
                    for (offset, (x, y)) in a
                        .iter()
                        .zip(b)
                        .enumerate()
                        .filter(|(_, (x, y))| x != y)
                        .take(32)
                    {
                        writeln!(report, "{name} offset {offset:#x}: {x:02x} -> {y:02x}")
                            .map_err(|e| e.to_string())?;
                    }
                }
                return Err(format!(
                    "DESYNC restore-from-{start} boundary={}",
                    start + offset + 1
                ));
            }
        }
        if start % 600 == 0 {
            println!("Snapshot replay {end}/{}", rows.len());
        }
    }
    let result = format!("PASS: {} segments, {} replayed ticks, both cores, production Link::save/load with the current core", rows.len().div_ceil(INTERVAL),rows.len());
    writeln!(report, "{result}").map_err(|e| e.to_string())?;
    println!("{result}");
    if let Some(output) = args.get(3) {
        let output = PathBuf::from(output);
        let mut replay = boot(&rom, &saves, rtc)?;
        let mut recorder = ss2_rollback_harness::determinism::Recorder::new(
            &output,
            &rom,
            &saves,
            rtc,
            &mut replay,
        )?;
        for input in &rows {
            step(&mut replay, input)?;
            recorder.record(input, &mut replay)?;
        }
        recorder.finish(&mut replay)?;
        fs::write(output.join("provenance.txt"),format!("Re-executed original input stream from {} with current diagnostics/core. Original input SHA256={}\n",dir.display(),hash(&inputs))).map_err(|e|e.to_string())?;
        if fs::read(output.join("inputs.txt")).map_err(|e| e.to_string())? != inputs {
            return Err("recertification changed inputs".into());
        }
        ss2_rollback_harness::determinism::verify(&output, &rom)?;
    }
    Ok(())
}
