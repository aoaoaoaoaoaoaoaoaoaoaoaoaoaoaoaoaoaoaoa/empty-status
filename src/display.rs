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
