//! # WaveCore Media Engine
//!
//! Provides HTML5 `<audio>` and `<video>` media abstractions, playback state machines,
//! pure-Rust PCM audio buffer generation and WAV decoding/encoding, and video frame representations.

use wavecore_pixels::{Rgba, Surface};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyState {
    HaveNothing = 0,
    HaveMetadata = 1,
    HaveCurrentData = 2,
    HaveFutureData = 3,
    HaveEnoughData = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkState {
    Empty = 0,
    Idle = 1,
    Loading = 2,
    NoSource = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Audio,
    Video,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MediaError {
    InvalidWavHeader,
    UnsupportedFormat(String),
    BufferTooShort,
    DecodeError(String),
}

/// A multi-channel or mono raw PCM audio buffer in 32-bit floating point format.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    pub channels: usize,
    pub sample_rate: u32,
    pub samples: Vec<f32>, // interleaved samples: [ch0_s0, ch1_s0, ch0_s1, ...]
}

impl AudioBuffer {
    pub fn new(channels: usize, sample_rate: u32, length_frames: usize) -> Self {
        Self {
            channels: channels.max(1),
            sample_rate,
            samples: vec![0.0; length_frames * channels.max(1)],
        }
    }

    pub fn duration(&self) -> f64 {
        if self.sample_rate == 0 || self.channels == 0 {
            0.0
        } else {
            (self.samples.len() / self.channels) as f64 / self.sample_rate as f64
        }
    }

    /// Generates a pure sine wave audio buffer (useful for audio test tones and synthesizing sounds).
    pub fn generate_sine_wave(freq_hz: f32, duration_secs: f32, sample_rate: u32, volume: f32) -> Self {
        let sample_rate = sample_rate.max(8000);
        let num_frames = (duration_secs * sample_rate as f32) as usize;
        let mut samples = Vec::with_capacity(num_frames * 2);

        for i in 0..num_frames {
            let t = i as f32 / sample_rate as f32;
            let sample = (t * freq_hz * 2.0 * std::f32::consts::PI).sin() * volume.clamp(0.0, 1.0);
            samples.push(sample); // Left
            samples.push(sample); // Right (Stereo)
        }

        Self {
            channels: 2,
            sample_rate,
            samples,
        }
    }

    /// Decodes a standard 16-bit or 8-bit PCM RIFF/WAVE byte stream.
    pub fn decode_wav(bytes: &[u8]) -> Result<Self, MediaError> {
        if bytes.len() < 44 {
            return Err(MediaError::BufferTooShort);
        }

        // Check RIFF header
        if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(MediaError::InvalidWavHeader);
        }

        let mut offset = 12;
        let mut audio_format = 1u16; // 1 = PCM
        let mut channels = 1u16;
        let mut sample_rate = 44100u32;
        let mut bits_per_sample = 16u16;
        let mut audio_data: Option<&[u8]> = None;

        while offset + 8 <= bytes.len() {
            let chunk_id = &bytes[offset..offset + 4];
            let chunk_size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            offset += 8;

            if offset + chunk_size > bytes.len() {
                // Some WAV files have slightly misreported sizes; clamp to available
                let available = bytes.len() - offset;
                if chunk_id == b"data" {
                    audio_data = Some(&bytes[offset..offset + available]);
                }
                break;
            }

            if chunk_id == b"fmt " {
                if chunk_size >= 16 {
                    audio_format = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
                    channels = u16::from_le_bytes(bytes[offset + 2..offset + 4].try_into().unwrap());
                    sample_rate = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
                    bits_per_sample = u16::from_le_bytes(bytes[offset + 14..offset + 16].try_into().unwrap());
                }
            } else if chunk_id == b"data" {
                audio_data = Some(&bytes[offset..offset + chunk_size]);
            }

            offset += chunk_size;
            if chunk_size % 2 != 0 && offset < bytes.len() {
                offset += 1; // Pad byte
            }
        }

        if audio_format != 1 {
            return Err(MediaError::UnsupportedFormat(format!("WAV audio format {}", audio_format)));
        }

        let raw_pcm = audio_data.ok_or(MediaError::InvalidWavHeader)?;
        let mut samples = Vec::new();

        match bits_per_sample {
            16 => {
                for chunk in raw_pcm.chunks_exact(2) {
                    let s = i16::from_le_bytes([chunk[0], chunk[1]]);
                    samples.push(s as f32 / 32768.0);
                }
            }
            8 => {
                for &b in raw_pcm {
                    // 8-bit WAV is unsigned [0..255], center at 128
                    let s = (b as f32 - 128.0) / 128.0;
                    samples.push(s);
                }
            }
            other => {
                return Err(MediaError::UnsupportedFormat(format!("{} bits per sample", other)));
            }
        }

        Ok(Self {
            channels: channels.max(1) as usize,
            sample_rate,
            samples,
        })
    }

    /// Encodes the PCM buffer to standard 16-bit PCM RIFF/WAVE bytes.
    pub fn encode_wav(&self) -> Vec<u8> {
        let num_channels = self.channels.max(1) as u16;
        let bits_per_sample = 16u16;
        let block_align = num_channels * (bits_per_sample / 8);
        let byte_rate = self.sample_rate * block_align as u32;
        let data_len = (self.samples.len() * 2) as u32;
        let file_len = 36 + data_len;

        let mut out = Vec::with_capacity(44 + data_len as usize);
        // RIFF header
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&file_len.to_le_bytes());
        out.extend_from_slice(b"WAVE");

        // "fmt " chunk
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // subchunk1size = 16 for PCM
        out.extend_from_slice(&1u16.to_le_bytes());  // PCM format = 1
        out.extend_from_slice(&num_channels.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&bits_per_sample.to_le_bytes());

        // "data" chunk
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for &sample in &self.samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let s = (clamped * 32767.0) as i16;
            out.extend_from_slice(&s.to_le_bytes());
        }

        out
    }
}

