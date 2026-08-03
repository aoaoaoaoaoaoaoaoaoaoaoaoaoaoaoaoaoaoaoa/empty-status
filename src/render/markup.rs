use std::fmt;

use super::color::Rgb8;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Markup(Vec<Run>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    text: String,
    fg: Option<Rgb8>,
}

impl Markup {
    pub const fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self(vec![Run {
            text: text.into(),
            fg: None,
        }])
    }

    pub fn fg(mut self, color: Rgb8) -> Self {
        for run in &mut self.0 {
            if run.fg.is_none() {
                run.fg = Some(color);
            }
        }
        self
    }

    pub fn append(mut self, mut other: Self) -> Self {
        self.0.append(&mut other.0);
        self
    }

    pub fn delimited(left: impl Into<Self>, inner: Self, right: impl Into<Self>) -> Self {
        left.into().append(inner).append(right.into())
    }

    pub fn bracketed(inner: Self) -> Self {
        Self::delimited("[", inner, "]")
    }

    pub fn join(separator: impl Into<Self>, parts: impl IntoIterator<Item = Self>) -> Self {
        let separator = separator.into();
        let mut parts = parts.into_iter();
        let Some(mut joined) = parts.next() else {
            return Self::empty();
        };
        for part in parts {
            joined = joined.append(separator.clone()).append(part);
        }
        joined
    }

    fn write_pango(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for run in &self.0 {
            if let Some(color) = run.fg {
                write!(f, "<span color='{color}'>")?;
            }
            escape_pango(f, &run.text)?;
            if run.fg.is_some() {
                f.write_str("</span>")?;
            }
        }
        Ok(())
    }
}

fn escape_pango(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    for ch in text.chars() {
        f.write_str(match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '\'' => "&apos;",
            '"' => "&quot;",
            _ => {
                write!(f, "{ch}")?;
                continue;
            }
        })?;
    }
    Ok(())
}

impl From<&str> for Markup {
    fn from(value: &str) -> Self {
        Self::text(value)
    }
}

impl From<String> for Markup {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}

impl std::ops::Add for Markup {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.append(rhs)
    }
}

impl fmt::Display for Markup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_pango(f)
    }
}

#[cfg(test)]
mod tests {
    use super::Markup;
    use crate::render::color::{GREEN, RED};

    #[test]
    fn escapes_and_preserves_inner_color() {
        let markup = (Markup::text("<&") + Markup::text("inner").fg(RED)).fg(GREEN);
        assert_eq!(
            markup.to_string(),
            "<span color='#B5BD68'>&lt;&amp;</span><span color='#CC6666'>inner</span>"
        );
    }
}
