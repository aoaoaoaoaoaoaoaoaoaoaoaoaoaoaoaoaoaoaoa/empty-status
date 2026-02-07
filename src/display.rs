use crate::core::{CYAN, GREEN, ORANGE, RED, YELLOW};
use crate::render::color::Srgb8;

pub const COL_USE_COOL: &str = CYAN;
pub const COL_USE_NORM: &str = GREEN;
pub const COL_USE_HIGH: &str = YELLOW;
pub const COL_USE_VERY_HIGH: &str = ORANGE;
pub const COL_USE_SCREAMING: &str = RED;

pub fn color_by_breakpoint<T: Into<String>, const N: usize>(
    value: f64,
    breakpoints: &[f64; N],
    colors: &[&'static str; N],
    outer_color: T,
) -> Srgb8 {
    for (i, &bp) in breakpoints.iter().enumerate() {
        if value < bp {
            return Srgb8::from(colors[i]);
        }
    }
    Srgb8::from(outer_color.into())
}
const PCT_BPS: &[f64; 4] = &[20.0, 40.0, 60.0, 80.0];
const PCT_COLORS: &[&str; 4] = &[COL_USE_COOL, COL_USE_NORM, COL_USE_HIGH, COL_USE_VERY_HIGH];

pub fn color_by_pct(value: f64) -> String {
    color_by_breakpoint(value, PCT_BPS, PCT_COLORS, COL_USE_SCREAMING).to_hex()
}

// TODO implement proper gradients! yeah!
pub fn color_by_pct_custom(value: f64, breakpoints: &[f64; 4]) -> String {
    color_by_breakpoint(value, breakpoints, PCT_COLORS, COL_USE_SCREAMING).to_hex()
}

pub fn color_by_pct_rev(value: f64) -> String {
    color_by_breakpoint(
        value,
        PCT_BPS,
        &[
            COL_USE_SCREAMING,
            COL_USE_VERY_HIGH,
            COL_USE_HIGH,
            COL_USE_NORM,
        ],
        COL_USE_COOL,
    )
    .to_hex()
}

pub fn format_duration(seconds: f64) -> String {
    if seconds < 60.0 {
        // Handle small values
        let (value, unit) = if seconds < 1e-9 {
            (seconds * 1e12, "ps")
        } else if seconds < 1e-6 {
            (seconds * 1e9, "ns")
        } else if seconds < 1e-3 {
            (seconds * 1e6, "μs")
        } else if seconds < 1.0 {
            (seconds * 1e3, "ms")
        } else {
            (seconds, "s")
        };

        let precision = (2.0 - value.log10().floor()).clamp(0.0, 2.0) as usize;
        format!(
            "  {:.precision$} {:<2} ",
            value,
            unit,
            precision = precision
        )
    } else if seconds < 3_155_760_000.0 {
        // Less than 10 years
        if seconds < 3600.0 {
            // < 1 hour
            let min = (seconds / 60.0).floor() as u32;
            let sec = (seconds % 60.0) as u32;
            format!("{min:2} m {sec:2} s")
        } else if seconds < 86400.0 {
            // < 1 day
            let hr = (seconds / 3600.0).floor() as u32;
            let min = ((seconds % 3600.0) / 60.0) as u32;
            format!("{hr:2} h {min:2} m")
        } else if seconds < 604_800.0 {
            // < 1 week
            let day = (seconds / 86400.0).floor() as u32;
            let hr = ((seconds % 86400.0) / 3600.0) as u32;
            format!("{day:2} d {hr:2} h")
        } else if seconds < 31_557_600.0 {
            // < 1 year
            let week = (seconds / 604_800.0).floor() as u32;
            let day = ((seconds % 604_800.0) / 86_400.0) as u32;
            format!("{week:2} w {day:2} d")
        } else {
            // < 10 years
            let year = (seconds / 31_557_600.0).floor() as u32;
            let week = ((seconds % 31_557_600.0) / 604_800.0) as u32;
            format!("{year:2} y {week:2} w")
        }
    } else {
        " > 10 y  ".to_string()
    }
}
