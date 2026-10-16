//! Little sounds, synthesised in memory (no sound files, no audio library).

use std::f32::consts::TAU;

const RATE: u32 = 22_050;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sound {
    /// A letter arrived.
    Chime,
    /// A pet came in or went out through the door.
    Door,
    /// A pat.
    Mew,
    /// A poke.
    Boing,
    /// The peek snapshot.
    Click,
    /// Someone won rock paper scissors.
    TaDa,
    /// The fetch ball was thrown (or a yeet).
    Whoosh,
    /// Found in hide-and-seek.
    Giggle,
    /// Bouncing off a wall or the floor.
    Bounce,
    /// A long meow.
    Meow,
    /// A purr.
    Purr,
    /// A quick trilled chirp.
    Mrrp,
}

/// A sine glide from `f0` to `f1` Hz over `secs`, with a quick attack and a fading tail.
fn glide(f0: f32, f1: f32, secs: f32, vibrato: f32) -> Vec<f32> {
    let n = (secs * RATE as f32) as usize;
    let mut phase = 0.0_f32;
    (0..n)
        .map(|i| {
            let s = i as f32 / n as f32;
            let f = f0 + (f1 - f0) * s + vibrato * (s * 40.0).sin();
            phase += TAU * f / RATE as f32;
            let env = (i as f32 / (0.004 * RATE as f32)).min(1.0) * (1.0 - s).powi(2);
            phase.sin() * env
        })
        .collect()
}

/// A burst of noise that fades out quickly (a click) or swells (a whoosh).
fn noise(secs: f32, swell: bool) -> Vec<f32> {
    let n = (secs * RATE as f32) as usize;
    let mut x: u32 = 0x1234_5678;
    (0..n)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let s = i as f32 / n as f32;
            let env = if swell {
                (s * std::f32::consts::PI).sin()
            } else {
                (1.0 - s).powi(3)
            };
            ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * env
        })
        .collect()
}

/// A low rumble that pulses about 25 times a second.
fn purr(secs: f32) -> Vec<f32> {
    let n = (secs * RATE as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let s = i as f32 / n as f32;
            let tone = (TAU * 95.0 * t).sin() + 0.5 * (TAU * 190.0 * t).sin();
            let pulse = 0.55 + 0.45 * (TAU * 25.0 * t).sin();
            tone * pulse * (s * std::f32::consts::PI).sin() * 0.7
        })
        .collect()
}

/// A cat's voice: a buzzy source pushed through two moving vowel resonances (formants).
/// `shape(s)` gives (pitch, first formant, second formant) in Hz for s from 0 to 1;
/// `trill` is a pulse rate in Hz (0 for none).
fn voice(secs: f32, trill: f32, shape: impl Fn(f32) -> (f32, f32, f32)) -> Vec<f32> {
    let n = (secs * RATE as f32) as usize;
    let mut phase = 0.0_f32;
    let mut state = [[0.0_f32; 4]; 2]; // x1, x2, y1, y2 for each resonator
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            let s = i as f32 / n as f32;
            let t = i as f32 / RATE as f32;
            let (mut f0, f1, f2) = shape(s);
            f0 += 10.0 * (TAU * 5.5 * t).sin(); // a little shake in the pitch
            phase = (phase + f0 / RATE as f32).fract();
            let source = 2.0 * phase - 1.0 + 0.04 * ((i * 7919) % 101) as f32 / 101.0; // saw + breath
            let mut sum = 0.0;
            for (k, (f, gain)) in [(f1, 1.0), (f2, 0.6)].into_iter().enumerate() {
                let w0 = TAU * f / RATE as f32;
                let alpha = w0.sin() / (2.0 * 7.0);
                let a0 = 1.0 + alpha;
                let [x1, x2, y1, y2] = state[k];
                let y =
                    (alpha * source - alpha * x2 + 2.0 * w0.cos() * y1 - (1.0 - alpha) * y2) / a0;
                state[k] = [source, x1, y, y1];
                sum += gain * y;
            }
            let attack = (t / 0.015).min(1.0);
            let release = (1.0 - s).powf(0.6);
            let pulse = if trill > 0.0 {
                0.55 + 0.45 * (TAU * trill * t).sin()
            } else {
                1.0
            };
            sum * attack * release * pulse
        })
        .collect();
    let peak = out.iter().fold(0.0_f32, |m, v| m.max(v.abs())).max(1e-6);
    out.iter_mut().for_each(|v| *v = *v / peak * 0.9);
    out
}

/// An impact: a low body that drops in pitch and dies fast, with a short knock on top.
fn thump() -> Vec<f32> {
    let n = (0.18 * RATE as f32) as usize;
    let mut phase = 0.0_f32;
    let mut x: u32 = 0x9E37_79B9;
    (0..n)
        .map(|i| {
            let s = i as f32 / n as f32;
            let f = 40.0 + 120.0 * (-s * 14.0).exp();
            phase += TAU * f / RATE as f32;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let knock = ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * (-s * 90.0).exp() * 0.5;
            (phase.sin() * (-s * 9.0).exp() * 0.9 + knock).clamp(-1.0, 1.0)
        })
        .collect()
}