/// Represents a single video frame with RGBA pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrame {
    pub timestamp: f64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>, // 0xAARRGGBB format
}

impl VideoFrame {
    pub fn new(timestamp: f64, width: u32, height: u32) -> Self {
        Self {
            timestamp,
            width,
            height,
            pixels: vec![0xFF000000; (width * height) as usize],
        }
    }

    /// Generates a test pattern video frame (e.g. animated color bars or gradient)
    pub fn generate_test_pattern(timestamp: f64, width: u32, height: u32) -> Self {
        let mut pixels = Vec::with_capacity((width * height) as usize);
        let shift = (timestamp * 60.0) as u32;

        for y in 0..height {
            for x in 0..width {
                let r = ((x + shift) % 256) as u8;
                let g = ((y + shift) % 256) as u8;
                let b = ((x + y) % 256) as u8;
                let color = 0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                pixels.push(color);
            }
        }

        Self {
            timestamp,
            width,
            height,
            pixels,
        }
    }
}

/// High-level HTML5 Media Element instance (`<audio>` or `<video>`).
#[derive(Debug, Clone)]
pub struct MediaElement {
    pub media_type: MediaType,
    pub src: String,
    pub current_time: f64,
    pub duration: f64,
    pub paused: bool,
    pub playback_rate: f64,
    pub volume: f64,
    pub muted: bool,
    pub looping: bool,
    pub ended: bool,
    pub controls: bool,
    pub autoplay: bool,
    pub ready_state: ReadyState,
    pub network_state: NetworkState,
    pub width: u32,
    pub height: u32,
    pub audio_buffer: Option<AudioBuffer>,
    pub current_video_frame: Option<VideoFrame>,
}

impl MediaElement {
    pub fn new_audio(src: &str) -> Self {
        Self {
            media_type: MediaType::Audio,
            src: src.to_string(),
            current_time: 0.0,
            duration: 0.0,
            paused: true,
            playback_rate: 1.0,
            volume: 1.0,
            muted: false,
            looping: false,
            ended: false,
            controls: true,
            autoplay: false,
            ready_state: ReadyState::HaveNothing,
            network_state: NetworkState::Idle,
            width: 300,
            height: 48,
            audio_buffer: None,
            current_video_frame: None,
        }
    }

