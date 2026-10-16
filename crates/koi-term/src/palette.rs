//! Terminal colors: Koi's default palette and per-cell color resolution.

use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

const fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

/// The colors a terminal draws with when the application does not override them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The 16 ANSI colors: 8 normal followed by 8 bright.
    pub ansi: [Rgb; 16],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// Background of selected cells.
    pub selection: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            ansi: [
                rgb(0x23262e),
                rgb(0xe5736b),
                rgb(0x8cc47a),
                rgb(0xe8c170),
                rgb(0x6fa6e8),
                rgb(0xc48de0),
                rgb(0x5ec4c4),
                rgb(0xc9ccd3),
                rgb(0x5d6270),
                rgb(0xff8a80),
                rgb(0xa6dc93),
                rgb(0xf5d48a),
                rgb(0x8fbcf5),
                rgb(0xd8a8f0),
                rgb(0x7fdada),
                rgb(0xeef0f4),
            ],
            foreground: rgb(0xd7dae0),
            background: rgb(0x16181d),
            cursor: rgb(0xf2a65a),
            selection: rgb(0x34405a),
        }
    }
}

/// Final colors of one cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CellColors {
    pub foreground: Rgb,
    pub background: Rgb,
}

impl Palette {
    /// ANSI colors with dark text on a light background.
    pub fn light() -> Self {
        Self {
            ansi: [
                rgb(0x303640),
                rgb(0xad343a),
                rgb(0x35733d),
                rgb(0x89600d),
                rgb(0x285fa6),
                rgb(0x854699),
                rgb(0x206f76),
                rgb(0xc1c6ce),
                rgb(0x626b78),
                rgb(0xc54248),
                rgb(0x3d8246),
                rgb(0x98700d),
                rgb(0x3472bd),
                rgb(0x9854ac),
                rgb(0x287f87),
                rgb(0xeff1f5),
            ],
            foreground: rgb(0x303640),
            background: rgb(0xf8f9fb),
            cursor: rgb(0x996019),
            selection: rgb(0xd4e2f5),
        }
    }

    /// The color at a terminal color index: 0–255 plus the named slots
    /// (foreground, background, cursor, dim and bright variants).
    pub(crate) fn indexed(&self, index: usize) -> Rgb {
        const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        match index {
            0..16 => self.ansi[index],
            16..232 => {
                let i = index - 16;
                Rgb {
                    r: CUBE_LEVELS[i / 36],
                    g: CUBE_LEVELS[i / 6 % 6],
                    b: CUBE_LEVELS[i % 6],
                }
            }
            232..256 => {
                let level = 8 + 10 * (index - 232) as u8;
                Rgb {
                    r: level,
                    g: level,
                    b: level,
                }
            }
            _ => self.named(index),
        }
    }

    fn named(&self, index: usize) -> Rgb {
        const DIM_FIRST: usize = NamedColor::DimBlack as usize;
        const DIM_LAST: usize = NamedColor::DimWhite as usize;
        match index {
            i if i == NamedColor::Background as usize => self.background,
            i if i == NamedColor::Cursor as usize => self.cursor,
            i if i == NamedColor::BrightForeground as usize => self.ansi[15],
            i if i == NamedColor::DimForeground as usize => dim(self.foreground, self.background),
            DIM_FIRST..=DIM_LAST => dim(self.ansi[index - DIM_FIRST], self.background),
            _ => self.foreground,
        }
    }

    /// Resolves a color, preferring colors the application set at runtime.
    pub(crate) fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Named(named) => self.lookup(named as usize, overrides),
            Color::Indexed(index) => self.lookup(usize::from(index), overrides),
        }
    }

    fn lookup(&self, index: usize, overrides: &Colors) -> Rgb {
        overrides[index].unwrap_or_else(|| self.indexed(index))
    }

    /// Resolves the colors of `cell`, applying bold-is-bright, dim, inverse and hidden.
    pub(crate) fn cell_colors(&self, cell: &Cell, overrides: &Colors) -> CellColors {
        let mut foreground = match cell.fg {
            Color::Named(named) if cell.flags.contains(Flags::BOLD) && (named as usize) < 8 => {
                self.lookup(named as usize + 8, overrides)
            }
            Color::Indexed(index) if cell.flags.contains(Flags::BOLD) && index < 8 => {
                self.lookup(usize::from(index) + 8, overrides)
            }
            color => self.resolve(color, overrides),
        };
        let mut background = self.resolve(cell.bg, overrides);
        if cell.flags.contains(Flags::DIM) {
            foreground = dim(foreground, background);
        }
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut foreground, &mut background);
        }
        if brightness(self.background) >= 180
            && cell.bg != Color::Named(NamedColor::Background)
            && !cell.flags.contains(Flags::INVERSE)
            && brightness(background) < 108
        {
            let original_background = background;
            background = blend(self.background, original_background, 32);
            foreground =
                if brightness(foreground).saturating_sub(brightness(original_background)) >= 130 {
                    self.foreground
                } else {
                    blend(self.foreground, background, 64)
                };
        }
        if cell.flags.contains(Flags::HIDDEN) {
            foreground = background;
        }
        CellColors {
            foreground,
            background,
        }
    }
}

fn brightness(color: Rgb) -> u8 {
    ((u32::from(color.r) * 299 + u32::from(color.g) * 587 + u32::from(color.b) * 114) / 1000) as u8
}

fn blend(base: Rgb, other: Rgb, other_weight: u16) -> Rgb {
    let channel = |a: u8, b: u8| {
        ((u16::from(a) * (256 - other_weight) + u16::from(b) * other_weight) / 256) as u8
    };
    Rgb {
        r: channel(base.r, other.r),
        g: channel(base.g, other.g),
        b: channel(base.b, other.b),
    }
}

