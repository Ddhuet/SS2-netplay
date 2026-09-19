//! Local preferences and nonblocking input capture; never touches a Session.
use crate::netplay_controls::{Binding, Settings};
use minifb::Key;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

pub struct Preferences {
    pub settings: Settings,
    pub capture: Option<usize>,
    pub status: String,
    pub save_failed: bool,
    previous: Vec<Binding>,
    suppressed: Vec<Binding>,
    revision: u64,
    writer: Option<Sender<(u64, Settings)>>,
    results: Receiver<(u64, Result<(), String>)>,
    worker: Option<JoinHandle<()>>,
}

impl Preferences {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        let missing = !path.exists();
        let (settings, warning) = Settings::load(&path);
        let (writer, work) = mpsc::channel::<(u64, Settings)>();
        let (done, results) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("netplay-settings".into())
            .spawn(move || {
                while let Ok(mut pending) = work.recv() {
                    // A slider drag can enqueue several values. Persist the latest.
                    while let Ok(newer) = work.try_recv() {
                        pending = newer;
                    }
                    let result = pending.1.save(&path);
                    let _ = done.send((pending.0, result));
                }
            })
            .map_err(|e| format!("Cannot start settings writer: {e}"))?;
        let mut this = Self {
            settings,
            capture: None,
            save_failed: warning.is_some(),
            status: warning.unwrap_or_else(|| "Settings: netplay-settings.txt (beside EXE)".into()),
            previous: Vec::new(),
            suppressed: Vec::new(),
            revision: 0,
            writer: Some(writer),
            results,
            worker: Some(worker),
        };
        if missing {
            this.save();
        }
        Ok(this)
    }

    fn save(&mut self) {
        self.revision += 1;
        if self
            .writer
            .as_ref()
            .unwrap()
            .send((self.revision, self.settings.clone()))
            .is_err()
        {
            self.save_failed = true;
            self.status = "Could not save settings: writer stopped".into();
        } else {
            self.status = "Saving netplay-settings.txt...".into();
            self.save_failed = false;
        }
    }

    pub fn poll_save(&mut self) {
        while let Ok((revision, result)) = self.results.try_recv() {
            if revision != self.revision {
                continue;
            }
            self.save_failed = result.is_err();
            self.status = match result {
                Ok(()) => "Saved: netplay-settings.txt (beside EXE)".into(),
                Err(e) => format!("Settings NOT saved: {e}"),
            };
        }
    }

    pub fn set_volume(&mut self, volume: u8) {
        let volume = volume.min(100);
        if self.settings.volume != volume {
            self.settings.volume = volume;
            self.save();
        }
    }

    pub fn begin_capture(&mut self, action: usize, active: &[Binding]) {
        self.capture = Some(action);
        self.previous = active.to_vec();
        self.suppressed = active.to_vec();
    }

    pub fn cancel_capture(&mut self, active: &[Binding]) {
        if self.capture.is_some() {
            self.suppressed = active.to_vec();
        }
        self.capture = None;
    }

    pub fn poll_inputs(&mut self, active: &[Binding]) {
        self.suppressed.retain(|b| active.contains(b));
        if let Some(action) = self.capture {
            // Consume all input during capture, including the newly bound button
            // until it is released. The game continues with neutral local input.
            self.suppressed = active.to_vec();
            if let Some(binding) = active.iter().find(|b| {
                !self.previous.contains(b) && !matches!(b, Binding::Keyboard(Key::Escape | Key::F1))
            }) {
                self.settings.bindings[action] = *binding;
                self.capture = None;
                self.save();
            }
        }
        self.previous = active.to_vec();
    }

    pub fn keys(&self, active: &[Binding]) -> u32 {
        if self.capture.is_some() {
            return 0;
        }
        let available: Vec<_> = active
            .iter()
            .copied()
            .filter(|b| {
                !self.suppressed.contains(b)
                    && !matches!(b, Binding::Keyboard(Key::Escape | Key::F1))
            })
            .collect();
        self.settings.mask(&available)
    }
}

