//! The chair's styles: 0.5.2's palette (`domain::screen::Meaning`), read from the same SGR codes
//! the screen kit paints with, so the chair and every verb look alike. Without colour (`NO_COLOR`,
//! `TERM=dumb`) only bold and reverse are left, and symbols carry the meaning.

use ratatui::style::{Color, Modifier, Style};

use crate::cli::screen::{self, Environment, Screen};
use crate::domain::screen::Meaning;

/// How the chair paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    colour: bool,
}

impl Theme {
    /// A theme with or without colour.
    pub fn new(colour: bool) -> Self {
        Self { colour }
    }

    /// The theme `NO_COLOR` and `TERM` call for, on a terminal.
    pub fn from_environment() -> Self {
        let no_color = std::env::var("NO_COLOR").ok();
        let term = std::env::var("TERM").ok();
        let screen = Screen::from_environment(Environment {
            stdout_is_terminal: true,
            no_color: no_color.as_deref(),
            term: term.as_deref(),
        });
        Self::new(screen.colour)
    }

    /// The style of `meaning`.
    pub fn style(self, meaning: Meaning) -> Style {
        parse_sgr(screen::code(meaning), self.colour)
    }

    /// Reversed: the selected square, the palette's chosen row, a button.
    pub fn reverse(self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED)
    }

    /// Bold, in colour or not.
    pub fn bold(self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    /// The current tab's name: bold and underlined.
    pub fn current(self) -> Style {
        let style = self.bold();
        if self.colour {
            style.add_modifier(Modifier::UNDERLINED)
        } else {
            style.add_modifier(Modifier::REVERSED)
        }
    }
}

/// The style SGR parameters `code` set: bold, dim, the eight colours and the 256-colour form.
/// Without colour only bold is kept.
fn parse_sgr(code: &str, colour: bool) -> Style {
    let mut style = Style::new();
    let mut parts = code.split(';');
    while let Some(part) = parts.next() {
        match part {
            "1" => style = style.add_modifier(Modifier::BOLD),
            "2" if colour => style = style.add_modifier(Modifier::DIM),
            "38" => match parts.next() {
                Some("5") => {
                    let index = parts.next().map(str::parse::<u8>);
                    if let (true, Some(Ok(index))) = (colour, index) {
                        style = style.fg(Color::Indexed(index));
                    }
                }
                // A true colour: its three numbers are no parameters of their own.
                Some("2") => {
                    parts.nth(2);
                }
                _ => {}
            },
            part if colour => {
                let named = match part {
                    "30" => Color::Black,
                    "31" => Color::Red,
                    "32" => Color::Green,
                    "33" => Color::Yellow,
                    "34" => Color::Blue,
                    "35" => Color::Magenta,
                    "36" => Color::Cyan,
                    "37" => Color::Gray,
                    _ => continue,
                };
                style = style.fg(named);
            }
            _ => {}
        }
    }
    style
}

#[cfg(test)]
mod tests;
