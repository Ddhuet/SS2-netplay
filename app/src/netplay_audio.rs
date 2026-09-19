//! Local-only playback. mGBA resamples into a bounded host queue; all cores still
//! run their audio hardware. No audio travels across the connection.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mgba::audio::{AudioResampler, OwnedAudioBuffer};
use mgba_rollback::session::Session;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

pub struct Audio {
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<[f32; 2]>>>,
    resampler: AudioResampler,
    output: OwnedAudioBuffer,
    rate: u32,
    volume: Arc<AtomicU8>,
}

impl Audio {
    pub fn new() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("No audio output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let queue = Arc::new(Mutex::new(VecDeque::<[f32; 2]>::new()));
        let channels = config.channels as usize;
        let source = queue.clone();
        let volume = Arc::new(AtomicU8::new(100));
        let callback_volume = volume.clone();
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |out: &mut [f32], _| fill(out, channels, &source, &callback_volume, |x| x),
                |_| {},
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config,
                move |out: &mut [i16], _| {
                    fill(out, channels, &source, &callback_volume, |x| {
                        (x * 32767.0) as i16
                    })
                },
                |_| {},
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                &config,
                move |out: &mut [u16], _| {
                    fill(out, channels, &source, &callback_volume, |x| {
                        ((x + 1.0) * 32767.5) as u16
                    })
                },
                |_| {},
                None,
            ),
            other => return Err(format!("Unsupported audio format: {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            queue,
            resampler: AudioResampler::new(),
            output: OwnedAudioBuffer::new(4096, 2),
            rate: config.sample_rate.0,
            volume,
        })
    }

    pub fn set_volume(&self, volume: u8) {
        self.volume.store(volume.min(100), Ordering::Relaxed);
    }

    pub fn pump(&mut self, session: &Session, player: usize) {
        session.with_link(|link| {
            let core = link.core_mut(player);
            let rate = core.audio_sample_rate();
            self.resampler
                .set_source(core.audio_buffer(), rate as f64, true);
            self.resampler
                .set_destination(&mut self.output, self.rate as f64);
            self.resampler.process();
            let count = self.output.available();
            let mut samples = vec![0i16; count * 2];
            self.output.read(&mut samples, count);
            let mut queue = self.queue.lock().unwrap();
            // At most 60 ms handed to the audio device; Session repairs the
            // samples still in mGBA's ring during rollback.
            let cap = self.rate as usize * 60 / 1000;
            for frame in samples.chunks_exact(2) {
                if queue.len() >= cap {
                    queue.pop_front();
                }
                queue.push_back([frame[0] as f32 / 32768.0, frame[1] as f32 / 32768.0]);
            }
            link.core_mut(1 - player).audio_buffer().clear();
        });
    }
}

fn fill<T: Copy>(
    out: &mut [T],
    channels: usize,
    queue: &Mutex<VecDeque<[f32; 2]>>,
    volume: &AtomicU8,
    convert: impl Fn(f32) -> T,
) {
    let gain = f32::from(volume.load(Ordering::Relaxed).min(100)) / 100.0;
    let mut queue = queue.lock().unwrap();
    for frame in out.chunks_mut(channels) {
        let sample = queue.pop_front().unwrap_or([0.0; 2]);
        for (ch, value) in frame.iter_mut().enumerate() {
            *value = convert(if channels == 1 {
                (sample[0] + sample[1]) * 0.5 * gain
            } else if ch < 2 {
                sample[ch] * gain
            } else {
                0.0
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(frames: &[[f32; 2]]) -> Mutex<VecDeque<[f32; 2]>> {
        Mutex::new(frames.iter().copied().collect())
    }

    #[test]
    fn fill_f32_applies_full_half_and_mute_to_stereo() {
        for (volume, expected) in [(100, [1.0, -0.5]), (50, [0.5, -0.25]), (0, [0.0, -0.0])] {
            let source = queue(&[[1.0, -0.5]]);
            let atomic = AtomicU8::new(volume);
            let mut out = [0.0; 2];
            fill(&mut out, 2, &source, &atomic, |x| x);
            assert_eq!(out, expected);
            assert!(source.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn fill_f32_mixes_mono_before_applying_volume() {
        for (volume, expected) in [(100, 0.25), (50, 0.125), (0, 0.0)] {
            let source = queue(&[[0.75, -0.25]]);
            let atomic = AtomicU8::new(volume);
            let mut out = [0.0; 1];
            fill(&mut out, 1, &source, &atomic, |x| x);
            assert_eq!(out, [expected]);
            assert!(source.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn fill_signed_output_keeps_zero_at_every_volume() {
        for volume in [100, 50, 0] {
            let source = queue(&[[0.0, 0.0]]);
            let atomic = AtomicU8::new(volume);
            let mut out = [1_i16; 2];
            fill(&mut out, 2, &source, &atomic, |x| (x * 32767.0) as i16);
            assert_eq!(out, [0, 0]);
        }
    }

    #[test]
    fn fill_unsigned_output_keeps_zero_at_the_unsigned_midpoint() {
        for volume in [100, 50, 0] {
            let source = queue(&[[0.0, 0.0]]);
            let atomic = AtomicU8::new(volume);
            let mut out = [0_u16; 2];
            fill(&mut out, 2, &source, &atomic, |x| {
                ((x + 1.0) * 32767.5) as u16
            });
            assert_eq!(out, [32767, 32767]);
        }
    }

    #[test]
    fn fill_clamps_out_of_range_volume_and_still_drains_at_mute() {
        let source = queue(&[[1.0, 1.0], [-1.0, -1.0]]);
        let atomic = AtomicU8::new(255);
        let mut out = [0.0; 2];
        fill(&mut out, 2, &source, &atomic, |x| x);
        assert_eq!(out, [1.0, 1.0]);

        atomic.store(0, Ordering::Relaxed);
        let mut out = [1.0; 2];
        fill(&mut out, 2, &source, &atomic, |x| x);
        assert_eq!(out, [0.0, 0.0]);
        assert!(source.lock().unwrap().is_empty());
    }
}