impl Drop for Preferences {
    fn drop(&mut self) {
        // Flush only on shutdown, after the session loop has finished.
        self.writer.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preferences() -> (Preferences, Receiver<(u64, Settings)>) {
        let (writer, pending) = mpsc::channel();
        let (_, results) = mpsc::channel();
        (
            Preferences {
                settings: Settings::default(),
                capture: None,
                status: String::new(),
                save_failed: false,
                previous: Vec::new(),
                suppressed: Vec::new(),
                revision: 0,
                writer: Some(writer),
                results,
                worker: None,
            },
            pending,
        )
    }

    #[test]
    fn capture_requires_fresh_press_and_consumes_it_until_release() {
        let (mut p, pending) = preferences();
        let held = Binding::Keyboard(Key::Z);
        let new = Binding::Keyboard(Key::Space);
        p.begin_capture(3, &[held]);
        p.poll_inputs(&[held]);
        assert_eq!(p.capture, Some(3));
        assert_eq!(p.keys(&[held]), 0);
        p.poll_inputs(&[held, new]);
        assert_eq!(p.capture, None);
        assert_eq!(pending.try_recv().unwrap().1.bindings[3], new);
        assert_eq!(p.keys(&[held, new]), 0);
        p.poll_inputs(&[]);
        p.poll_inputs(&[new]);
        assert_eq!(p.keys(&[new]), mgba::input::keys::START);
    }

    #[test]
    fn shortcuts_cannot_be_captured_and_cancel_preserves_bindings() {
        let (mut p, _pending) = preferences();
        let old = p.settings.bindings[0];
        p.begin_capture(0, &[]);
        p.poll_inputs(&[Binding::Keyboard(Key::F1), Binding::Keyboard(Key::Escape)]);
        assert_eq!(p.capture, Some(0));
        p.cancel_capture(&[]);
        assert_eq!(p.settings.bindings[0], old);
        assert_eq!(p.keys(&[Binding::Keyboard(Key::Z)]), mgba::input::keys::A);
    }

    #[test]
    fn cancelling_capture_consumes_simultaneous_gameplay_press_until_release() {
        let (mut p, _pending) = preferences();
        let button = Binding::Keyboard(Key::Z);
        p.begin_capture(0, &[]);
        p.cancel_capture(&[button]);
        p.poll_inputs(&[button]);
        assert_eq!(p.keys(&[button]), 0);
        p.poll_inputs(&[]);
        p.poll_inputs(&[button]);
        assert_eq!(p.keys(&[button]), mgba::input::keys::A);
    }

    #[test]
    fn volume_changes_do_not_block_inputs_and_stale_save_results_are_ignored() {
        let (mut p, pending) = preferences();
        let (done, results) = mpsc::channel();
        p.results = results;
        p.set_volume(25);
        let (first, _) = pending.try_recv().unwrap();
        p.set_volume(50);
        let (latest, settings) = pending.try_recv().unwrap();
        assert_eq!(settings.volume, 50);
        assert_eq!(p.keys(&[Binding::Keyboard(Key::Z)]), mgba::input::keys::A);
        done.send((first, Err("old failure".into()))).unwrap();
        done.send((latest, Ok(()))).unwrap();
        p.poll_save();
        assert!(!p.save_failed);
        assert!(p.status.starts_with("Saved:"));
    }

    #[test]
    fn shutdown_flushes_latest_settings_and_next_session_loads_them() {
        let path = std::env::temp_dir().join(format!(
            "ss2-preferences-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut p = Preferences::new(path.clone()).unwrap();
            for volume in 0..=100 {
                p.set_volume(volume);
            }
            p.set_volume(37);
            p.begin_capture(0, &[]);
            p.poll_inputs(&[Binding::Keyboard(Key::Space)]);
        }
        let (loaded, warning) = Settings::load(&path);
        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(loaded.volume, 37);
        assert_eq!(loaded.bindings[0], Binding::Keyboard(Key::Space));
        std::fs::remove_file(path).unwrap();
    }
}
