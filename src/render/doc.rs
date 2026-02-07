use crate::render::color::Srgb8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Style {
    pub fg: Option<Srgb8>,
    pub bg: Option<Srgb8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Doc {
    spans: Vec<Span>,
}

impl Doc {
    #[allow(dead_code)]
    pub fn new(spans: Vec<Span>) -> Self {
        Self { spans }
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Text { text: String, style: Style },
}

#[allow(dead_code)]
pub fn text(s: impl Into<String>) -> Span {
    Span::Text {
        text: s.into(),
        style: Style::default(),
    }
}

#[allow(dead_code)]
pub fn styled(span: Span, style: Style) -> Span {
    match span {
        Span::Text { text, .. } => Span::Text { text, style },
    }
}
