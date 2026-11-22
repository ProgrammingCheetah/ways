use ratatui::style::{Color, Modifier, Style};

use super::{Theme, parse_sgr};
use crate::domain::screen::Meaning;

#[test]
fn the_palette_is_the_screen_kits() {
    let theme = Theme::new(true);

    assert_eq!(
        theme.style(Meaning::Program),
        Style::new()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Indexed(201))
    );
    assert_eq!(
        theme.style(Meaning::Waiting),
        Style::new()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Indexed(141))
    );
    assert_eq!(theme.style(Meaning::Noun), Style::new().fg(Color::Yellow));
    assert_eq!(theme.style(Meaning::Done), Style::new().fg(Color::Green));
    assert_eq!(theme.style(Meaning::Failed), Style::new().fg(Color::Red));
    assert_eq!(
        theme.style(Meaning::Verb),
        Style::new().add_modifier(Modifier::BOLD).fg(Color::Cyan)
    );
    assert_eq!(
        theme.style(Meaning::Dim),
        Style::new().add_modifier(Modifier::DIM)
    );
}

#[test]
fn without_colour_only_bold_and_reverse_are_left() {
    let theme = Theme::new(false);

    for meaning in [Meaning::Noun, Meaning::Done, Meaning::Failed, Meaning::Dim] {
        assert_eq!(theme.style(meaning), Style::new(), "{meaning:?}");
    }
    assert_eq!(theme.style(Meaning::Program), theme.bold());
    assert_eq!(theme.style(Meaning::Waiting), theme.bold());
    assert_eq!(
        theme.current(),
        Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    );
    assert_eq!(
        theme.reverse(),
        Style::new().add_modifier(Modifier::REVERSED)
    );
}

#[test]
fn an_unknown_parameter_is_left_out() {
    assert_eq!(parse_sgr("4;38;2;1;2;3;99", true), Style::new());
}
