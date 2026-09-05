use mgba_rollback::{Link, LinkOptions, Peripheral, SideOptions};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

pub const INTERVAL: usize = 60;
pub type Fingerprint = Vec<(String, String)>;
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn fingerprint(link: &mut Link) -> Result<Fingerprint, String> {
    Ok(link
        .diagnostic_components()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(name, bytes)| (name, hash(&bytes)))
        .collect())
}
pub fn boot(rom: &[u8], saves: &[Option<Vec<u8>>; 2], rtc: u64) -> Result<Link, String> {
    Link::with_options(LinkOptions {
        sides: saves
            .iter()
            .map(|save| SideOptions {
                rom: rom.to_vec(),
                save: save.clone(),
            })
            .collect(),
        rtc: Some(
            UNIX_EPOCH
                .checked_add(Duration::from_secs(rtc))
                .ok_or("RTC out of range")?,
        ),
        peripheral: Peripheral::Cable,
    })
    .map_err(|e| e.to_string())
}
pub fn step(link: &mut Link, input: &[u32; 2]) -> Result<(), String> {
    link.try_tick(input).map_err(|e| e.to_string())?;
    drain(link);
    Ok(())
}
pub fn drain(link: &mut Link) {
    for p in 0..2 {
        link.core_mut(p).audio_buffer().clear();
    }
}
pub struct Recorder {
    dir: PathBuf,
    inputs: BufWriter<File>,
    checkpoints: BufWriter<File>,
    pub ticks: usize,
}
impl Recorder {
    pub fn new(
        dir: &Path,
        rom: &[u8],
        saves: &[Option<Vec<u8>>; 2],
        rtc: u64,
        link: &mut Link,
    ) -> Result<Self, String> {
        // A new directory prevents accidental replacement of a previous capture.
        fs::create_dir(dir).map_err(|e| format!("create recording {}: {e}", dir.display()))?;
        let mut manifest = format!(
            "ss2-recording-v1\nrom_sha256={}\nrtc_seconds={}\ncheckpoint_interval={}\n",
            hash(rom),
            rtc,
            INTERVAL
        );
        for (p, save) in saves.iter().enumerate() {
            if let Some(bytes) = save {
                fs::write(dir.join(format!("initial-p{}.sav", p + 1)), bytes)
                    .map_err(|e| e.to_string())?;
                manifest.push_str(&format!("save{}_sha256={}\n", p + 1, hash(bytes)));
            } else {
                manifest.push_str(&format!("save{}_sha256=fresh\n", p + 1));
            }
        }
        // Hash the executable so replay refuses an accidentally different build.
        let exe = fs::read(std::env::current_exe().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        manifest.push_str(&format!("recorder_sha256={}\n", hash(&exe)));
        fs::write(dir.join("recorder.exe"), &exe).map_err(|e| e.to_string())?;
        fs::write(dir.join("manifest.txt"), manifest).map_err(|e| e.to_string())?;
        let mut recorder = Self {
            dir: dir.to_owned(),
            inputs: BufWriter::new(
                File::create(dir.join("inputs.txt")).map_err(|e| e.to_string())?,
            ),
            checkpoints: BufWriter::new(
                File::create(dir.join("checkpoints.txt")).map_err(|e| e.to_string())?,
            ),
            ticks: 0,
        };
        recorder.checkpoint(link)?;
        Ok(recorder)
    }
    pub fn record(&mut self, masks: &[u32; 2], link: &mut Link) -> Result<(), String> {
        writeln!(
            self.inputs,
            "{}, 0x{:03x}, 0x{:03x}",
            self.ticks, masks[0], masks[1]
        )
        .map_err(|e| e.to_string())?;
        self.ticks += 1;
        if self.ticks % INTERVAL == 0 {
            self.checkpoint(link)?;
            self.inputs.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn checkpoint(&mut self, link: &mut Link) -> Result<(), String> {
        for (name, digest) in fingerprint(link)? {
            writeln!(self.checkpoints, "{} {} {}", self.ticks, name, digest)
                .map_err(|e| e.to_string())?;
        }
        self.checkpoints.flush().map_err(|e| e.to_string())
    }
    pub fn finish(mut self, link: &mut Link) -> Result<(), String> {
        if self.ticks % INTERVAL != 0 {
            self.checkpoint(link)?;
        }
        self.inputs.flush().map_err(|e| e.to_string())?;
        fs::write(
            self.dir.join("inputs.sha256"),
            hash(&fs::read(self.dir.join("inputs.txt")).map_err(|e| e.to_string())?),
        )
        .map_err(|e| e.to_string())?;
        fs::write(self.dir.join("complete.txt"), self.ticks.to_string())
            .map_err(|e| e.to_string())?;
        println!("Recorded {} ticks in {}", self.ticks, self.dir.display());
        Ok(())
    }
}
pub fn parse_inputs(text: &str) -> Result<Vec<[u32; 2]>, String> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let fields: Vec<_> = line
            .split('#')
            .next()
            .unwrap()
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|s| !s.is_empty())
            .collect();
        if fields.is_empty() {
            continue;
        }
        if fields.len() != 3 || fields[0].parse::<usize>().ok() != Some(rows.len()) {
            return Err(format!("invalid input row at tick {}", rows.len()));
        }
        let mut masks = [0; 2];
        for p in 0..2 {
            masks[p] = if let Some(s) = fields[p + 1].strip_prefix("0x") {
                u32::from_str_radix(s, 16)
            } else {
                fields[p + 1].parse()
            }
            .map_err(|_| "invalid key mask")?;
            if masks[p] & !0x3ff != 0 {
                return Err("key mask outside GBA buttons".into());
            }
        }
        rows.push(masks);
    }
    if rows.is_empty() {
        return Err("recording contains no input ticks".into());
    }
    Ok(rows)
}

