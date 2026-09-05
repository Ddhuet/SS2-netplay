//! Session adapter: only validated contiguous inputs enter the untagged FIFO.
use crate::netplay_wire::Message;
use mgba_rollback::{
    session::{Session, TickObserver},
    Link,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const RTC: u64 = 1_752_000_000;
pub const FRAME_RATE: f64 = 16_777_216.0 / 280_896.0;

#[derive(Clone)]
struct Checkpoint {
    hash: [u8; 32],
    save: Option<Vec<u8>>,
}
#[derive(Default)]
struct Observed {
    checkpoints: BTreeMap<u32, Checkpoint>,
    error: Option<String>,
}
struct Observer {
    shared: Arc<Mutex<Observed>>,
    player: usize,
}
impl TickObserver for Observer {
    fn on_tick(&mut self, link: &mut Link, tick: u32) {
        if tick % 60 != 0 {
            return;
        }
        let result = state_hash(link).map(|hash| Checkpoint {
            hash,
            save: link.export_save(self.player),
        });
        let mut obs = self.shared.lock().unwrap();
        match result {
            Ok(value) => {
                obs.checkpoints.insert(tick, value);
            }
            Err(e) => obs.error = Some(e),
        }
    }
    fn on_rewind(&mut self, tick: u32) {
        self.shared
            .lock()
            .unwrap()
            .checkpoints
            .retain(|t, _| *t <= tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_adapter_rejects_skips_duplicates_and_wrong_initial_state() {
        let rom = mgba_rollback::testrom::build_idle();
        let mut game = Game::new(&rom, &[None, None], 0).unwrap();
        assert!(game.receive(Message::Ready([99; 32])).is_err());
        game.receive(Message::Ready(game.initial_hash)).unwrap();
        assert!(game
            .receive(Message::Input {
                tick: 1,
                keys: 0,
                advantage: 0
            })
            .is_err());
        game.receive(Message::Input {
            tick: 0,
            keys: 0,
            advantage: 0,
        })
        .unwrap();
        assert!(game
            .receive(Message::Input {
                tick: 0,
                keys: 0,
                advantage: 0
            })
            .is_err());
        assert!(game
            .receive(Message::Input {
                tick: 1,
                keys: 0x8000,
                advantage: 0
            })
            .is_err());
    }
}

pub fn state_hash(link: &mut Link) -> Result<[u8; 32], String> {
    let mut hash = Sha256::new();
    hash.update(b"ss2-netplay-diagnostics-v1");
    for (name, bytes) in link.diagnostic_components().map_err(|e| e.to_string())? {
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(hash.finalize().into())
}

pub struct Game {
    pub session: Session,
    pub player: usize,
    pub initial_hash: [u8; 32],
    pub ready: bool,
    pub matched_hash_tick: u32,
    pub corrections: u64,
    pub max_depth: u32,
    pub last_depth: u32,
    observed: Arc<Mutex<Observed>>,
    sent_hash_tick: u32,
    local_hashes: BTreeMap<u32, Checkpoint>,
    remote_hashes: BTreeMap<u32, [u8; 32]>,
    next_remote: u32,
    last_input: Instant,
    started: Instant,
    last_hash_match: Instant,
}

impl Game {
    pub fn new(rom: &[u8], saves: &[Option<Vec<u8>>; 2], player: usize) -> Result<Self, String> {
        let link = crate::determinism::boot(rom, saves, RTC)?;
        let session = Session::new(link, player, 2).map_err(|e| e.to_string())?;
        session.with_link(|link| {
            for side in 0..2 {
                link.set_frameskip(side, 0);
            }
        });
        let initial_hash = session.with_link(state_hash)?;
        let observed = Arc::new(Mutex::new(Observed::default()));
        let mut game = Self {
            session,
            player,
            initial_hash,
            ready: false,
            matched_hash_tick: 0,
            corrections: 0,
            max_depth: 0,
            last_depth: 0,
            observed: observed.clone(),
            sent_hash_tick: 0,
            local_hashes: BTreeMap::new(),
            remote_hashes: BTreeMap::new(),
            next_remote: 0,
            last_input: Instant::now(),
            started: Instant::now(),
            last_hash_match: Instant::now(),
        };
        game.session.set_observer(Some(Box::new(Observer {
            shared: observed,
            player,
        })));
        Ok(game)
    }

    pub fn receive(&mut self, message: Message) -> Result<(), String> {
        match message {
            Message::Ready(hash) => {
                if self.ready {
                    return Err("Duplicate Ready message".into());
                }
                if hash != self.initial_hash {
                    return Err("Initial machine states differ; session cancelled".into());
                }
                self.ready = true;
                self.last_input = Instant::now();
                self.last_hash_match = Instant::now();
            }
            Message::Input {
                tick,
                keys,
                advantage,
            } => {
                if !self.ready {
                    return Err("Input before Ready".into());
                }
                if tick != self.next_remote || keys & !0x3ff != 0 {
                    return Err(format!(
                        "Invalid input sequence {tick}; expected {}",
                        self.next_remote
                    ));
                }
                if tick > self.session.frontier().saturating_add(120) {
                    return Err("Peer input queue exceeded limit".into());
                }
                self.session
                    .add_remote_input(1 - self.player, keys, advantage);
                self.next_remote = self
                    .next_remote
                    .checked_add(1)
                    .ok_or("Input counter exhausted")?;
                self.last_input = Instant::now();
            }
            Message::Hash { tick, hash } => {
                if !self.ready
                    || tick == 0
                    || tick % 60 != 0
                    || tick <= self.matched_hash_tick
                    || self.remote_hashes.contains_key(&tick)
                    || tick > self.session.frontier().saturating_add(120)
                {
                    return Err("Invalid state-hash boundary".into());
                }
                self.remote_hashes.insert(tick, hash);
                if self.remote_hashes.len() > 12 {
                    return Err("Peer hash queue exceeded limit".into());
                }
            }
            Message::Leave => return Err(
                "The other player disconnected. Your saved character remains in the save folder."
                    .into(),
            ),
        }
        Ok(())
    }

    pub fn can_advance(&self) -> bool {
        self.ready && (self.session.local_queue_length() < 10 || self.session.matchable() > 0)
    }

    pub fn advance(&mut self, keys: u32) -> Result<Message, String> {
        if self.session.frontier() >= u32::MAX - 1024 {
            return Err("Session duration limit reached".into());
        }
        let (packet, report) = self
            .session
            .advance(keys)
            .map_err(|e| format!("Emulation stopped: {e}"))?;
        self.last_depth = report.rolled_back;
        self.max_depth = self.max_depth.max(report.rolled_back);
        self.corrections += u64::from(report.rolled_back > 0);
        Ok(Message::Input {
            tick: packet.tick,
            keys: packet.keys,
            advantage: packet.tick_advantage,
        })
    }

    /// Hashes and saves are released only for observations the Session actually
    /// settled. Matched input rows alone are insufficient for this gate.
    pub fn synchronization(&mut self) -> Result<(Vec<Message>, Option<Vec<u8>>), String> {
        if !self.ready && self.started.elapsed() > Duration::from_secs(30) {
            return Err("Timed out waiting for the other player's emulator".into());
        }
        if self.ready && self.last_input.elapsed() > Duration::from_secs(10) {
            return Err("Connection timed out: no new player inputs for 10 seconds".into());
        }
        if self.ready && self.last_hash_match.elapsed() > Duration::from_secs(20) {
            return Err("State synchronization timed out".into());
        }
        let settled = self.session.checkpoint().map(|(t, _)| t).unwrap_or(0);
        let mut obs = self.observed.lock().unwrap();
        if let Some(error) = &obs.error {
            return Err(error.clone());
        }
        let mut outgoing = Vec::new();
        while self.sent_hash_tick + 60 <= settled {
            let tick = self.sent_hash_tick + 60;
            let value = obs
                .checkpoints
                .remove(&tick)
                .ok_or_else(|| format!("Missing settled observation at {tick}"))?;
            outgoing.push(Message::Hash {
                tick,
                hash: value.hash,
            });
            self.local_hashes.insert(tick, value);
            self.sent_hash_tick = tick;
        }
        if self.local_hashes.len() > 12 {
            return Err("Peer is not confirming state hashes".into());
        }
        let mut save = None;
        loop {
            let tick = self.matched_hash_tick + 60;
            let (Some(local), Some(remote)) =
                (self.local_hashes.get(&tick), self.remote_hashes.get(&tick))
            else {
                break;
            };
            if local.hash != *remote {
                return Err(format!(
                    "DESYNC at settled frame {tick}. Session stopped; reconnect to restart."
                ));
            }
            save = local.save.clone();
            self.local_hashes.remove(&tick);
            self.remote_hashes.remove(&tick);
            self.matched_hash_tick = tick;
            self.last_hash_match = Instant::now();
        }
        Ok((outgoing, save))
    }
}
