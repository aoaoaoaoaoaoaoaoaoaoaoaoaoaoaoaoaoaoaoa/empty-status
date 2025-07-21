use derive_builder::Builder;

use crate::core::{BLUE, GREEN, ORANGE, RED, YELLOW};

// Get appropriate color based on value and thresholds
pub const COL_USE_COOL: &str = BLUE;
pub const COL_USE_NORM: &str = GREEN;
pub const COL_USE_HIGH: &str = YELLOW;
pub const COL_USE_VERY_HIGH: &str = ORANGE;
pub const COL_USE_SCREAMING: &str = RED;

#[derive(Builder, Debug)]
pub struct RangeColorizer {
    #[builder(default = vec![20.0, 40.0, 60.0, 80.0])]
    breakpoints: Vec<f64>, // Breakpoints for color ranges
    #[builder(default = vec![COL_USE_COOL, COL_USE_NORM, COL_USE_HIGH, COL_USE_VERY_HIGH, COL_USE_SCREAMING])]
    colors: Vec<&'static str>,
    #[builder(default = false)]
    reverse: bool, // Whether to reverse the color order
}

impl RangeColorizer {
    pub fn validate(&self) -> Result<(), String> {
        if self.breakpoints.len() + 1 != self.colors.len() {
            return Err(format!(
                "Expected {} colors, got {}",
                self.breakpoints.len() + 1,
                self.colors.len()
            ));
        }
        Ok(())
    }

    pub fn get(&self, value: f64) -> &'static str {
        // find first breakpoint ≥ value, or use breakpoints.len() if none
        let idx = self
            .breakpoints
            .iter()
            .position(|&bp| value <= bp)
            .unwrap_or(self.breakpoints.len());
        // if reversed, mirror index within colors; else leave it
        let i = if self.reverse {
            self.colors.len() - 1 - idx
        } else {
            idx
        };
        &self.colors[i]
    }
}

// Add color to text using pango markup
pub fn color<S: Into<String>>(text: S, color: &str) -> String {
    pangofy(text, Some(color), None)
}

// Create a pango formatted string
pub fn pangofy<S: Into<String>>(text: S, color: Option<&str>, background: Option<&str>) -> String {
    let mut attrs = Vec::new();

    if let Some(c) = color {
        attrs.push(format!("color='{c}'"));
    }

    if let Some(bg) = background {
        attrs.push(format!("background='{bg}'"));
    }

    let text = text.into();
    if attrs.is_empty() {
        text
    } else {
        format!("<span {}>{text}</span>", attrs.join(" "))
    }
}

// Format a value, automatically choosing a time unit
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

        let precision = std::cmp::max(0, 2 - value.log10().floor() as i32);
        format!(
            "  {:.precision$} {:<2} ",
            value,
            unit,
            precision = precision as usize
        )
    } else if seconds < 3155760000.0 {
        // Less than 10 years
        if seconds < 3600.0 {
            // < 1 hour
            let min = (seconds / 60.0).floor() as i32;
            let sec = (seconds % 60.0) as i32;
            format!("{min:2} m {sec:2} s")
        } else if seconds < 86400.0 {
            // < 1 day
            let hr = (seconds / 3600.0).floor() as i32;
            let min = ((seconds % 3600.0) / 60.0) as i32;
            format!("{hr:2} h {min:2} m")
        } else if seconds < 604800.0 {
            // < 1 week
            let day = (seconds / 86400.0).floor() as i32;
            let hr = ((seconds % 86400.0) / 3600.0) as i32;
            format!("{day:2} d {hr:2} h")
        } else if seconds < 31557600.0 {
            // < 1 year
            let week = (seconds / 604800.0).floor() as i32;
            let day = ((seconds % 604800.0) / 86400.0) as i32;
            format!("{week:2} w {day:2} d")
        } else {
            // < 10 years
            let year = (seconds / 31557600.0).floor() as i32;
            let week = ((seconds % 31557600.0) / 604800.0) as i32;
            format!("{year:2} y {week:2} w")
        }
    } else {
        " > 10 y  ".to_string()
    }
}
