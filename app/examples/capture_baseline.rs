//! Offline direct replay of a diagnostic capture. Never writes playable saves.
use mgba_rollback::{
    session::{Session, TickObserver},
    Link,
};
use sha2::{Digest, Sha256};
use ss2_rollback_harness::determinism;
use std::sync::{Arc, Mutex};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    mgba::log::install_default_logger();
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 && args.len() != 5 {
        return Err("usage: capture_baseline ROM CAPTURE NEW_OUTPUT_DIRECTORY [--schedule|--isolate|--restore-probe]".into());
    }
    let isolate = args.get(4).map(|a| a == "--isolate").unwrap_or(false);
    let probe = args.get(4).map(|a| a == "--restore-probe").unwrap_or(false);
    let scheduled = isolate || args.get(4).map(|a| a == "--schedule").unwrap_or(false);
    if args.len() == 5 && !scheduled && !probe {
        return Err("unknown mode".into());
    }
    let capture = PathBuf::from(&args[2]);
    let output = PathBuf::from(&args[3]);
    let manifest = fs::read_to_string(capture.join("manifest.txt"))?;
    let manifest: BTreeMap<_, _> = manifest.lines().filter_map(|l| l.split_once('=')).collect();
    let seat: usize = manifest["seat"].parse()?;
    if seat > 1 {
        return Err("invalid seat".into());
    }
    let rom = fs::read(&args[1])?;
    if determinism::hash(&rom) != manifest["rom_sha256"] {
        return Err("ROM mismatch".into());
    }
    let mut saves = [None, None];
    for p in 0..2 {
        let expected = manifest[format!("save_{p}").as_str()];
        if expected != "absent" {
            let bytes = fs::read(capture.join(format!("initial-player-{p}.sav")))?;
            if determinism::hash(&bytes) != expected {
                return Err("save fixture mismatch".into());
            }
            saves[p] = Some(bytes);
        }
    }
    let mut inputs = [BTreeMap::<u32, u32>::new(), BTreeMap::<u32, u32>::new()];
    let mut expected = BTreeMap::<u32, BTreeMap<String, String>>::new();
    let mut events = Vec::new();
    for line in BufReader::new(fs::File::open(capture.join("events.txt"))?).lines() {
        let line = line?;
        let words: Vec<_> = line.split_whitespace().collect();
        if words.get(1) == Some(&"ADVANCE") {
            let tick = words[2].parse()?;
            if inputs[seat].insert(tick, words[3].parse()?).is_some() {
                return Err("duplicate advance".into());
            }
            events.push((true, tick, words[3].parse::<u32>()?, 0i16));
        } else if words.get(1) == Some(&"RECEIVE") && words.get(3) == Some(&"Input") {
            let tick = words[6].trim_end_matches(',').parse()?;
            if inputs[1 - seat]
                .insert(tick, words[8].trim_end_matches(',').parse()?)
                .is_some()
            {
                return Err("duplicate input".into());
            }
            events.push((
                false,
                tick,
                words[8].trim_end_matches(',').parse::<u32>()?,
                words[10].parse::<i16>()?,
            ));
        } else if words.get(1) == Some(&"COMPONENT") {
            expected
                .entry(words[2].parse()?)
                .or_default()
                .insert(words[3].into(), words[5].into());
        }
    }
    let end = *expected.keys().last().ok_or("no checkpoints")?;
    for p in 0..2 {
        for tick in 0..end {
            if !inputs[p].contains_key(&tick) {
                return Err(format!("missing input {p}/{tick}").into());
            }
        }
    }
    let rtc = manifest["rtc"].parse()?;
    let mut link = determinism::boot(&rom, &saves, rtc)?;
    for p in 0..2 {
        link.set_frameskip(p, 0);
    }
    let legacy_capture = !expected
        .values()
        .next()
        .ok_or("no component hashes")?
        .contains_key("core/0/video/contention");
    let mut initial = link.diagnostic_components()?;
    filter_legacy(&mut initial, legacy_capture);
    let mut initial_hash = Sha256::new();
    initial_hash.update(b"ss2-netplay-diagnostics-v1");
    for (name, bytes) in &initial {
        initial_hash.update((name.len() as u64).to_le_bytes());
        initial_hash.update(name.as_bytes());
        initial_hash.update((bytes.len() as u64).to_le_bytes());
        initial_hash.update(bytes);
    }
    if format!("{:x}", initial_hash.finalize()) != manifest["initial_sha256"] {
        return Err("initial state mismatch".into());
    }
    fs::create_dir(&output)?;
    let mut report = fs::File::create(output.join("report.txt"))?;
    if scheduled {
        writeln!(report,"Recorded rollback schedule; source seat={seat}; end={end}; diagnostic executable differs")?;
        let observations = Arc::new(Mutex::new(BTreeMap::new()));
        let mut session = Session::new(link, seat, 2)?;
        session.set_observer(Some(Box::new(Observer(observations.clone(), isolate))));
        let mut baseline = if isolate {
            Some(determinism::boot(&rom, &saves, rtc)?)
        } else {
            None
        };
        if let Some(link) = &mut baseline {
            for p in 0..2 {
                link.set_frameskip(p, 0);
            }
        }
        let mut baseline_tick = 0;
        let mut last_good = None;
        let mut checked = 0;
        for (advance, tick, keys, advantage) in events {
            if advance {
                if session.frontier() != tick {
                    return Err("advance frontier mismatch".into());
                }
                let (_, rollback) = session.advance(keys)?;
                if isolate && tick >= 65690 {
                    writeln!(report, "ADVANCE {tick} keys={keys} rollback={rollback:?}")?;
                }
                session.drain_confirmed();
                session.with_link(determinism::drain);
            } else {
                session.add_remote_input(1 - seat, keys, advantage);
            }
            let settled = session.checkpoint().map(|(t, _)| t).unwrap_or(0);
            loop {
                let stride = if isolate && checked >= 65700 { 1 } else { 60 };
                if checked + stride > settled || checked >= end {
                    break;
                }
                checked += stride;
                let mut components = observations
                    .lock()
                    .unwrap()
                    .remove(&checked)
                    .ok_or("missing observation")?;
                if !isolate {
                    filter_legacy(&mut components, legacy_capture);
                }
                let reference;
                let baseline_components;
                let checkpoint = if let Some(link) = &mut baseline {
                    while baseline_tick < checked {
                        determinism::step(
                            link,
                            &[inputs[0][&baseline_tick], inputs[1][&baseline_tick]],
                        )?;
                        baseline_tick += 1;
                    }
                    baseline_components = link.diagnostic_components()?;
                    reference = baseline_components
                        .iter()
                        .map(|(n, b)| (n.clone(), determinism::hash(b)))
                        .collect();
                    &reference
                } else {
                    baseline_components = Vec::new();
                    &expected[&checked]
                };
                if components.len() != checkpoint.len() {
                    return Err("component inventory differs".into());
                }
                let mismatches: Vec<_> = components
                    .iter()
                    .filter(|(name, bytes)| checkpoint.get(name) != Some(&determinism::hash(bytes)))
                    .collect();
                if !mismatches.is_empty() {
                    if isolate {
                        dump_components(
                            &output.join(format!("baseline-{checked}")),
                            &baseline_components,
                        )?;
                        dump_components(&output.join(format!("rollback-{checked}")), &components)?;
                        if let Some((tick, good)) = last_good.take() {
                            dump_components(&output.join(format!("matched-{tick}")), &good)?;
                        }
                    }
                    for (i, (name, bytes)) in mismatches.iter().enumerate() {
                        writeln!(
                            report,
                            "MISMATCH {checked} {name} replay={} capture={:?} bytes={i}.bin",
                            determinism::hash(bytes),
                            checkpoint.get(name)
                        )?;
                        fs::write(output.join(format!("{i}.bin")), bytes)?;
                    }
                    return Ok(());
                }
                if isolate && checked >= 65700 {
                    last_good = Some((checked, components));
                }
                if checked % 6000 == 0 {
                    writeln!(report, "MATCH through {checked}")?;
                    report.flush()?;
                }
            }
            if checked == end {
                writeln!(report, "MATCH all checkpoints through {end}")?;
                return Ok(());
            }
        }
        return Err(
            format!("capture ended before settling final checkpoint: {checked}/{end}").into(),
        );
    }
    writeln!(report, "Direct baseline; source seat={seat}; end={end}; capture_build={}; replay build differs (diagnostic tool)", manifest["build_sha256"])?;
    for tick in 1..=end {
        determinism::step(&mut link, &[inputs[0][&(tick - 1)], inputs[1][&(tick - 1)]])?;
        if probe && tick == 65728 {
            let snapshot = link.save()?;
            let before = link.diagnostic_components()?;
            let stall_masks: Vec<_> = (0..2)
                .map(|p| unsafe { (*link.core_mut(p).gba_mut().as_raw()).video.stallMask })
                .collect();
            writeln!(report, "STALL before={stall_masks:?}")?;
            let memory_runtime: Vec<_> = (0..2)
                .map(|p| unsafe {
                    let gba = &*link.core_mut(p).gba_mut().as_raw();
                    (gba.memory, (*gba.cpu).memory)
                })
                .collect();
            dump_runtime(&mut link, &output.join("runtime-before.txt"))?;
            let runtime: Vec<_> = (0..2)
                .map(|p| unsafe {
                    let gba = &*link.core_mut(p).gba_mut().as_raw();
                    let cpu = &*gba.cpu;
                    (
                        gba.lastJump,
                        cpu.shifterOperand,
                        cpu.shifterCarryOut,
                        gba.timing.root.is_null(),
                        gba.timing.reroot.is_null(),
                    )
                })
                .collect();
            let row = [inputs[0][&tick], inputs[1][&tick]];
            determinism::step(&mut link, &row)?;
            let expected_step = link.diagnostic_components()?;
            for (case, depth, keys) in [
                ("immediate", 0, [0, 0]),
                ("correct-speculation", 4, row),
                ("zero-speculation", 4, [0, 0]),
                ("all-speculation", 4, [0x3ff, 0x3ff]),
                ("queue0", 0, [0, 0]),
                ("queue1", 0, [0, 0]),
                ("queues", 0, [0, 0]),
                ("scratch", 0, [0, 0]),
                ("last-jump", 0, [0, 0]),
                ("all-runtime", 0, [0, 0]),
                ("memory-runtime", 0, [0, 0]),
                ("video-runtime", 0, [0, 0]),
            ] {
                link.load(&snapshot)?;
                if case == "immediate" {
                    writeln!(
                        report,
                        "STALL restored={:?}",
                        (0..2)
                            .map(|p| unsafe {
                                (*link.core_mut(p).gba_mut().as_raw()).video.stallMask
                            })
                            .collect::<Vec<_>>()
                    )?;
                }
                if case == "immediate" {
                    dump_runtime(&mut link, &output.join("runtime-restored.txt"))?;
                }
                let restored = link.diagnostic_components()?;
                if restored != before {
                    let names: Vec<_> = restored
                        .iter()
                        .zip(&before)
                        .filter(|(a, b)| a != b)
                        .map(|(a, _)| a.0.as_str())
                        .collect();
                    writeln!(report, "PROBE {case} restore_differences={names:?}")?;
                    dump_components(&output.join(format!("restored-{case}")), &restored)?;
                }
                writeln!(report, "PROBE {case} restore_equal={}", restored == before)?;
                for _ in 0..depth {
                    determinism::step(&mut link, &keys)?;
                }
                if depth > 0 {
                    link.load(&snapshot)?;
                }
                for p in 0..2 {
                    unsafe {
                        let gba = &mut *link.core_mut(p).gba_mut().as_raw();
                        let cpu = &mut *gba.cpu;
                        if case == "video-runtime" {
                            gba.video.stallMask = stall_masks[p];
                        }
                        if case == "memory-runtime" {
                            let memory = &memory_runtime[p].0;
                            gba.memory.waitstatesSeq16 = memory.waitstatesSeq16;
                            gba.memory.waitstatesSeq32 = memory.waitstatesSeq32;
                            gba.memory.waitstatesNonseq16 = memory.waitstatesNonseq16;
                            gba.memory.waitstatesNonseq32 = memory.waitstatesNonseq32;
                            gba.memory.activeRegion = memory.activeRegion;
                            gba.memory.prefetch = memory.prefetch;
                            gba.memory.lastPrefetchedPc = memory.lastPrefetchedPc;
                            gba.memory.biosPrefetch = memory.biosPrefetch;
                            cpu.memory = memory_runtime[p].1;
                        }
                        if case == "scratch" || case == "all-runtime" {
                            cpu.shifterOperand = runtime[p].1;
                            cpu.shifterCarryOut = runtime[p].2;
                        }
                        if case == "last-jump" || case == "all-runtime" {
                            gba.lastJump = runtime[p].0;
                        }
                        if case == "queues"
                            || case == "all-runtime"
                            || (case == "queue0" && p == 0)
                            || (case == "queue1" && p == 1)
                        {
                            assert!(
                                !runtime[p].3 && runtime[p].4,
                                "probe expects original root-only queue"
                            );
                            assert!(
                                gba.timing.root.is_null() && !gba.timing.reroot.is_null(),
                                "probe expects restored reroot-only queue"
                            );
                            gba.timing.root = gba.timing.reroot;
                            gba.timing.reroot = std::ptr::null_mut();
                        }
                    }
                }
                determinism::step(&mut link, &row)?;
                if case == "immediate" {}
                let actual = link.diagnostic_components()?;
                let differences: Vec<_> = actual
                    .iter()
                    .zip(&expected_step)
                    .filter(|(a, b)| a != b)
                    .map(|(a, _)| a.0.as_str())
                    .collect();
                writeln!(report, "PROBE {case} next_tick_differences={differences:?}")?;
                if !differences.is_empty() {
                    dump_components(&output.join(case), &actual)?;
                }
            }
            dump_components(&output.join("before"), &before)?;
            dump_components(&output.join("expected"), &expected_step)?;
            return Ok(());
        }
        if let Some(checkpoint) = expected.get(&tick) {
            let mut components = link.diagnostic_components()?;
            filter_legacy(&mut components, legacy_capture);
            if components.len() != checkpoint.len() {
                return Err("component inventory differs".into());
            }
            let mismatches: Vec<_> = components
                .iter()
                .filter(|(name, bytes)| checkpoint.get(name) != Some(&determinism::hash(bytes)))
                .collect();
            if !mismatches.is_empty() {
                for (i, (name, bytes)) in mismatches.iter().enumerate() {
                    writeln!(
                        report,
                        "MISMATCH {tick} {name} baseline={} capture={:?} bytes={i}.bin",
                        determinism::hash(bytes),
                        checkpoint.get(name)
                    )?;
                    fs::write(output.join(format!("{i}.bin")), bytes)?;
                }
                return Ok(());
            }
        }
        if tick % 6000 == 0 {
            writeln!(report, "MATCH through {tick}")?;
            report.flush()?;
        }
    }
    writeln!(report, "MATCH all checkpoints through {end}")?;
    Ok(())
}