fn samples(s: Sound) -> Vec<f32> {
    let cat = |parts: Vec<Vec<f32>>| parts.into_iter().flatten().collect::<Vec<f32>>();
    match s {
        Sound::Chime => cat(vec![
            glide(880.0, 880.0, 0.12, 0.0),
            glide(1320.0, 1320.0, 0.25, 0.0),
        ]),
        Sound::Door => glide(420.0, 760.0, 0.09, 0.0),
        Sound::Mew => voice(0.32, 0.0, |s| {
            // up then down, with the vowel moving from "e" to "ow"
            let up = (s * 3.0).min(1.0);
            let f0 = 560.0 + 340.0 * up - 280.0 * ((s - 0.33).max(0.0) / 0.67);
            (f0, 350.0 + 350.0 * up, 2300.0 - 1200.0 * up)
        }),
        Sound::Boing => glide(260.0, 520.0, 0.22, 25.0),
        Sound::Click => noise(0.04, false),
        Sound::TaDa => cat(vec![
            glide(660.0, 660.0, 0.09, 0.0),
            glide(880.0, 880.0, 0.09, 0.0),
            glide(1320.0, 1320.0, 0.3, 0.0),
        ]),
        Sound::Whoosh => noise(0.22, true),
        Sound::Bounce => thump(),
        Sound::Meow => voice(0.62, 0.0, |s| {
            let f0 = if s < 0.3 {
                480.0 + 340.0 * (s / 0.3)
            } else {
                820.0 - 440.0 * ((s - 0.3) / 0.7)
            };
            let open = (s * 2.2).min(1.0);
            (f0, 380.0 + 340.0 * open, 2200.0 - 1000.0 * open)
        }),
        Sound::Mrrp => voice(0.28, 28.0, |s| (380.0 + 150.0 * s, 520.0, 1500.0)),
        Sound::Purr => purr(0.9),
        Sound::Giggle => cat(vec![
            glide(900.0, 1100.0, 0.06, 0.0),
            glide(1100.0, 900.0, 0.06, 0.0),
            glide(950.0, 1150.0, 0.06, 0.0),
            glide(1150.0, 950.0, 0.1, 0.0),
        ]),
    }
}

/// A complete mono 16-bit WAV file of the sound; `volume` is 0.0 to 1.0.
pub fn wav(s: Sound, volume: f32) -> Vec<u8> {
    let pcm: Vec<i16> = samples(s)
        .into_iter()
        .map(|v| (v * volume.clamp(0.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for v in pcm {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Sound; 12] = [
        Sound::Mrrp,
        Sound::Bounce,
        Sound::Meow,
        Sound::Purr,
        Sound::Chime,
        Sound::Door,
        Sound::Mew,
        Sound::Boing,
        Sound::Click,
        Sound::TaDa,
        Sound::Whoosh,
        Sound::Giggle,
    ];

    fn peak(w: &[u8]) -> i16 {
        w[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]).saturating_abs())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn every_sound_is_a_valid_short_wav_that_is_not_silent() {
        for s in ALL {
            let w = wav(s, 0.5);
            assert_eq!(&w[0..4], b"RIFF", "{s:?}");
            assert_eq!(&w[8..16], b"WAVEfmt ", "{s:?}");
            let data_len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
            assert_eq!(w.len(), 44 + data_len, "{s:?}");
            let riff_len = u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize;
            assert_eq!(riff_len, w.len() - 8, "{s:?}");
            assert!(
                data_len / 2 < RATE as usize,
                "{s:?} is longer than a second"
            );
            assert!(peak(&w) > 1000, "{s:?} is silent");
        }
    }

    #[test]
    fn bounce_is_a_thump_that_dies_away() {
        let w = wav(Sound::Bounce, 1.0);
        let pcm: Vec<i16> = w[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let n = (RATE as usize * 30) / 1000;
        let peak = |s: &[i16]| s.iter().map(|v| v.saturating_abs()).max().unwrap_or(0);
        assert!(peak(&pcm[..n]) > 4 * peak(&pcm[pcm.len() - n..]));
        // mostly low: few zero crossings per second compared with a 1 kHz tone
        let crossings = pcm.windows(2).filter(|p| (p[0] < 0) != (p[1] < 0)).count();
        let secs = pcm.len() as f32 / RATE as f32;
        assert!(
            (crossings as f32 / secs) < 1500.0,
            "{} crossings/s",
            crossings as f32 / secs
        );
    }

    #[test]
    fn cat_voices_have_overtones_not_a_pure_sine() {
        for s in [Sound::Mew, Sound::Meow, Sound::Mrrp] {
            let x = samples(s);
            // a pure sine's second difference is proportional to the signal itself; a voice with
            // formants is not, so the ratio varies a lot
            let ratios: Vec<f32> = x
                .windows(3)
                .filter(|w| w[1].abs() > 0.2)
                .map(|w| (w[0] - 2.0 * w[1] + w[2]) / w[1])
                .collect();
            let (lo, hi) = ratios
                .iter()
                .fold((f32::MAX, f32::MIN), |(l, h), r| (l.min(*r), h.max(*r)));
            assert!(hi - lo > 0.5, "{s:?} is too pure ({lo}..{hi})");
            assert!(
                x.iter().all(|v| v.is_finite() && v.abs() <= 1.01),
                "{s:?} blew up"
            );
        }
    }

    #[test]
    fn volume_scales_the_loudness_and_zero_is_silent() {
        let loud = peak(&wav(Sound::Chime, 1.0));
        let quiet = peak(&wav(Sound::Chime, 0.25));
        assert!(quiet < loud / 2, "{quiet} vs {loud}");
        assert_eq!(peak(&wav(Sound::Chime, 0.0)), 0);
    }
}
