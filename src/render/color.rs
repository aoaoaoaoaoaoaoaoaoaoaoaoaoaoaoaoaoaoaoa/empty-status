use palette::{Clamp, FromColor, Oklab, Srgb};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Srgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<&str> for Srgb8 {
    fn from(value: &str) -> Self {
        let value = value.strip_prefix('#').unwrap_or(value);
        if value.len() != 6 {
            return Self::new(0, 0, 0);
        }
        let Ok(r) = u8::from_str_radix(&value[0..2], 16) else {
            return Self::new(0, 0, 0);
        };
        let Ok(g) = u8::from_str_radix(&value[2..4], 16) else {
            return Self::new(0, 0, 0);
        };
        let Ok(b) = u8::from_str_radix(&value[4..6], 16) else {
            return Self::new(0, 0, 0);
        };
        Self::new(r, g, b)
    }
}

impl From<String> for Srgb8 {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl Srgb8 {
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

#[allow(dead_code)]
impl Srgb8 {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct Stop {
    pub t: f32,
    pub color: Oklab,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Gradient {
    stops: Vec<Stop>,
}

#[allow(dead_code)]
impl Gradient {
    pub fn new(mut stops: Vec<Stop>) -> Self {
        stops.sort_by(|a, b| a.t.total_cmp(&b.t));
        Self { stops }
    }

    pub fn at(&self, t: f32) -> Oklab {
        let t = t.clamp(0.0, 1.0);
        let [a, b] = self.bracket(t);
        let dt = (b.t - a.t).max(f32::EPSILON);
        let u = ((t - a.t) / dt).clamp(0.0, 1.0);
        lerp_oklab(a.color, b.color, u)
    }

    pub fn map_clamped(&self, x: f64, min: f64, max: f64) -> Srgb8 {
        let den = (max - min).max(f64::EPSILON);
        let t = ((x - min) / den).clamp(0.0, 1.0) as f32;
        oklab_to_srgb8(self.at(t))
    }

    fn bracket(&self, t: f32) -> [Stop; 2] {
        debug_assert!(!self.stops.is_empty());
        if self.stops.len() == 1 {
            return [self.stops[0], self.stops[0]];
        }

        let mut prev = self.stops[0];
        for s in &self.stops[1..] {
            if t <= s.t {
                return [prev, *s];
            }
            prev = *s;
        }
        let last = *self.stops.last().unwrap();
        [last, last]
    }
}

fn lerp_oklab(a: Oklab, b: Oklab, t: f32) -> Oklab {
    Oklab {
        l: a.l + (b.l - a.l) * t,
        a: a.a + (b.a - a.a) * t,
        b: a.b + (b.b - a.b) * t,
    }
}

fn oklab_to_srgb8(c: Oklab) -> Srgb8 {
    let rgb: Srgb = Srgb::from_color(c).clamp();
    let rgb8 = rgb.into_format::<u8>();
    Srgb8::new(rgb8.red, rgb8.green, rgb8.blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_clamps() {
        let g = Gradient::new(vec![
            Stop {
                t: 0.0,
                color: Oklab::from_color(Srgb::new(0.0, 0.0, 1.0)),
            },
            Stop {
                t: 1.0,
                color: Oklab::from_color(Srgb::new(1.0, 0.0, 0.0)),
            },
        ]);
        let _ = g.map_clamped(-100.0, 0.0, 1.0);
        let _ = g.map_clamped(100.0, 0.0, 1.0);
    }

    #[test]
    fn gradient_midpoint_different() {
        let g = Gradient::new(vec![
            Stop {
                t: 0.0,
                color: Oklab::from_color(Srgb::new(0.0, 0.0, 1.0)),
            },
            Stop {
                t: 1.0,
                color: Oklab::from_color(Srgb::new(1.0, 0.0, 0.0)),
            },
        ]);
        let a = g.map_clamped(0.0, 0.0, 1.0);
        let m = g.map_clamped(0.5, 0.0, 1.0);
        let b = g.map_clamped(1.0, 0.0, 1.0);
        assert_ne!(a, m);
        assert_ne!(b, m);
    }
}
