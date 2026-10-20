//! Colors: parsing what people type (#hex, rgb(), hsl(), oklch()), every
//! format a designer copies, and tint/shade ramps.

use serde::{Deserialize, Serialize};

use crate::settings::ColorFormat;

/// sRGB, 0..=1 per channel.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    #[serde(default = "one")]
    pub a: f32,
}

fn one() -> f32 {
    1.0
}

impl Rgb {
    pub fn new(r: f32, g: f32, b: f32) -> Rgb {
        Rgb { r, g, b, a: 1.0 }
    }

    pub fn from_u8(r: u8, g: u8, b: u8) -> Rgb {
        Rgb::new(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0)
    }

    pub fn u8(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    }

    pub fn hex(self) -> String {
        let [r, g, b, a] = self.u8();
        if a == 255 { format!("#{r:02X}{g:02X}{b:02X}") } else { format!("#{r:02X}{g:02X}{b:02X}{a:02X}") }
    }

    pub fn format(self, f: ColorFormat) -> String {
        let [r, g, b, _] = self.u8();
        match f {
            ColorFormat::Hex => self.hex(),
            ColorFormat::Rgb if self.a < 1.0 => format!("rgba({r}, {g}, {b}, {})", trim(self.a, 2)),
            ColorFormat::Rgb => format!("rgb({r}, {g}, {b})"),
            ColorFormat::Hsl => {
                let (h, s, l) = self.hsl();
                format!("hsl({}, {}%, {}%)", h.round(), (s * 100.0).round(), (l * 100.0).round())
            }
            ColorFormat::Oklch => {
                let (l, c, h) = self.oklch();
                format!("oklch({}% {} {})", trim(l * 100.0, 1), trim(c, 3), trim(h, 1))
            }
            ColorFormat::Swift => format!("Color(red: {}, green: {}, blue: {})", trim(self.r, 3), trim(self.g, 3), trim(self.b, 3)),
        }
    }

