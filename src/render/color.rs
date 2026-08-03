use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb8 {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

impl fmt::Display for Rgb8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Knot {
    at: f64,
    color: Rgb8,
}

impl Knot {
    pub const fn new(at: f64, color: Rgb8) -> Self {
        Self { at, color }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Gradient<const N: usize> {
    root: Knot,
    successors: [Knot; N],
}

impl<const N: usize> Gradient<N> {
    pub const fn new(root: Knot, successors: [Knot; N]) -> Self {
        let mut previous = root.at;
        let mut index = 0;
        while index < N {
            assert!(successors[index].at > previous);
            previous = successors[index].at;
            index += 1;
        }
        Self { root, successors }
    }

    pub fn sample(self, value: f64) -> Rgb8 {
        let mut lower = self.root;
        if value <= lower.at {
            return lower.color;
        }
        for upper in self.successors {
            if value <= upper.at {
                return interpolate(
                    lower.color,
                    upper.color,
                    (value - lower.at) / (upper.at - lower.at),
                );
            }
            lower = upper;
        }
        lower.color
    }
}

fn interpolate(from: Rgb8, to: Rgb8, position: f64) -> Rgb8 {
    let channel =
        |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * position).round() as u8;
    Rgb8::new(
        channel(from.r, to.r),
        channel(from.g, to.g),
        channel(from.b, to.b),
    )
}

// base16 tomorrow
pub const DARK_GREY: Rgb8 = Rgb8::new(0x37, 0x3B, 0x41);
pub const GREY: Rgb8 = Rgb8::new(0x96, 0x98, 0x96);
pub const RED: Rgb8 = Rgb8::new(0xCC, 0x66, 0x66);
pub const ORANGE: Rgb8 = Rgb8::new(0xDE, 0x93, 0x5F);
pub const YELLOW: Rgb8 = Rgb8::new(0xF0, 0xC6, 0x74);
pub const GREEN: Rgb8 = Rgb8::new(0xB5, 0xBD, 0x68);
pub const CYAN: Rgb8 = Rgb8::new(0x8A, 0xBE, 0xB7);
pub const BLUE: Rgb8 = Rgb8::new(0x81, 0xA2, 0xBE);
pub const VIOLET: Rgb8 = Rgb8::new(0xB2, 0x94, 0xBB);
pub const BROWN: Rgb8 = Rgb8::new(0xA3, 0x68, 0x5A);
