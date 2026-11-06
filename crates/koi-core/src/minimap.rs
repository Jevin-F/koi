//! The shape of a file at a glance, for the minimap beside the editor, and
//! how the minimap scrolls along with the editor.

/// Columns beyond this are left out of the minimap.
pub const MAX_COLUMNS: usize = 120;
const TAB_WIDTH: usize = 4;

/// A run of non-blank characters, in columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub start: u16,
    pub len: u16,
}

/// The non-blank runs of each line of `text`, tabs expanded.
pub fn line_runs(text: &str) -> Vec<Box<[Run]>> {
    text.lines()
        .map(|line| {
            let mut runs: Vec<Run> = Vec::new();
            let mut column = 0;
            for ch in line.chars() {
                if column >= MAX_COLUMNS {
                    break;
                }
                if ch == '\t' {
                    column += TAB_WIDTH - column % TAB_WIDTH;
                    continue;
                }
                if !ch.is_whitespace() {
                    match runs.last_mut() {
                        Some(run) if (run.start + run.len) as usize == column => run.len += 1,
                        _ => runs.push(Run {
                            start: column as u16,
                            len: 1,
                        }),
                    }
                }
                column += 1;
            }
            runs.into_boxed_slice()
        })
        .collect()
}

/// Where the minimap stands relative to the editor, measured in lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinimapGeometry {
    /// Lines in the file.
    pub lines: f32,
    /// Lines the editor shows at once.
    pub visible: f32,
    /// The line at the top of the editor, fractional while between lines.
    pub top: f32,
    /// Lines the minimap has room for.
    pub capacity: f32,
}

impl MinimapGeometry {
    /// The editor's furthest scroll position, as its top line.
    fn max_top(&self) -> f32 {
        (self.lines - self.visible).max(0.)
    }

    /// The line at the top of the minimap. A file taller than the minimap
    /// scrolls through it in proportion to the editor.
    pub fn first_line(&self) -> f32 {
        let overflow = self.lines - self.capacity;
        if overflow <= 0. || self.max_top() <= 0. {
            return 0.;
        }
        (self.top / self.max_top()).clamp(0., 1.) * overflow
    }

    /// The editor top line that centers `line` in the editor.
    pub fn top_centering(&self, line: f32) -> f32 {
        (line - self.visible / 2.).clamp(0., self.max_top())
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_COLUMNS, MinimapGeometry, Run, line_runs};

    fn run(start: u16, len: u16) -> Run {
        Run { start, len }
    }

    #[test]
    fn lines_become_runs_of_non_blank_columns() {
        let runs = line_runs("fn main() {\n\tlet x = 1;\n\n");
        assert_eq!(runs.len(), 3);
        assert_eq!(*runs[0], [run(0, 2), run(3, 6), run(10, 1)]);
        assert_eq!(*runs[1], [run(4, 3), run(8, 1), run(10, 1), run(12, 2)]);
        assert!(runs[2].is_empty());
    }

    #[test]
    fn long_lines_are_cut_at_the_last_column() {
        let runs = line_runs(&"x".repeat(MAX_COLUMNS * 2));
        assert_eq!(*runs[0], [run(0, MAX_COLUMNS as u16)]);
    }

    #[test]
    fn a_short_file_fits_without_scrolling() {
        let geometry = MinimapGeometry {
            lines: 100.,
            visible: 40.,
            top: 60.,
            capacity: 300.,
        };
        assert_eq!(geometry.first_line(), 0.);
    }

    #[test]
    fn a_tall_file_scrolls_the_minimap_in_proportion() {
        let mut geometry = MinimapGeometry {
            lines: 1040.,
            visible: 40.,
            top: 0.,
            capacity: 240.,
        };
        assert_eq!(geometry.first_line(), 0.);
        geometry.top = 500.;
        assert_eq!(geometry.first_line(), 400.);
        geometry.top = 1000.;
        assert_eq!(geometry.first_line(), 800.);
    }

    #[test]
    fn centering_a_line_stays_within_the_file() {
        let geometry = MinimapGeometry {
            lines: 1000.,
            visible: 40.,
            top: 0.,
            capacity: 240.,
        };
        assert_eq!(geometry.top_centering(500.), 480.);
        assert_eq!(geometry.top_centering(5.), 0.);
        assert_eq!(geometry.top_centering(999.), 960.);
    }
}
