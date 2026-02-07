use crate::render::color::Srgb8;
use crate::render::markup::{Markup, Span, Style};

#[allow(dead_code)]
pub fn to_pango(markup: &Markup) -> String {
    let mut out = String::new();
    for span in markup.spans() {
        match span {
            Span::Text(text) => out.push_str(&render_text(text, Style::default())),
            Span::Styled(style, inner) => {
                out.push_str(&render_styled(*style, inner));
            }
        }
    }
    out
}

fn render_styled(style: Style, inner: &Markup) -> String {
    let mut out = String::new();
    for span in inner.spans() {
        match span {
            Span::Text(text) => out.push_str(&render_text(text, style)),
            Span::Styled(child_style, child_inner) => {
                out.push_str(&render_styled(merge(style, *child_style), child_inner));
            }
        }
    }
    out
}

fn merge(outer: Style, inner: Style) -> Style {
    Style {
        fg: inner.fg.or(outer.fg),
        bg: inner.bg.or(outer.bg),
    }
}

fn render_text(text: &str, style: Style) -> String {
    let mut attrs = Vec::new();
    if let Some(fg) = style.fg {
        attrs.push(format!("color='{}'", fg.to_hex()));
    }
    if let Some(bg) = style.bg {
        attrs.push(format!("background='{}'", bg.to_hex()));
    }
    if attrs.is_empty() {
        text.to_string()
    } else {
        format!("<span {}>{}</span>", attrs.join(" "), text)
    }
}

trait Hex {
    fn to_hex(self) -> String;
}

impl Hex for Srgb8 {
    fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}
