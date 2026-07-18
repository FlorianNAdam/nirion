pub use console::{Attribute, Color};
use console::{StyledObject, style};

pub const GREY: Color = Color::Color256(7);
pub const DARK_GREY: Color = Color::Color256(8);

macro_rules! colorize {
    (
        colors { $($color_method:ident => $color:expr),* $(,)? }
        backgrounds { $($bg_method:ident => $bg_color:expr),* $(,)? }
        attrs { $($attr_method:ident => $attr:expr),* $(,)? }
    ) => {
        pub trait Colorize: Sized {
            fn force_styling(self, value: bool) -> StyledObject<Self>;
            fn for_stderr(self) -> StyledObject<Self>;
            fn for_stdout(self) -> StyledObject<Self>;
            fn fg(self, color: Color) -> StyledObject<Self>;
            fn bg(self, color: Color) -> StyledObject<Self>;
            fn attr(self, attr: Attribute) -> StyledObject<Self>;
            fn color256(self, color: u8) -> StyledObject<Self>;
            fn true_color(self, r: u8, g: u8, b: u8) -> StyledObject<Self>;
            fn bright(self) -> StyledObject<Self>;
            fn on_color256(self, color: u8) -> StyledObject<Self>;
            fn on_true_color(self, r: u8, g: u8, b: u8) -> StyledObject<Self>;
            fn on_bright(self) -> StyledObject<Self>;

            $(
                fn $color_method(self) -> StyledObject<Self>;
            )*

            $(
                fn $bg_method(self) -> StyledObject<Self>;
            )*

            $(
                fn $attr_method(self) -> StyledObject<Self>;
            )*
        }

        impl<T> Colorize for T {
            fn force_styling(self, value: bool) -> StyledObject<Self> {
                style(self).force_styling(value)
            }

            fn for_stderr(self) -> StyledObject<Self> {
                style(self).for_stderr()
            }

            fn for_stdout(self) -> StyledObject<Self> {
                style(self).for_stdout()
            }

            fn fg(self, color: Color) -> StyledObject<Self> {
                style(self).fg(color)
            }

            fn bg(self, color: Color) -> StyledObject<Self> {
                style(self).bg(color)
            }

            fn attr(self, attr: Attribute) -> StyledObject<Self> {
                style(self).attr(attr)
            }

            fn color256(self, color: u8) -> StyledObject<Self> {
                self.fg(Color::Color256(color))
            }

            fn true_color(self, r: u8, g: u8, b: u8) -> StyledObject<Self> {
                self.fg(Color::TrueColor(r, g, b))
            }

            fn bright(self) -> StyledObject<Self> {
                style(self).bright()
            }

            fn on_color256(self, color: u8) -> StyledObject<Self> {
                self.bg(Color::Color256(color))
            }

            fn on_true_color(self, r: u8, g: u8, b: u8) -> StyledObject<Self> {
                self.bg(Color::TrueColor(r, g, b))
            }

            fn on_bright(self) -> StyledObject<Self> {
                style(self).on_bright()
            }

            $(
                fn $color_method(self) -> StyledObject<Self> {
                    self.fg($color)
                }
            )*

            $(
                fn $bg_method(self) -> StyledObject<Self> {
                    self.bg($bg_color)
                }
            )*

            $(
                fn $attr_method(self) -> StyledObject<Self> {
                    self.attr($attr)
                }
            )*
        }
    };
}