pub fn verify(dir: &Path, rom: &[u8]) -> Result<(), String> {
    let manifest = fs::read_to_string(dir.join("manifest.txt")).map_err(|e| e.to_string())?;
    let value = |name: &str| -> Result<&str, String> {
        manifest
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{name}=")))
            .ok_or_else(|| format!("missing {name}"))
    };
    if manifest.lines().next() != Some("ss2-recording-v1") {
        return Err("unknown recording format".into());
    }
    if value("rom_sha256")? != hash(rom) {
        return Err("recording ROM hash mismatch".into());
    }
    let exe =
        fs::read(std::env::current_exe().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if value("recorder_sha256")? != hash(&exe) {
        return Err(
            "recording executable hash mismatch; use the original recorder executable".into(),
        );
    }
    let rtc = value("rtc_seconds")?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    let mut saves = [None, None];
    for p in 0..2 {
        let expected = value(if p == 0 {
            "save1_sha256"
        } else {
            "save2_sha256"
        })?;
        if expected != "fresh" {
            let bytes =
                fs::read(dir.join(format!("initial-p{}.sav", p + 1))).map_err(|e| e.to_string())?;
            if hash(&bytes) != expected {
                return Err(format!("initial save {} hash mismatch", p + 1));
            }
            saves[p] = Some(bytes);
        }
    }
    let input_bytes = fs::read(dir.join("inputs.txt")).map_err(|e| e.to_string())?;
    if fs::read_to_string(dir.join("inputs.sha256")).map_err(|e| e.to_string())?
        != hash(&input_bytes)
    {
        return Err("input stream hash mismatch".into());
    }
    let rows =
        parse_inputs(&fs::read_to_string(dir.join("inputs.txt")).map_err(|e| e.to_string())?)?;
    let completed: usize = fs::read_to_string(dir.join("complete.txt"))
        .map_err(|e| format!("recording incomplete: {e}"))?
        .parse()
        .map_err(|_| "invalid completion marker")?;
    if completed != rows.len() {
        return Err("input count differs from completion marker".into());
    }
    let mut report =
        BufWriter::new(File::create(dir.join("verification.txt")).map_err(|e| e.to_string())?);
    let mut recorded = std::collections::BTreeMap::<usize, Fingerprint>::new();
    for line in fs::read_to_string(dir.join("checkpoints.txt"))
        .map_err(|e| e.to_string())?
        .lines()
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 {
            return Err("malformed checkpoint".into());
        }
        let tick = fields[0].parse::<usize>().map_err(|e| e.to_string())?;
        recorded
            .entry(tick)
            .or_default()
            .push((fields[1].into(), fields[2].into()));
    }
    for tick in (0..=rows.len())
        .step_by(INTERVAL)
        .chain(std::iter::once(rows.len()))
    {
        if !recorded.contains_key(&tick) {
            return Err(format!("missing recorded checkpoint {tick}"));
        }
    }
    let mut golden = Vec::with_capacity(rows.len() + 1);
    for pass in 0..2 {
        let mut link = boot(rom, &saves, rtc)?;
        for tick in 0..=rows.len() {
            if tick > 0 {
                step(&mut link, &rows[tick - 1])?;
            }
            let actual = fingerprint(&mut link)?;
            let expected = if pass == 0 {
                recorded.get(&tick)
            } else {
                golden.get(tick)
            };
            if let Some(expected) = expected {
                if expected != &actual {
                    return mismatch(
                        dir,
                        &mut report,
                        &format!("fresh-pass-{pass}"),
                        tick,
                        expected,
                        &actual,
                        &mut link,
                    );
                }
            }
            if pass == 0 {
                golden.push(actual);
            }
            if tick % 600 == 0 {
                println!("Fresh replay {}: {tick}/{}", pass + 1, rows.len());
            }
        }
        writeln!(
            report,
            "PASS fresh replay {}: {} ticks",
            pass + 1,
            rows.len()
        )
        .map_err(|e| e.to_string())?;
        report.flush().map_err(|e| e.to_string())?;
    }
    verify_rollback(dir, rom, &saves, rtc, &rows, &golden, &mut report)?;
    let mut link = boot(rom, &saves, rtc)?;
    for start in (0..rows.len()).step_by(INTERVAL) {
        let end = (start + INTERVAL).min(rows.len());
        let snapshot = link.save().map_err(|e| e.to_string())?;
        for tick in start..end {
            step(&mut link, &rows[tick])?;
        }
        // Exercise the production Link snapshot without restoring save-memory on its behalf.
        link.load(&snapshot).map_err(|e| e.to_string())?;
        drain(&mut link);
        for tick in start..end {
            step(&mut link, &rows[tick])?;
            let actual = fingerprint(&mut link)?;
            if actual != golden[tick + 1] {
                return mismatch(
                    dir,
                    &mut report,
                    &format!("restore-from-{start}"),
                    tick + 1,
                    &golden[tick + 1],
                    &actual,
                    &mut link,
                );
            }
        }
        if start % 600 == 0 {
            println!("Snapshot replay: {end}/{}", rows.len());
        }
    }
    writeln!(
        report,
        "PASS snapshot/replay: every {}-tick segment, every replayed tick compared",
        INTERVAL
    )
    .map_err(|e| e.to_string())?;
    report.flush().map_err(|e| e.to_string())?;
    println!("PASS: recorded checkpoints, two fresh runs, and snapshot replay; {} input ticks. Report: {}", rows.len(), dir.join("verification.txt").display());
    Ok(())
}
fn mismatch(
    dir: &Path,
    report: &mut impl Write,
    phase: &str,
    tick: usize,
    expected: &Fingerprint,
    actual: &Fingerprint,
    link: &mut Link,
) -> Result<(), String> {
    let message = format!("DESYNC {phase} at boundary {tick}");
    writeln!(report, "{message}").map_err(|e| e.to_string())?;
    for (name, digest) in actual {
        let old = expected
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.as_str())
            .unwrap_or("missing");
        if old != digest {
            writeln!(report, "  {name}: expected {old}, actual {digest}")
                .map_err(|e| e.to_string())?;
        }
    }
    report.flush().map_err(|e| e.to_string())?;
    let dump = dir.join(format!("desync-{phase}-{tick}"));
    fs::create_dir_all(&dump).map_err(|e| e.to_string())?;
    for (name, bytes) in link.diagnostic_components().map_err(|e| e.to_string())? {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        fs::write(dump.join(format!("{safe}.bin")), bytes).map_err(|e| e.to_string())?;
    }
    Err(format!(
        "{message}; see {}",
        dir.join("verification.txt").display()
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_missing_ticks_and_invalid_buttons() {
        assert!(parse_inputs("0, 0, 0\n2, 0, 0").is_err());
        assert!(parse_inputs("0, 0x400, 0").is_err());
        assert!(parse_inputs("").is_err());
        assert_eq!(
            parse_inputs("# hi\n0, 0x001, 32\n1 0 0").unwrap(),
            vec![[1, 32], [0, 0]]
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn synthetic_cable_recording_replays_and_restores() {
        let dir = std::env::temp_dir().join(format!(
            "ss2-determinism-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let rom = mgba_rollback::testrom::build();
        let saves = [None, None];
        let mut link = boot(&rom, &saves, 1_752_000_000).unwrap();
        let mut recorder = Recorder::new(&dir, &rom, &saves, 1_752_000_000, &mut link).unwrap();
        for tick in 0..125 {
            let input = [
                if tick % 7 < 3 { 1 } else { 0 },
                if tick % 11 < 4 { 32 } else { 0 },
            ];
            step(&mut link, &input).unwrap();
            recorder.record(&input, &mut link).unwrap();
        }
        recorder.finish(&mut link).unwrap();
        verify(&dir, &rom).unwrap();
        let checkpoints = dir.join("checkpoints.txt");
        let original = fs::read_to_string(&checkpoints).unwrap();
        let mut corrupted = original.clone();
        let first_hash = corrupted
            .find(' ')
            .and_then(|i| corrupted[i + 1..].find(' ').map(|j| i + j + 2))
            .unwrap();
        corrupted.replace_range(
            first_hash..first_hash + 1,
            if &original[first_hash..first_hash + 1] == "0" {
                "1"
            } else {
                "0"
            },
        );
        fs::write(&checkpoints, corrupted).unwrap();
        assert!(verify(&dir, &rom).unwrap_err().contains("DESYNC"));
        fs::write(&checkpoints, original).unwrap();
        // Deliberately change a valid input and prove integrity validation detects it.
        let path = dir.join("inputs.txt");
        let text =
            fs::read_to_string(&path)
                .unwrap()
                .replacen("0, 0x001, 0x020", "0, 0x000, 0x020", 1);
        fs::write(&path, text).unwrap();
        assert!(verify(&dir, &rom)
            .unwrap_err()
            .contains("input stream hash mismatch"));
    }
}

#[derive(Default)]
struct Observations {
    ticks: std::collections::BTreeMap<usize, Fingerprint>,
    error: Option<String>,
    divergent: std::collections::BTreeMap<usize, Vec<(String, Vec<u8>)>>,
}
struct Observer(
    std::sync::Arc<std::sync::Mutex<Observations>>,
    std::sync::Arc<Vec<Fingerprint>>,
);
impl mgba_rollback::session::TickObserver for Observer {
    fn on_tick(&mut self, link: &mut Link, tick: u32) {
        let result = fingerprint(link);
        let mut obs = self.0.lock().unwrap();
        match result {
            Ok(value) => {
                if self
                    .1
                    .get(tick as usize)
                    .is_some_and(|expected| expected != &value)
                {
                    match link.diagnostic_components() {
                        Ok(bytes) => {
                            obs.divergent.insert(tick as usize, bytes);
                        }
                        Err(e) => obs.error = Some(e.to_string()),
                    }
                }
                obs.ticks.insert(tick as usize, value);
            }
            Err(e) => obs.error = Some(e),
        }
    }
    fn on_rewind(&mut self, tick: u32) {
        let mut obs = self.0.lock().unwrap();
        obs.ticks.retain(|t, _| *t <= tick as usize);
        obs.divergent.retain(|t, _| *t <= tick as usize);
    }
}
fn verify_rollback(
    dir: &Path,
    rom: &[u8],
    saves: &[Option<Vec<u8>>; 2],
    rtc: u64,
    rows: &[[u32; 2]],
    golden: &[Fingerprint],
    report: &mut impl Write,
) -> Result<(), String> {
    use mgba_rollback::session::{Outgoing, Session};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    let expected_for_observer = Arc::new(golden.to_vec());
    for latency in [0usize, 2, 5, 10] {
        let observations: Vec<_> = (0..2)
            .map(|_| Arc::new(Mutex::new(Observations::default())))
            .collect();
        let mut peers = Vec::new();
        for p in 0..2 {
            let mut session =
                Session::new(boot(rom, saves, rtc)?, p, 0).map_err(|e| e.to_string())?;
            // Match the recording's renderer configuration for all simulated cores.
            session.with_link(|link| {
                for core in 0..2 {
                    link.set_frameskip(core, 0);
                }
            });
            session.set_observer(Some(Box::new(Observer(
                observations[p].clone(),
                expected_for_observer.clone(),
            ))));
            peers.push(session);
        }
        let mut queues: [VecDeque<(usize, Outgoing)>; 2] = Default::default();
        let mut compared = [0usize; 2];
        let mut events = [0u64; 2];
        let mut depths = [0u32; 2];
        for frame in 0..rows.len() + latency + 2 {
            // Deliver in stable sender order, exactly once, before this frame's advance.
            for p in 0..2 {
                while queues[p].front().is_some_and(|(at, _)| *at <= frame) {
                    let (_, packet) = queues[p].pop_front().unwrap();
                    peers[p].add_remote_input(1 - p, packet.keys, packet.tick_advantage);
                }
            }
            if latency == 0 {
                let input = rows.get(frame).copied().unwrap_or([0, 0]);
                for p in 0..2 {
                    peers[p].add_remote_input(1 - p, input[1 - p], 0);
                }
            }
            for p in 0..2 {
                let input = rows.get(frame).copied().unwrap_or([0, 0]);
                let (packet, status) = peers[p].advance(input[p]).map_err(|e| e.to_string())?;
                if latency > 0 {
                    queues[1 - p].push_back((frame + latency, packet));
                }
                if status.rolled_back > 0 {
                    events[p] += 1;
                    depths[p] = depths[p].max(status.rolled_back);
                }
                let confirmed_inputs = peers[p].drain_confirmed();
                for (boundary, actual_input) in &confirmed_inputs {
                    let tick = *boundary as usize - 1;
                    if tick < rows.len() && actual_input.as_ref() != rows[tick] {
                        return Err(format!("settled input ordering mismatch at {tick}"));
                    }
                }
                let mut obs = observations[p].lock().unwrap();
                if let Some(e) = &obs.error {
                    return Err(e.clone());
                }
                let through = peers[p]
                    .checkpoint()
                    .map(|(tick, _)| tick as usize)
                    .unwrap_or(0)
                    .min(rows.len());
                while compared[p] < through {
                    let tick = compared[p] + 1;
                    let actual = obs.ticks.remove(&tick).ok_or_else(|| {
                        format!("missing settled observation peer {p} tick {tick}")
                    })?;
                    if actual != golden[tick] {
                        let message = format!("DESYNC rollback latency={latency} peer={p} first settled boundary={tick}");
                        writeln!(report, "{message}").map_err(|e| e.to_string())?;
                        for (name, digest) in &actual {
                            let expected = golden[tick]
                                .iter()
                                .find(|(n, _)| n == name)
                                .map(|(_, d)| d.as_str())
                                .unwrap_or("missing");
                            if expected != digest {
                                writeln!(report, "  {name}: expected {expected}, actual {digest}")
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                        let actual_bytes = obs
                            .divergent
                            .remove(&tick)
                            .ok_or("missing divergence bytes")?;
                        let mut reference = boot(rom, saves, rtc)?;
                        for input in &rows[..tick] {
                            step(&mut reference, input)?;
                        }
                        let expected_bytes = reference
                            .diagnostic_components()
                            .map_err(|e| e.to_string())?;
                        let dump =
                            dir.join(format!("rollback-latency{latency}-peer{p}-tick{tick}"));
                        dump_difference(&dump, &expected_bytes, &actual_bytes, report)?;
                        report.flush().map_err(|e| e.to_string())?;
                        return Err(message);
                    }
                    compared[p] = tick;
                }
                obs.ticks.retain(|t, _| *t > compared[p]);
                obs.divergent.retain(|t, _| *t > compared[p]);
                drop(obs);
                peers[p].with_link(drain);
            }
            if frame % 600 == 0 {
                println!("Rollback latency={latency}: {frame}/{}", rows.len());
            }
        }
        if compared != [rows.len(); 2] {
            return Err(format!(
                "rollback failed to settle all recorded ticks: {compared:?}"
            ));
        }
        writeln!(report, "PASS rollback latency={latency}: compared={compared:?}, correction_events={events:?}, max_depth={depths:?}").map_err(|e| e.to_string())?;
        report.flush().map_err(|e| e.to_string())?;
        if latency > 0 && events == [0, 0] {
            return Err(
                "INCONCLUSIVE: input recording did not provoke rollback corrections".into(),
            );
        }
    }
    Ok(())
}

fn dump_difference(
    dir: &Path,
    expected: &[(String, Vec<u8>)],
    actual: &[(String, Vec<u8>)],
    report: &mut impl Write,
) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for ((name, before), (other, after)) in expected.iter().zip(actual) {
        if name != other {
            return Err("component schema mismatch".into());
        }
        if before == after {
            continue;
        }
        let safe = name.replace('/', "_");
        fs::write(dir.join(format!("{safe}-expected.bin")), before).map_err(|e| e.to_string())?;
        fs::write(dir.join(format!("{safe}-actual.bin")), after).map_err(|e| e.to_string())?;
        let differences: Vec<_> = before
            .iter()
            .zip(after)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(32)
            .map(|(offset, (a, b))| format!("{offset:#x}:{a:02x}->{b:02x}"))
            .collect();
        writeln!(
            report,
            "  {name} lengths {} -> {}, first byte differences: {}",
            before.len(),
            after.len(),
            differences.join(", ")
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
