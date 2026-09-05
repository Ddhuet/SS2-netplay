//! Local-only playback. mGBA resamples into a bounded host queue; all cores still
//! run their audio hardware. No audio travels across the connection.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mgba::audio::{AudioResampler, OwnedAudioBuffer};
use mgba_rollback::session::Session;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct Audio {
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<[f32; 2]>>>,
    resampler: AudioResampler,
    output: OwnedAudioBuffer,
    rate: u32,
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
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |out: &mut [f32], _| fill(out, channels, &source, |x| x),
                |_| {},
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config,
                move |out: &mut [i16], _| fill(out, channels, &source, |x| (x * 32767.0) as i16),
                |_| {},
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                &config,
                move |out: &mut [u16], _| {
                    fill(out, channels, &source, |x| ((x + 1.0) * 32767.5) as u16)
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
        })
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
    convert: impl Fn(f32) -> T,
) {
    let mut queue = queue.lock().unwrap();
    for frame in out.chunks_mut(channels) {
        let sample = queue.pop_front().unwrap_or([0.0; 2]);
        for (ch, value) in frame.iter_mut().enumerate() {
            *value = convert(if channels == 1 {
                (sample[0] + sample[1]) * 0.5
            } else if ch < 2 {
                sample[ch]
            } else {
                0.0
            });
        }
    }
}
