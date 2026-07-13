use ratatui::style::Color;

use crate::harbor::Condition;

/// Colors for the scene, chosen per terminal capability so the harbor stays
/// readable from truecolor down to 16-color terminals.
pub struct Theme {
    pub water: Color,
    pub pier: Color,
    pub text: Color,
    pub dim: Color,
    calm: Color,
    loading: Color,
    sealed: Color,
    outbound: Color,
    incoming: Color,
    diverged: Color,
    local: Color,
    blocked: Color,
    moored: Color,
}

impl Theme {
    /// Pick a palette from the environment. `COLORTERM` advertises truecolor;
    /// a 256-color `TERM` gets the indexed palette; everything else falls back
    /// to the 16 ANSI colors and inherits the user's terminal scheme.
    pub fn detect() -> Self {
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            Self::truecolor()
        } else if term.contains("256") {
            Self::indexed()
        } else {
            Self::ansi()
        }
    }

    fn truecolor() -> Self {
        Self {
            water: Color::Rgb(86, 148, 195),
            pier: Color::Rgb(158, 129, 96),
            text: Color::Rgb(220, 223, 228),
            dim: Color::Rgb(120, 128, 140),
            calm: Color::Rgb(126, 192, 145),
            loading: Color::Rgb(229, 192, 100),
            sealed: Color::Rgb(184, 152, 235),
            outbound: Color::Rgb(102, 199, 216),
            incoming: Color::Rgb(105, 157, 216),
            diverged: Color::Rgb(214, 150, 91),
            local: Color::Rgb(172, 178, 188),
            blocked: Color::Rgb(233, 116, 116),
            moored: Color::Rgb(130, 138, 150),
        }
    }

    fn indexed() -> Self {
        Self {
            water: Color::Indexed(74),
            pier: Color::Indexed(137),
            text: Color::Indexed(252),
            dim: Color::Indexed(244),
            calm: Color::Indexed(114),
            loading: Color::Indexed(179),
            sealed: Color::Indexed(140),
            outbound: Color::Indexed(80),
            incoming: Color::Indexed(74),
            diverged: Color::Indexed(173),
            local: Color::Indexed(250),
            blocked: Color::Indexed(167),
            moored: Color::Indexed(245),
        }
    }

    fn ansi() -> Self {
        Self {
            water: Color::Blue,
            pier: Color::Yellow,
            text: Color::White,
            dim: Color::DarkGray,
            calm: Color::Green,
            loading: Color::Yellow,
            sealed: Color::Magenta,
            outbound: Color::Cyan,
            incoming: Color::Blue,
            diverged: Color::Yellow,
            local: Color::White,
            blocked: Color::Red,
            moored: Color::Gray,
        }
    }

    pub fn condition(&self, condition: Condition) -> Color {
        match condition {
            Condition::Blocked => self.blocked,
            Condition::Sealed => self.sealed,
            Condition::Loading => self.loading,
            Condition::Outbound => self.outbound,
            Condition::Incoming => self.incoming,
            Condition::Diverged => self.diverged,
            Condition::Local => self.local,
            Condition::Awaiting => self.outbound,
            Condition::Calm => self.calm,
            Condition::Moored => self.moored,
        }
    }
}