    pub fn new_video(src: &str, width: u32, height: u32) -> Self {
        Self {
            media_type: MediaType::Video,
            src: src.to_string(),
            current_time: 0.0,
            duration: 0.0,
            paused: true,
            playback_rate: 1.0,
            volume: 1.0,
            muted: false,
            looping: false,
            ended: false,
            controls: true,
            autoplay: false,
            ready_state: ReadyState::HaveNothing,
            network_state: NetworkState::Idle,
            width: if width > 0 { width } else { 480 },
            height: if height > 0 { height } else { 270 },
            audio_buffer: None,
            current_video_frame: None,
        }
    }

    pub fn play(&mut self) {
        if self.ended {
            self.current_time = 0.0;
            self.ended = false;
        }
        self.paused = false;
        if self.ready_state == ReadyState::HaveNothing {
            self.ready_state = ReadyState::HaveEnoughData;
        }
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn load_wav_bytes(&mut self, bytes: &[u8]) -> Result<(), MediaError> {
        if self.media_type != MediaType::Audio {
            return Err(MediaError::UnsupportedFormat(
                "WAV audio cannot be loaded into a video element".to_string(),
            ));
        }
        self.network_state = NetworkState::Loading;
        match AudioBuffer::decode_wav(bytes) {
            Ok(buffer) => {
                self.duration = buffer.duration();
                self.current_time = 0.0;
                self.ended = false;
                self.audio_buffer = Some(buffer);
                self.ready_state = ReadyState::HaveEnoughData;
                self.network_state = NetworkState::Idle;
                Ok(())
            }
            Err(error) => {
                self.ready_state = ReadyState::HaveNothing;
                self.network_state = NetworkState::NoSource;
                Err(error)
            }
        }
    }

    pub fn buffered_audio_frames(&self) -> usize {
        self.audio_buffer
            .as_ref()
            .map(|buffer| buffer.samples.len() / buffer.channels.max(1))
            .unwrap_or(0)
    }

    pub fn seek(&mut self, time_secs: f64) {
        let max_time = if self.duration > 0.0 { self.duration } else { 0.0 };
        self.current_time = time_secs.clamp(0.0, max_time);
        self.ended = self.duration > 0.0 && self.current_time >= self.duration;
    }

    /// Progresses the playback timeline by `delta_secs`.
    pub fn step(&mut self, delta_secs: f64) {
        if self.paused || self.ended {
            return;
        }

        self.current_time += delta_secs * self.playback_rate;

        if self.duration > 0.0 && self.current_time >= self.duration {
            if self.looping {
                self.current_time = 0.0;
            } else {
                self.current_time = self.duration;
                self.ended = true;
                self.paused = true;
            }
        }

        // Update video test frame if active video
        if self.media_type == MediaType::Video {
            self.current_video_frame = Some(VideoFrame::generate_test_pattern(
                self.current_time,
                self.width,
                self.height,
            ));
        }
    }

    /// Renders a media control bar into a Surface buffer.
    pub fn render_controls(&self, width: u32, height: u32) -> Surface {
        let mut surface = Surface::new(width, height);
        // Background: dark slate glass
        surface.fill_rect(0.0, 0.0, width as f32, height as f32, Rgba(0x1E, 0x29, 0x3B, 0xEE));

        // Play/Pause icon indicator box
        let icon_box_w = 32.0;
        let icon_box_h = 24.0;
        let icon_y = (height as f32 - icon_box_h) * 0.5;
        surface.fill_rect(10.0, icon_y, icon_box_w, icon_box_h, Rgba(0x3B, 0x82, 0xF6, 0xFF));

        // Progress bar background
        let progress_x = 52.0;
        let progress_w = (width as f32 - 120.0).max(40.0);
        let progress_h = 6.0;
        let progress_y = (height as f32 - progress_h) * 0.5;
        surface.fill_rect(progress_x, progress_y, progress_w, progress_h, Rgba(0x47, 0x55, 0x69, 0xFF));

        // Progress fill
        let progress_pct = if self.duration > 0.0 {
            (self.current_time / self.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        surface.fill_rect(progress_x, progress_y, progress_w * progress_pct, progress_h, Rgba(0x38, 0xBD, 0xF8, 0xFF));

        // Volume / status indicator block
        let vol_x = progress_x + progress_w + 12.0;
        let vol_color = if self.muted { Rgba(0xEF, 0x44, 0x44, 0xFF) } else { Rgba(0x10, 0xB9, 0x81, 0xFF) };
        surface.fill_rect(vol_x, progress_y, 40.0, progress_h, vol_color);

        surface
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sine_wave_and_wav_roundtrip() {
        let buffer = AudioBuffer::generate_sine_wave(440.0, 0.25, 44100, 0.8);
        assert_eq!(buffer.channels, 2);
        assert_eq!(buffer.sample_rate, 44100);
        assert!(buffer.duration() >= 0.24 && buffer.duration() <= 0.26);

        let wav_bytes = buffer.encode_wav();
        assert!(wav_bytes.len() > 44);

        let decoded = AudioBuffer::decode_wav(&wav_bytes).expect("WAV decode failed");
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.sample_rate, 44100);
        assert_eq!(decoded.samples.len(), buffer.samples.len());

        // Check sample fidelity
        for (a, b) in buffer.samples.iter().zip(decoded.samples.iter()) {
            assert!((a - b).abs() < 0.001);
        }
    }

    #[test]
    fn test_media_element_playback_flow() {
        let mut audio = MediaElement::new_audio("https://example.com/audio.mp3");
        audio.duration = 10.0;
        assert!(audio.paused);
        assert_eq!(audio.current_time, 0.0);

        audio.play();
        assert!(!audio.paused);
        assert_eq!(audio.ready_state, ReadyState::HaveEnoughData);

        audio.step(2.5);
        assert_eq!(audio.current_time, 2.5);

        audio.seek(9.0);
        assert_eq!(audio.current_time, 9.0);

        audio.step(1.5);
        assert_eq!(audio.current_time, 10.0);
        assert!(audio.ended);
        assert!(audio.paused);
    }

    #[test]
    fn test_video_element_and_test_pattern() {
        let mut video = MediaElement::new_video("test.mp4", 320, 240);
        video.duration = 5.0;
        video.play();
        video.step(1.0);
        assert!(video.current_video_frame.is_some());
        let frame = video.current_video_frame.clone().unwrap();
        assert_eq!(frame.width, 320);
        assert_eq!(frame.height, 240);
        assert_eq!(frame.pixels.len(), (320 * 240) as usize);

        let controls = video.render_controls(320, 40);
        assert_eq!(controls.width, 320);
        assert_eq!(controls.height, 40);
    }
    #[test]
    fn audio_element_loads_real_wav_buffer_and_metadata() {
        let source = AudioBuffer::generate_sine_wave(220.0, 0.1, 48_000, 0.5);
        let wav = source.encode_wav();
        let mut audio = MediaElement::new_audio("tone.wav");

        audio.load_wav_bytes(&wav).unwrap();

        assert_eq!(audio.ready_state, ReadyState::HaveEnoughData);
        assert_eq!(audio.network_state, NetworkState::Idle);
        assert_eq!(audio.buffered_audio_frames(), 4_800);
        assert!((audio.duration - 0.1).abs() < 0.001);
    }

    #[test]
    fn invalid_audio_load_moves_element_to_no_source_state() {
        let mut audio = MediaElement::new_audio("broken.wav");
        assert!(audio.load_wav_bytes(b"not-a-wave").is_err());
        assert_eq!(audio.ready_state, ReadyState::HaveNothing);
        assert_eq!(audio.network_state, NetworkState::NoSource);
    }

}
