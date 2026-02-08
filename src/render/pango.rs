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
    let text = escape_pango(text);
    let mut attrs = Vec::new();
    if let Some(fg) = style.fg {
        attrs.push(format!("color='{}'", fg.to_hex()));
    }
    if let Some(bg) = style.bg {
        attrs.push(format!("background='{}'", bg.to_hex()));
    }
    if attrs.is_empty() {
        text
    } else {
        format!("<span {}>{}</span>", attrs.join(" "), text)
    }
}

fn escape_pango(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\'' => out.push_str("&apos;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::to_pango;
    use crate::render::markup::Markup;

    #[test]
    fn escapes_text() {
        let m = Markup::text("<&>\"'");
        let out = to_pango(&m);
        assert_eq!(out, "&lt;&amp;&gt;&quot;&apos;");
    }
}
