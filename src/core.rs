use serde::{Deserialize, Serialize};

use crate::render::{
    color::{DARK_GREY, RED, Rgb8, VIOLET, YELLOW},
    markup::Markup,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    Ok,
    Degraded,
    Error,
}

impl Health {
    const fn border(self) -> Rgb8 {
        match self {
            Self::Ok => DARK_GREY,
            Self::Degraded => YELLOW,
            Self::Error => RED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub body: Markup,
    pub health: Health,
}

impl View {
    pub const fn new(body: Markup, health: Health) -> Self {
        Self { body, health }
    }

    pub const fn ok(body: Markup) -> Self {
        Self::new(body, Health::Ok)
    }

    pub fn loading(label: &str) -> Self {
        Self::new(
            Markup::text(format!("{label} ")) + Markup::text("loading").fg(VIOLET),
            Health::Degraded,
        )
    }

    pub fn error(label: &str, message: impl Into<String>) -> Self {
        Self::new(
            Markup::text(format!("{label}: ")) + Markup::text(message).fg(RED),
            Health::Error,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    Other(u8),
}

impl From<u8> for Button {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Left,
            2 => Self::Middle,
            3 => Self::Right,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct I3Click {
    pub name: String,
    pub button: u8,
}

impl I3Click {
    pub fn button(self) -> Button {
        self.button.into()
    }
}

#[derive(Debug, Serialize)]
pub struct I3Block {
    full_text: String,
    name: String,
    markup: &'static str,
    border: String,
    separator: bool,
    separator_block_width: u8,
}

impl I3Block {
    pub fn new(name: &str, padding: u8, view: &View) -> Self {
        let pad = " ".repeat(usize::from(padding));
        Self {
            full_text: format!("{pad}{}{pad}", view.body),
            name: name.to_owned(),
            markup: "pango",
            border: view.health.border().to_string(),
            separator: false,
            separator_block_width: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{I3Block, View};
    use crate::render::markup::Markup;

    #[test]
    fn separator_is_a_json_boolean() {
        let block = I3Block::new("Time::0", 0, &View::ok(Markup::text("time")));
        let value = serde_json::to_value(block);
        assert!(value.is_ok());
        let Ok(value) = value else { return };
        assert_eq!(value["separator"], false);
    }
}
