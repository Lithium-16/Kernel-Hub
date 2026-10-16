//! Pet sprites from assets/, with the bows recoloured per person.

use anyhow::Context;
use image::{Rgba as Px, RgbaImage};

pub type Rgba = RgbaImage;

const WALK: &[u8] = include_bytes!("../assets/walk.png");
const IDLE: &[u8] = include_bytes!("../assets/idle.png");
const SIT: &[u8] = include_bytes!("../assets/sit.png");
const SLEEP: &[u8] = include_bytes!("../assets/sleep.png");
const CARRY: &[u8] = include_bytes!("../assets/carry.png");
const PARCEL: &[u8] = include_bytes!("../assets/parcel.png");
/// walk.json: 64x64 cells, row 1 is the 8-frame walk facing east.
const CELL: u32 = 64;
const WALK_ROW: u32 = 1;
const POSE_W: u32 = 32;
const POSE_H: u32 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    Idle,
    Sit,
    Sleep,
    Carry,
}

pub struct Sprites {
    pub walk: Vec<Rgba>,
    pub idle: Rgba,
    pub sit: Rgba,
    pub sleep: Rgba,
    pub carry: Rgba,
    pub parcel: Rgba,
}

fn decode(bytes: &[u8], name: &str) -> anyhow::Result<Rgba> {
    Ok(image::load_from_memory(bytes)
        .with_context(|| format!("decoding {name}"))?
        .to_rgba8())
}

fn hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

fn rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let f = |u: f32| ((u + m) * 255.0).round() as u8;
    (f(r), f(g), f(b))
}

/// The bows and chest ribbon: saturated reds (checked against walk.png: skin, eyes and the
/// inner ears stay below 0.6 saturation).
pub fn is_bow(r: u8, g: u8, b: u8) -> bool {
    let (h, s, _) = hsv(r, g, b);
    s >= 0.6 && !(15.0..340.0).contains(&h)
}

pub fn recolor_bows(img: &mut Rgba, hue: u16) -> usize {
    if hue.is_multiple_of(360) {
        return 0;
    }
    let mut n = 0;
    for p in img.pixels_mut() {
        if p[3] == 0 || !is_bow(p[0], p[1], p[2]) {
            continue;
        }
        let (_, s, v) = hsv(p[0], p[1], p[2]);
        let (r, g, b) = rgb(f32::from(hue % 360), s, v);
        *p = Px([r, g, b, p[3]]);
        n += 1;
    }
    n
}

pub fn mirror(img: &Rgba) -> Rgba {
    image::imageops::flip_horizontal(img)
}

/// Puts a 64x64 walk cell on the 32x48 canvas the pose images use, feet on the bottom row.
pub fn trim_to(cell: &Rgba, w: u32, h: u32) -> Rgba {
    let bottom = (0..cell.height())
        .rev()
        .find(|&y| (0..cell.width()).any(|x| cell.get_pixel(x, y)[3] > 0))
        .unwrap_or(cell.height() - 1);
    let mut out = Rgba::new(w, h);
    let x0 = (cell.width() - w) / 2;
    let y0 = (bottom + 1).saturating_sub(h);
    image::imageops::overlay(
        &mut out,
        &image::imageops::crop_imm(cell, x0, y0, w, h).to_image(),
        0,
        0,
    );
    out
}

impl Sprites {
    pub fn load(bow_hue: u16) -> anyhow::Result<Sprites> {
        let sheet = decode(WALK, "walk.png")?;
        let mut s = Sprites {
            walk: (0..8)
                .map(|i| {
                    trim_to(
                        &image::imageops::crop_imm(&sheet, i * CELL, WALK_ROW * CELL, CELL, CELL)
                            .to_image(),
                        POSE_W,
                        POSE_H,
                    )
                })
                .collect(),
            idle: decode(IDLE, "idle.png")?,
            sit: decode(SIT, "sit.png")?,
            sleep: decode(SLEEP, "sleep.png")?,
            carry: decode(CARRY, "carry.png")?,
            parcel: decode(PARCEL, "parcel.png")?,
        };
        for img in s
            .walk
            .iter_mut()
            .chain([&mut s.idle, &mut s.sit, &mut s.sleep, &mut s.carry])
        {
            recolor_bows(img, bow_hue);
        }
        Ok(s)
    }

    pub fn pose(&self, p: Pose) -> &Rgba {
        match p {
            Pose::Idle => &self.idle,
            Pose::Sit => &self.sit,
            Pose::Sleep => &self.sleep,
            Pose::Carry => &self.carry,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bow_red_is_recoloured() {
        assert!(is_bow(198, 45, 45) && is_bow(123, 25, 33) && is_bow(200, 43, 43));
        assert!(!is_bow(241, 151, 150)); // inner ear pink
        assert!(!is_bow(250, 220, 204)); // skin
        assert!(!is_bow(87, 85, 104)); // hair
        let s = Sprites::load(0).unwrap();
        let mut idle = s.idle.clone();
        let changed = recolor_bows(&mut idle, 210);
        assert!(changed > 10, "the bows should change");
        let red_left = idle.pixels().filter(|p| is_bow(p[0], p[1], p[2])).count();
        assert_eq!(red_left, 0);
        let mut same = s.idle.clone();
        assert_eq!(
            recolor_bows(&mut same, 0),
            0,
            "hue 0 keeps the art as drawn"
        );
    }

    #[test]
    fn sheet_is_sliced_into_eight_walk_frames() {
        let s = Sprites::load(0).unwrap();
        assert_eq!(s.walk.len(), 8);
        assert!(s.walk.iter().all(|f| f.dimensions() == (32, 48)));
        assert_eq!(s.parcel.dimensions(), (15, 12));
        assert_eq!(mirror(&s.idle).get_pixel(0, 20), s.idle.get_pixel(31, 20));
    }
}