type Components = Vec<(String, Vec<u8>)>;
// Old captures predate the added contention component. Only historical hash
// checks omit those two new fields; direct-vs-rollback isolation compares all.
fn filter_legacy(components: &mut Components, legacy: bool) {
    if legacy {
        components
            .retain(|(n, _)| n != "core/0/video/contention" && n != "core/1/video/contention");
    }
}
struct Observer(Arc<Mutex<BTreeMap<u32, Components>>>, bool);
impl TickObserver for Observer {
    fn on_tick(&mut self, link: &mut Link, tick: u32) {
        if tick % 60 == 0 || (self.1 && tick >= 65700) {
            self.0.lock().unwrap().insert(
                tick,
                link.diagnostic_components().expect("diagnostic state"),
            );
        }
    }
    fn on_rewind(&mut self, tick: u32) {
        self.0.lock().unwrap().retain(|t, _| *t <= tick);
    }
}

fn dump_components(path: &std::path::Path, components: &Components) -> std::io::Result<()> {
    fs::create_dir(path)?;
    let mut index = fs::File::create(path.join("index.txt"))?;
    for (i, (name, bytes)) in components.iter().enumerate() {
        fs::write(path.join(format!("{i}.bin")), bytes)?;
        writeln!(
            index,
            "{i}.bin {name} {} {}",
            bytes.len(),
            determinism::hash(bytes)
        )?;
    }
    Ok(())
}

