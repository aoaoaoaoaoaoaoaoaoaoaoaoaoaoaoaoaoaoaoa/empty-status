use crate::render::color::Srgb8;
use crate::render::doc::{Doc, Span, Style};

#[allow(dead_code)]
pub fn to_pango(doc: &Doc) -> String {
    let mut out = String::new();
    for span in doc.spans() {
        match span {
            Span::Text { text, style } => {
                out.push_str(&render_text(text, *style));
            }
        }
    }
    out
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
