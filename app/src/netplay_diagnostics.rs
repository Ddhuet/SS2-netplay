//! Private local evidence, never sent to the peer or included in the package.
use crate::netplay_wire::Hello;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub struct Capture {
    directory: PathBuf,
    trace: BufWriter<File>,
    started: Instant,
}
impl Capture {
    pub fn new(
        root: &Path,
        hello: &Hello,
        saves: &[Option<Vec<u8>>; 2],
        player: usize,
        initial: [u8; 32],
    ) -> Result<Self, String> {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let directory = root.join("logs").join(format!("capture-{id}-p{player}"));
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let mut manifest = format!("capture_version=1\nseat={player}\nrtc={}\nplayers=2\ncheckpoint_interval=60\nrom_sha256={}\nbuild_sha256={}\ninitial_sha256={}\n", crate::netplay_game::RTC, hex(&hello.rom_hash), hex(&hello.build_hash), hex(&initial));
        for (seat, save) in saves.iter().enumerate() {
            if let Some(bytes) = save {
                fs::write(directory.join(format!("initial-player-{seat}.sav")), bytes)
                    .map_err(|e| e.to_string())?;
                manifest.push_str(&format!("save_{seat}={}\n", hex(&Sha256::digest(bytes))));
            } else {
                manifest.push_str(&format!("save_{seat}=absent\n"));
            }
        }
        fs::write(directory.join("manifest.txt"), manifest).map_err(|e| e.to_string())?;
        let trace =
            BufWriter::new(File::create(directory.join("events.txt")).map_err(|e| e.to_string())?);
        Ok(Self {
            directory,
            trace,
            started: Instant::now(),
        })
    }
    pub fn record(&mut self, event: &str) -> Result<(), String> {
        writeln!(self.trace, "{} {event}", self.started.elapsed().as_micros())
            .map_err(|e| e.to_string())
    }
    pub fn hashes(&mut self, tick: u32, components: &[(String, Vec<u8>)]) -> Result<(), String> {
        for (name, bytes) in components {
            self.record(&format!(
                "COMPONENT {tick} {name} {} {}",
                bytes.len(),
                hex(&Sha256::digest(bytes))
            ))?;
        }
        self.trace.flush().map_err(|e| e.to_string())
    }
    pub fn dump(
        &mut self,
        tick: u32,
        label: &str,
        components: &[(String, Vec<u8>)],
    ) -> Result<(), String> {
        let directory = self.directory.join(format!("{label}-{tick}"));
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let mut index = String::new();
        for (i, (name, bytes)) in components.iter().enumerate() {
            // Numeric filenames avoid interpreting component names as paths.
            fs::write(directory.join(format!("{i}.bin")), bytes).map_err(|e| e.to_string())?;
            index.push_str(&format!(
                "{i}.bin {name} {} {}\n",
                bytes.len(),
                hex(&Sha256::digest(bytes))
            ));
        }
        fs::write(directory.join("index.txt"), index).map_err(|e| e.to_string())?;
        self.trace.flush().map_err(|e| e.to_string())
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