fn dump_runtime(link: &mut Link, path: &std::path::Path) -> std::io::Result<()> {
    let mut out = fs::File::create(path)?;
    for p in 0..2 {
        unsafe {
            let g = &*link.core_mut(p).gba_mut().as_raw();
            let c = &*g.cpu;
            macro_rules! field { ($($v:expr),*) => { $(writeln!(out,"p{p} {}={:?}",stringify!($v),$v)?;)* }; }
            field!(
                c.cycles,
                c.nextEvent,
                c.memory.activeRegion,
                c.memory.activeMask,
                c.memory.activeSeqCycles16,
                c.memory.activeSeqCycles32,
                c.memory.activeNonseqCycles16,
                c.memory.activeNonseqCycles32,
                c.memory.accessSource,
                g.memory.activeRegion,
                g.memory.prefetch,
                g.memory.lastPrefetchedPc,
                g.memory.biosPrefetch,
                g.memory.waitstatesSeq16,
                g.memory.waitstatesSeq32,
                g.memory.waitstatesNonseq16,
                g.memory.waitstatesNonseq32,
                g.memory.activeDMA,
                g.performingDMA,
                g.dmaPC,
                g.biosStall,
                g.bus,
                g.earlyExit,
                g.cpuBlocked
            );
        }
    }
    Ok(())
}