    /// Relative luminance, for picking legible text on a swatch.
    pub fn luminance(self) -> f32 {
        let [r, g, b] = [self.r, self.g, self.b].map(to_linear);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    pub fn hsl(self) -> (f32, f32, f32) {
        let (r, g, b) = (self.r, self.g, self.b);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d < 1e-6 {
            return (0.0, 0.0, l);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (h, s, l)
    }

    pub fn from_hsl(h: f32, s: f32, l: f32) -> Rgb {
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let hp = h.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
        let (r, g, b) = match hp as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = l - c / 2.0;
        Rgb::new(r + m, g + m, b + m)
    }

    /// OKLCH: lightness 0..1, chroma, hue degrees.
    pub fn oklch(self) -> (f32, f32, f32) {
        let [r, g, b] = [self.r, self.g, self.b].map(to_linear);
        let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
        let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
        let ll = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
        let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
        let bb = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
        let c = a.hypot(bb);
        let h = if c < 1e-4 { 0.0 } else { bb.atan2(a).to_degrees().rem_euclid(360.0) };
        (ll, c, h)
    }

    pub fn from_oklch(l: f32, c: f32, h: f32) -> Rgb {
        let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
        let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
        let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
        let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
        let (l3, m3, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
        let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
        let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_4 * s3;
        let bl = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
        Rgb::new(from_linear(r).clamp(0.0, 1.0), from_linear(g).clamp(0.0, 1.0), from_linear(bl).clamp(0.0, 1.0))
    }

    /// Eleven steps from near-white to near-black at this hue and chroma,
    /// like a Tailwind ramp (50, 100, ... 950).
    pub fn ramp(self) -> Vec<(u16, Rgb)> {
        let (_, c, h) = self.oklch();
        const STEPS: [(u16, f32); 11] = [
            (50, 0.97),
            (100, 0.93),
            (200, 0.87),
            (300, 0.79),
            (400, 0.70),
            (500, 0.62),
            (600, 0.54),
            (700, 0.46),
            (800, 0.38),
            (900, 0.30),
            (950, 0.22),
        ];
        STEPS
            .iter()
            .map(|&(n, l)| {
                // Pale and very dark ends can't hold full chroma in sRGB.
                let fade = 1.0 - ((l - 0.6).abs() / 0.45).powi(2);
                (n, Rgb::from_oklch(l, c * fade.clamp(0.2, 1.0), h))
            })
            .collect()
    }
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f32) -> f32 {
    if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055 }
}

fn trim(v: f32, places: usize) -> String {
    let s = format!("{v:.places$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// What someone typed, if it's unambiguously a color.
pub fn parse(s: &str) -> Option<Rgb> {
    let s = s.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    let (name, args) = s.strip_suffix(')')?.split_once('(')?;
    let parts: Vec<&str> = args.split([',', ' ', '/']).map(str::trim).filter(|p| !p.is_empty()).collect();
    let num = |p: &str, scale: f32| -> Option<f32> {
        if let Some(pct) = p.strip_suffix('%') { pct.parse::<f32>().ok().map(|v| v / 100.0) } else { p.trim_end_matches("deg").parse::<f32>().ok().map(|v| v / scale) }
    };
    let alpha = |i: usize| parts.get(i).map_or(Some(1.0), |p| num(p, 1.0));
    match name.trim() {
        "rgb" | "rgba" if parts.len() >= 3 => {
            let mut c = Rgb::new(num(parts[0], 255.0)?, num(parts[1], 255.0)?, num(parts[2], 255.0)?);
            c.a = alpha(3)?;
            Some(c)
        }
        "hsl" | "hsla" if parts.len() >= 3 => {
            let h: f32 = parts[0].trim_end_matches("deg").parse().ok()?;
            let mut c = Rgb::from_hsl(h, num(parts[1], 100.0)?, num(parts[2], 100.0)?);
            c.a = alpha(3)?;
            Some(c)
        }
        "oklch" if parts.len() >= 3 => {
            let l = num(parts[0], 1.0)?;
            let l = if l > 1.0 { l / 100.0 } else { l };
            let mut c = Rgb::from_oklch(l, num(parts[1], 1.0)?, parts[2].trim_end_matches("deg").parse().ok()?);
            c.a = alpha(3)?;
            Some(c)
        }
        _ => None,
    }
}

fn parse_hex(h: &str) -> Option<Rgb> {
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = |s: &str| u8::from_str_radix(s, 16).ok();
    let d = |c: char| v(&format!("{c}{c}"));
    let ch: Vec<char> = h.chars().collect();
    let (r, g, b, a) = match ch.len() {
        3 => (d(ch[0])?, d(ch[1])?, d(ch[2])?, 255),
        4 => (d(ch[0])?, d(ch[1])?, d(ch[2])?, d(ch[3])?),
        6 => (v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?, 255),
        8 => (v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?, v(&h[6..8])?),
        _ => return None,
    };
    let mut c = Rgb::from_u8(r, g, b);
    c.a = f32::from(a) / 255.0;
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_designers_type() {
        assert_eq!(parse("#fff").unwrap().hex(), "#FFFFFF");
        assert_eq!(parse("#A6C8F4").unwrap().hex(), "#A6C8F4");
        assert_eq!(parse("#a6c8f480").unwrap().u8()[3], 128);
        assert_eq!(parse("rgb(166, 200, 244)").unwrap().hex(), "#A6C8F4");
        assert_eq!(parse("rgb(166 200 244 / 50%)").unwrap().u8()[3], 128);
        assert_eq!(parse("hsl(0, 100%, 50%)").unwrap().hex(), "#FF0000");
        assert_eq!(parse("hsl(120deg 100% 25%)").unwrap().hex(), "#008000");
        assert!(parse("#ggg").is_none());
        assert!(parse("facade").is_none(), "bare words are not colors");
        assert!(parse("rgb(1,2)").is_none());
    }

    #[test]
    fn formats_round_trip() {
        let c = parse("#3B82F6").unwrap();
        assert_eq!(c.format(ColorFormat::Rgb), "rgb(59, 130, 246)");
        assert_eq!(c.format(ColorFormat::Hsl), "hsl(217, 91%, 60%)");
        let ok = c.format(ColorFormat::Oklch);
        assert!(ok.starts_with("oklch(62.3% 0.188 259"), "{ok}");
        assert_eq!(parse(&ok).unwrap().hex(), "#3B82F6");
        assert!(parse(&c.format(ColorFormat::Hsl)).is_some());
        assert_eq!(Rgb::new(1.0, 1.0, 1.0).format(ColorFormat::Swift), "Color(red: 1, green: 1, blue: 1)");
    }

    #[test]
    fn ramps_go_light_to_dark() {
        let ramp = parse("#3B82F6").unwrap().ramp();
        assert_eq!(ramp.len(), 11);
        assert!(ramp.windows(2).all(|w| w[0].1.luminance() > w[1].1.luminance()));
    }
}
