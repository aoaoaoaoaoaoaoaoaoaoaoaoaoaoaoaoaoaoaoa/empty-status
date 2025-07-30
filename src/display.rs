use crate::core::{CYAN, GREEN, ORANGE, RED, YELLOW};

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
) -> String {
    for (i, &bp) in breakpoints.iter().enumerate() {
        if value < bp {
            return colors[i].to_string();
        }
    }
    outer_color.into()
}
const PCT_BPS: &[f64; 4] = &[20.0, 40.0, 60.0, 80.0];
const PCT_COLORS: &[&str; 4] = &[COL_USE_COOL, COL_USE_NORM, COL_USE_HIGH, COL_USE_VERY_HIGH];

pub fn color_by_pct(value: f64) -> String {
    color_by_breakpoint(value, PCT_BPS, PCT_COLORS, COL_USE_SCREAMING)
}

// TODO implement proper gradients! yeah!
pub fn color_by_pct_custom(value: f64, breakpoints: &[f64; 4]) -> String {
    color_by_breakpoint(value, breakpoints, PCT_COLORS, COL_USE_SCREAMING)
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
}

pub fn color<S: AsRef<str>, T: AsRef<str>>(text: S, color: T) -> String {
    pangofy(text.as_ref(), Some(color.as_ref()), None)
}

pub fn pangofy(text: &str, color: Option<&str>, background: Option<&str>) -> String {
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
