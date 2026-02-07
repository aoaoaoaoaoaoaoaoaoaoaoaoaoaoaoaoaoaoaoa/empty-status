#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Markup {
    spans: Vec<Span>,
}

impl Markup {
    #[must_use]
    pub fn empty() -> Self {
        Self { spans: vec![] }
    }

    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            spans: vec![Span::Text(text.into())],
        }
    }

    #[must_use]
    pub fn push(mut self, span: Span) -> Self {
        self.spans.push(span);
        self
    }

    #[must_use]
    pub fn append(mut self, other: Self) -> Self {
        self.spans.extend(other.spans);
        self
    }

    #[must_use]
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }
}

impl Default for Markup {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Text(String),
    Styled(Style, Markup),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Option<crate::render::color::Srgb8>,
    pub bg: Option<crate::render::color::Srgb8>,
}

impl Style {
    #[must_use]
    pub fn fg(self, fg: crate::render::color::Srgb8) -> Self {
        Self {
            fg: Some(fg),
            ..self
        }
    }

    #[must_use]
    pub fn bg(self, bg: crate::render::color::Srgb8) -> Self {
        Self {
            bg: Some(bg),
            ..self
        }
    }
}
