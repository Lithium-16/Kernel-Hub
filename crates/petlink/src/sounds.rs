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

fn samples(s: Sound) -> Vec<f32> {
    let cat = |parts: Vec<Vec<f32>>| parts.into_iter().flatten().collect::<Vec<f32>>();
    match s {
        Sound::Chime => cat(vec![
            glide(880.0, 880.0, 0.12, 0.0),
            glide(1320.0, 1320.0, 0.25, 0.0),
        ]),
        Sound::Door => glide(420.0, 760.0, 0.09, 0.0),
        Sound::Mew => cat(vec![
            glide(620.0, 980.0, 0.12, 12.0),
            glide(980.0, 700.0, 0.16, 12.0),
        ]),
        Sound::Boing => glide(260.0, 520.0, 0.22, 25.0),
        Sound::Click => noise(0.04, false),
        Sound::TaDa => cat(vec![
            glide(660.0, 660.0, 0.09, 0.0),
            glide(880.0, 880.0, 0.09, 0.0),
            glide(1320.0, 1320.0, 0.3, 0.0),
        ]),
        Sound::Whoosh => noise(0.22, true),
        Sound::Bounce => cat(vec![
            glide(340.0, 110.0, 0.14, 0.0),
            glide(180.0, 300.0, 0.08, 20.0),
        ]),
        Sound::Meow => cat(vec![
            glide(520.0, 860.0, 0.14, 10.0),
            glide(860.0, 700.0, 0.16, 10.0),
            glide(700.0, 380.0, 0.22, 8.0),
        ]),
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

    const ALL: [Sound; 11] = [
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
    fn volume_scales_the_loudness_and_zero_is_silent() {
        let loud = peak(&wav(Sound::Chime, 1.0));
        let quiet = peak(&wav(Sound::Chime, 0.25));
        assert!(quiet < loud / 2, "{quiet} vs {loud}");
        assert_eq!(peak(&wav(Sound::Chime, 0.0)), 0);
    }
}