/// Blends `color` one third of the way toward `background`.
fn dim(color: Rgb, background: Rgb) -> Rgb {
    let mix = |c: u8, b: u8| ((u16::from(c) * 2 + u16::from(b)) / 3) as u8;
    Rgb {
        r: mix(color.r, background.r),
        g: mix(color.g, background.g),
        b: mix(color.b, background.b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(fg: Color, bg: Color, flags: Flags) -> Cell {
        Cell {
            fg,
            bg,
            flags,
            ..Cell::default()
        }
    }

    #[test]
    fn color_cube_and_grayscale() {
        let palette = Palette::default();
        assert_eq!(palette.indexed(16), rgb(0x000000));
        assert_eq!(palette.indexed(231), rgb(0xffffff));
        assert_eq!(palette.indexed(196), rgb(0xff0000));
        assert_eq!(palette.indexed(232), rgb(0x080808));
        assert_eq!(palette.indexed(255), rgb(0xeeeeee));
    }

    #[test]
    fn named_slots() {
        let palette = Palette::default();
        assert_eq!(
            palette.indexed(NamedColor::Foreground as usize),
            palette.foreground
        );
        assert_eq!(
            palette.indexed(NamedColor::Background as usize),
            palette.background
        );
        assert_eq!(palette.indexed(NamedColor::Cursor as usize), palette.cursor);
        assert_eq!(
            palette.indexed(NamedColor::DimRed as usize),
            dim(palette.ansi[1], palette.background)
        );
    }

    #[test]
    fn light_palette_keeps_default_text_readable() {
        let palette = Palette::light();
        let colors = palette.cell_colors(&Cell::default(), &Colors::default());
        assert_eq!(colors.foreground, palette.foreground);
        assert_eq!(colors.background, palette.background);
        assert!(palette.background.r > palette.foreground.r);
    }

    #[test]
    fn runtime_overrides_win() {
        let palette = Palette::default();
        let mut overrides = Colors::default();
        overrides[1] = Some(rgb(0x123456));
        let red = Color::Named(NamedColor::Red);
        assert_eq!(palette.resolve(red, &overrides), rgb(0x123456));
    }

    #[test]
    fn bold_brightens_and_inverse_swaps() {
        let palette = Palette::default();
        let overrides = Colors::default();
        let red = Color::Named(NamedColor::Red);
        let background = Color::Named(NamedColor::Background);

        let bold = palette.cell_colors(&cell(red, background, Flags::BOLD), &overrides);
        assert_eq!(bold.foreground, palette.ansi[9]);

        let inverse = palette.cell_colors(&cell(red, background, Flags::INVERSE), &overrides);
        assert_eq!(inverse.foreground, palette.background);
        assert_eq!(inverse.background, palette.ansi[1]);
    }

    #[test]
    fn hidden_text_matches_background() {
        let palette = Palette::default();
        let colors = palette.cell_colors(
            &cell(
                Color::Named(NamedColor::Green),
                Color::Named(NamedColor::Background),
                Flags::HIDDEN,
            ),
            &Colors::default(),
        );
        assert_eq!(colors.foreground, colors.background);
    }

    #[test]
    fn light_theme_recolors_explicit_dark_panels_and_their_text() {
        let palette = Palette::light();
        let dark_green = rgb(0x20362a);
        let white = rgb(0xf3f3f3);
        let colors = palette.cell_colors(
            &cell(Color::Spec(white), Color::Spec(dark_green), Flags::empty()),
            &Colors::default(),
        );
        assert!(brightness(colors.background) > 200);
        assert_eq!(colors.foreground, palette.foreground);
        assert_ne!(colors.background, palette.background);

        let muted = palette.cell_colors(
            &cell(
                Color::Spec(rgb(0x9aa3a0)),
                Color::Spec(dark_green),
                Flags::empty(),
            ),
            &Colors::default(),
        );
        assert!(brightness(muted.foreground) > brightness(palette.foreground));
        assert!(brightness(muted.foreground) < brightness(muted.background));
    }

    #[test]
    fn dark_theme_and_light_panels_keep_explicit_colors() {
        let foreground = rgb(0xf3f3f3);
        let background = rgb(0x20362a);
        let dark = Palette::default().cell_colors(
            &cell(
                Color::Spec(foreground),
                Color::Spec(background),
                Flags::empty(),
            ),
            &Colors::default(),
        );
        assert_eq!(dark.foreground, foreground);
        assert_eq!(dark.background, background);

        let light_background = rgb(0xd6eeee);
        let light = Palette::light().cell_colors(
            &cell(
                Color::Spec(background),
                Color::Spec(light_background),
                Flags::empty(),
            ),
            &Colors::default(),
        );
        assert_eq!(light.foreground, background);
        assert_eq!(light.background, light_background);
    }

    #[test]
    fn hidden_and_inverse_cells_preserve_their_meaning() {
        let dark_green = Color::Spec(rgb(0x20362a));
        let white = Color::Spec(rgb(0xf3f3f3));
        let palette = Palette::light();
        let hidden =
            palette.cell_colors(&cell(white, dark_green, Flags::HIDDEN), &Colors::default());
        assert_eq!(hidden.foreground, hidden.background);

        let inverse =
            palette.cell_colors(&cell(white, dark_green, Flags::INVERSE), &Colors::default());
        assert_eq!(inverse.foreground, rgb(0x20362a));
        assert_eq!(inverse.background, rgb(0xf3f3f3));
    }
}
