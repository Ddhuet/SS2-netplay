//! Real local UDP/QUIC sockets, two replicated worlds, synthetic public test ROM.
//! This does not automate SS2 gameplay or test personal save persistence.
use crate::{
    netplay_game::{state_hash, Game, RTC},
    netplay_wire::{self, Event, Hello, Message},
};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn keys(tick: u32, player: usize) -> u32 {
    if (tick / 7 + player as u32) % 3 == 0 {
        1 << ((tick / 13 + player as u32) % 10)
    } else {
        0
    }
}

pub fn run(root: &Path) -> Result<String, String> {
    let rom = mgba_rollback::testrom::build();
    let saves = [None, None];
    let mut baseline = crate::determinism::boot(&rom, &saves, RTC)?;
    let mut expected = BTreeMap::new();
    for tick in 0..360 {
        baseline
            .try_tick(&[keys(tick, 0), keys(tick, 1)])
            .map_err(|e| e.to_string())?;
        for p in 0..2 {
            baseline.core_mut(p).audio_buffer().clear();
        }
        if (tick + 1) % 60 == 0 {
            expected.insert(tick + 1, state_hash(&mut baseline)?);
        }
    }
    drop(baseline);
    let bind = std::net::UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let address = bind.local_addr().map_err(|e| e.to_string())?;
    drop(bind);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let config = root.join("logs").join(format!("selftest-{stamp}"));
    let hello = Hello {
        rom_hash: [17; 32],
        build_hash: [23; 32],
        save: None,
    };
    let host = netplay_wire::spawn(true, address, hello.clone(), config.join("host"));
    // Wait for the bind operation before asking the guest to connect.
    match host
        .rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?
    {
        Event::Error(e) => return Err(e),
        _ => {}
    }
    let guest = netplay_wire::spawn(false, address, hello, config.join("guest"));
    let wires = [host, guest];
    let mut games: [Option<Game>; 2] = [None, None];
    let mut pending: [VecDeque<(Instant, Message)>; 2] = Default::default();
    let start = Instant::now();
    let mut next_tick = [Instant::now(); 2];
    loop {
        if start.elapsed() > Duration::from_secs(25) {
            return Err(format!(
                "Self-test timed out: {:?}",
                games.each_ref().map(|g| g
                    .as_ref()
                    .map(|g| (g.session.frontier(), g.matched_hash_tick)))
            ));
        }
        for p in 0..2 {
            while let Ok(event) = wires[p].rx.try_recv() {
                match event {
                    Event::Connected {
                        local_player,
                        saves,
                    } => {
                        if local_player != p {
                            return Err("Incorrect seat assignment".into());
                        }
                        let game = Game::new(&rom, &saves, p)?;
                        wires[p]
                            .tx
                            .try_send(Message::Ready(game.initial_hash))
                            .map_err(|e| e.to_string())?;
                        games[p] = Some(game);
                    }
                    Event::Message(message) => {
                        // Delayed application delivery exercises rollback on the
                        // same wire messages used by the playable harness.
                        let delay = if matches!(message, Message::Input { .. }) {
                            Duration::from_millis(55)
                        } else {
                            Duration::ZERO
                        };
                        pending[p].push_back((Instant::now() + delay, message));
                    }
                    Event::Error(e) => return Err(e),
                    Event::Status(_) => {}
                }
            }
            let Some(game) = &mut games[p] else {
                continue;
            };
            while pending[p]
                .front()
                .is_some_and(|(at, _)| *at <= Instant::now())
            {
                game.receive(pending[p].pop_front().unwrap().1)?;
            }
            if game.can_advance() && Instant::now() >= next_tick[p] && game.session.frontier() < 380
            {
                let tick = game.session.frontier();
                let message = game.advance(keys(tick, p))?;
                wires[p].tx.try_send(message).map_err(|e| e.to_string())?;
                for (boundary, row) in game.session.drain_confirmed() {
                    if row.as_ref() != [keys(boundary - 1, 0), keys(boundary - 1, 1)] {
                        return Err(format!("Input ordering mismatch at {boundary}"));
                    }
                }
                game.session.with_link(|link| {
                    for side in 0..2 {
                        link.core_mut(side).audio_buffer().clear();
                    }
                });
                next_tick[p] = Instant::now() + Duration::from_millis(16);
            }
            let (messages, _) = game.synchronization()?;
            for message in messages {
                if let Message::Hash { tick, hash } = &message {
                    if let Some(direct) = expected.get(tick) {
                        if direct != hash {
                            return Err(format!(
                                "Direct-baseline mismatch peer={p} boundary={tick}"
                            ));
                        }
                    }
                }
                wires[p].tx.try_send(message).map_err(|e| e.to_string())?;
            }
        }
        if games
            .iter()
            .all(|g| g.as_ref().is_some_and(|g| g.matched_hash_tick >= 360))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut summary = "PASS: two real QUIC endpoints; both peers matched the direct synthetic-ROM baseline at all six checkpoints through frame 360; all confirmed input rows ordered correctly.\n".to_string();
    for (p, game) in games.iter().enumerate() {
        let game = game.as_ref().unwrap();
        if game.corrections == 0 {
            return Err(format!("No corrections exercised on peer {p}"));
        }
        summary.push_str(&format!(
            "Peer {p}: {} corrections, maximum depth {}.\n",
            game.corrections, game.max_depth
        ));
        let _ = wires[p].tx.try_send(Message::Leave);
    }
    summary.push_str("No SS2 gameplay or in-game save-persistence check was performed.\n");
    Ok(summary)
}

#[cfg(test)]
mod tests {
    #[test]
    fn actual_quic_rollback_matches_direct_baseline() {
        let dir = std::env::temp_dir().join("ss2-netplay-synthetic-tests");
        super::run(&dir).unwrap();
    }
}
