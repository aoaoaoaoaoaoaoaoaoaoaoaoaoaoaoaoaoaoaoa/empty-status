use crate::render::color::{CYAN, GREEN, ORANGE, RED, Rgb8, YELLOW};

pub const USE_COOL: Rgb8 = CYAN;
pub const USE_NORMAL: Rgb8 = GREEN;
pub const USE_HIGH: Rgb8 = YELLOW;
pub const USE_VERY_HIGH: Rgb8 = ORANGE;
pub const USE_SCREAMING: Rgb8 = RED;

const PERCENT_BREAKPOINTS: [f64; 4] = [20.0, 40.0, 60.0, 80.0];
const PERCENT_COLORS: [Rgb8; 4] = [USE_COOL, USE_NORMAL, USE_HIGH, USE_VERY_HIGH];

pub fn color_by_breakpoint<const N: usize>(
    value: f64,
    breakpoints: [f64; N],
    colors: [Rgb8; N],
    outer: Rgb8,
) -> Rgb8 {
    breakpoints
        .into_iter()
        .zip(colors)
        .find_map(|(breakpoint, color)| (value < breakpoint).then_some(color))
        .unwrap_or(outer)
}

pub fn color_by_percent(value: f64) -> Rgb8 {
    color_by_breakpoint(value, PERCENT_BREAKPOINTS, PERCENT_COLORS, USE_SCREAMING)
}

pub fn color_by_thresholds(value: f64, breakpoints: [f64; 4]) -> Rgb8 {
    color_by_breakpoint(value, breakpoints, PERCENT_COLORS, USE_SCREAMING)
}

pub fn color_by_percent_remaining(value: f64) -> Rgb8 {
    color_by_breakpoint(
        value,
        PERCENT_BREAKPOINTS,
        [USE_SCREAMING, USE_VERY_HIGH, USE_HIGH, USE_NORMAL],
        USE_COOL,
    )
}

pub fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds:>6} s  ")
    } else if seconds < 3_600 {
        format!("{:2} m {:2} s", seconds / 60, seconds % 60)
    } else if seconds < 86_400 {
        format!("{:2} h {:2} m", seconds / 3_600, seconds % 3_600 / 60)
    } else if seconds < 604_800 {
        format!("{:2} d {:2} h", seconds / 86_400, seconds % 86_400 / 3_600)
    } else if seconds < 31_557_600 {
        format!(
            "{:2} w {:2} d",
            seconds / 604_800,
            seconds % 604_800 / 86_400
        )
    } else if seconds < 315_576_000 {
        format!(
            "{:2} y {:2} w",
            seconds / 31_557_600,
            seconds % 31_557_600 / 604_800
        )
    } else {
        " > 10 y  ".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn duration_ceiling_is_ten_years() {
        assert_eq!(format_duration(315_576_000), " > 10 y  ");
    }
}