colorize! {
    colors {
        black => Color::Black,
        red => Color::Red,
        green => Color::Green,
        yellow => Color::Yellow,
        blue => Color::Blue,
        magenta => Color::Magenta,
        cyan => Color::Cyan,
        white => Color::White,
        grey => GREY,
        dark_grey => DARK_GREY,
    }
    backgrounds {
        on_black => Color::Black,
        on_red => Color::Red,
        on_green => Color::Green,
        on_yellow => Color::Yellow,
        on_blue => Color::Blue,
        on_magenta => Color::Magenta,
        on_cyan => Color::Cyan,
        on_white => Color::White,
        on_grey => GREY,
        on_dark_grey => DARK_GREY,
    }
    attrs {
        bold => Attribute::Bold,
        dim => Attribute::Dim,
        italic => Attribute::Italic,
        underlined => Attribute::Underlined,
        blink => Attribute::Blink,
        blink_fast => Attribute::BlinkFast,
        reverse => Attribute::Reverse,
        hidden => Attribute::Hidden,
        strikethrough => Attribute::StrikeThrough,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use console::strip_ansi_codes;

    fn forced(styled: StyledObject<&str>) -> String {
        styled.force_styling(true).to_string()
    }

    #[test]
    fn foreground_shortcuts_match_fg_colors() {
        let cases = [
            (
                "black",
                forced("text".fg(Color::Black)),
                forced("text".black()),
            ),
            ("red", forced("text".fg(Color::Red)), forced("text".red())),
            (
                "green",
                forced("text".fg(Color::Green)),
                forced("text".green()),
            ),
            (
                "yellow",
                forced("text".fg(Color::Yellow)),
                forced("text".yellow()),
            ),
            (
                "blue",
                forced("text".fg(Color::Blue)),
                forced("text".blue()),
            ),
            (
                "magenta",
                forced("text".fg(Color::Magenta)),
                forced("text".magenta()),
            ),
            (
                "cyan",
                forced("text".fg(Color::Cyan)),
                forced("text".cyan()),
            ),
            (
                "white",
                forced("text".fg(Color::White)),
                forced("text".white()),
            ),
            ("grey", forced("text".fg(GREY)), forced("text".grey())),
            (
                "dark_grey",
                forced("text".fg(DARK_GREY)),
                forced("text".dark_grey()),
            ),
        ];

        for (name, expected, actual) in cases {
            assert_eq!(actual, expected, "{name}");
        }
    }

    #[test]
    fn background_shortcuts_match_bg_colors() {
        let cases = [
            (
                "on_black",
                forced("text".bg(Color::Black)),
                forced("text".on_black()),
            ),
            (
                "on_red",
                forced("text".bg(Color::Red)),
                forced("text".on_red()),
            ),
            (
                "on_green",
                forced("text".bg(Color::Green)),
                forced("text".on_green()),
            ),
            (
                "on_yellow",
                forced("text".bg(Color::Yellow)),
                forced("text".on_yellow()),
            ),
            (
                "on_blue",
                forced("text".bg(Color::Blue)),
                forced("text".on_blue()),
            ),
            (
                "on_magenta",
                forced("text".bg(Color::Magenta)),
                forced("text".on_magenta()),
            ),
            (
                "on_cyan",
                forced("text".bg(Color::Cyan)),
                forced("text".on_cyan()),
            ),
            (
                "on_white",
                forced("text".bg(Color::White)),
                forced("text".on_white()),
            ),
            ("on_grey", forced("text".bg(GREY)), forced("text".on_grey())),
            (
                "on_dark_grey",
                forced("text".bg(DARK_GREY)),
                forced("text".on_dark_grey()),
            ),
        ];

        for (name, expected, actual) in cases {
            assert_eq!(actual, expected, "{name}");
        }
    }

    #[test]
    fn parameterized_colors_match_explicit_colors() {
        assert_eq!(
            forced("text".fg(Color::Color256(42))),
            forced("text".color256(42))
        );
        assert_eq!(
            forced("text".fg(Color::TrueColor(1, 2, 3))),
            forced("text".true_color(1, 2, 3))
        );
        assert_eq!(
            forced("text".bg(Color::Color256(42))),
            forced("text".on_color256(42))
        );
        assert_eq!(
            forced("text".bg(Color::TrueColor(1, 2, 3))),
            forced("text".on_true_color(1, 2, 3))
        );
    }

    #[test]
    fn attribute_shortcuts_match_attr_values() {
        let cases = [
            (
                "bold",
                forced("text".attr(Attribute::Bold)),
                forced("text".bold()),
            ),
            (
                "dim",
                forced("text".attr(Attribute::Dim)),
                forced("text".dim()),
            ),
            (
                "italic",
                forced("text".attr(Attribute::Italic)),
                forced("text".italic()),
            ),
            (
                "underlined",
                forced("text".attr(Attribute::Underlined)),
                forced("text".underlined()),
            ),
            (
                "blink",
                forced("text".attr(Attribute::Blink)),
                forced("text".blink()),
            ),
            (
                "blink_fast",
                forced("text".attr(Attribute::BlinkFast)),
                forced("text".blink_fast()),
            ),
            (
                "reverse",
                forced("text".attr(Attribute::Reverse)),
                forced("text".reverse()),
            ),
            (
                "hidden",
                forced("text".attr(Attribute::Hidden)),
                forced("text".hidden()),
            ),
            (
                "strikethrough",
                forced("text".attr(Attribute::StrikeThrough)),
                forced("text".strikethrough()),
            ),
        ];

        for (name, expected, actual) in cases {
            assert_eq!(actual, expected, "{name}");
        }
    }

    #[test]
    fn styling_controls_apply_to_raw_values() {
        let styled = "text"
            .for_stdout()
            .red()
            .on_blue()
            .bright()
            .on_bright()
            .force_styling(true)
            .to_string();

        assert_eq!(strip_ansi_codes(&styled), "text");
        assert!(styled.contains("\x1b["));

        let stderr = "text"
            .for_stderr()
            .green()
            .force_styling(true)
            .to_string();

        assert_eq!(strip_ansi_codes(&stderr), "text");
        assert!(stderr.contains("\x1b["));
    }

    #[test]
    fn direct_style_controls_use_colorize_trait_methods() {
        let forced_text = Colorize::force_styling("text", true).to_string();
        assert_eq!(strip_ansi_codes(&forced_text), "text");

        let bright = Colorize::bright("text")
            .force_styling(true)
            .to_string();
        assert_eq!(strip_ansi_codes(&bright), "text");

        let on_bright = Colorize::on_bright("text")
            .force_styling(true)
            .to_string();
        assert_eq!(strip_ansi_codes(&on_bright), "text");
    }
}
