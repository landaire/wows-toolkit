//! Where the viewer's characters sit across a line, and the indent guides drawn under them.

use zgui::app::LineRequest;
use zgui::app::Shaper;
use zgui::custom::ShapedRunOwned;
use zgui::view::Ident;

use crate::tabs::unpacker::viewer::CodeLine;
use crate::tabs::unpacker::viewer::Token;

/// The code faces, tried in order. The text names the same list, with `monospace` last, so the
/// shaped line matches the drawn one. A shaped line cannot resolve generic names.
pub const CODE_FAMILIES: [&str; 10] = [
    "Cascadia Mono",
    "Consolas",
    "SF Mono",
    "Menlo",
    "JetBrains Mono",
    "Adwaita Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Courier New",
];

/// The code's size in CSS pixels.
pub const CODE_SIZE: f32 = 12.0;

/// How many spaces a tab is drawn as, and how many columns one indent level is.
pub const TAB_SIZE: usize = 2;

/// The `font-family` value for [`CODE_FAMILIES`].
pub fn code_font_family() -> String {
    let mut names: Vec<String> = CODE_FAMILIES.iter().map(|name| format!("\"{name}\"")).collect();
    names.push("monospace".to_owned());

    names.join(", ")
}

/// Shapes the viewer's lines in its code face.
pub struct LineShaper {
    shaper: Shaper,
    families: Vec<Ident>,
}

impl LineShaper {
    pub fn new(shaper: Shaper) -> Self {
        Self { shaper, families: CODE_FAMILIES.iter().copied().map(Ident::new).collect() }
    }

    /// The x of every boundary between the characters of `line`, in CSS pixels from the line's
    /// start: one more than the line has characters.
    pub fn stops(&mut self, line: &CodeLine) -> Vec<f32> {
        let mut shaped = String::new();
        let mut starts = Vec::new();
        for ch in line.iter().flat_map(|token| token.text.chars()) {
            starts.push(shaped.len());
            if ch == '\t' {
                shaped.extend(std::iter::repeat_n(' ', TAB_SIZE));
            } else {
                shaped.push(ch);
            }
        }
        starts.push(shaped.len());

        // A shaped line reports where each glyph starts but not where the line ends, so one more
        // glyph is shaped after the text and its start taken as the text's end.
        shaped.push('0');
        let request = LineRequest {
            families: &self.families,
            weight: 400,
            italic: false,
            size_device_px: CODE_SIZE,
            letter_spacing: 0.0,
            ligatures: true,
        };
        let runs = self.shaper.shape_line(&shaped, &request);

        stops_from_runs(&runs, shaped.len(), &starts)
    }

    /// How wide one space is, in CSS pixels.
    pub fn space_width(&mut self) -> f32 {
        let stops = self.stops(&vec![Token { class: None, text: " ".to_owned() }]);

        stops[1] - stops[0]
    }
}

/// The x of each byte in `starts`, read from where the glyphs of `runs` start. A byte no glyph
/// starts at, such as one inside a ligature, is placed between its neighbours.
fn stops_from_runs(runs: &[ShapedRunOwned], shaped_len: usize, starts: &[usize]) -> Vec<f32> {
    let mut at_byte: Vec<Option<f32>> = vec![None; shaped_len + 1];
    for run in runs {
        for (glyph, cluster) in run.glyphs.iter().zip(&run.clusters) {
            if let Some(slot) = at_byte.get_mut(*cluster as usize) {
                *slot = Some(slot.map_or(glyph.x, |x: f32| x.min(glyph.x)));
            }
        }
    }

    let mut stops: Vec<Option<f32>> = starts.iter().map(|byte| at_byte[*byte]).collect();
    if let Some(first) = stops.first_mut() {
        first.get_or_insert(0.0);
    }
    let mut known = 0;
    for at in 1..stops.len() {
        if stops[at].is_none() {
            continue;
        }
        fill_between(&mut stops, known, at);
        known = at;
    }
    let last = stops.len() - 1;
    if known < last {
        // Nothing was shaped after `known`: the characters after it take no room.
        let x = stops[known];
        stops[known + 1..].fill(x);
    }

    stops.into_iter().map(|x| x.unwrap_or(0.0)).collect()
}

/// Spreads the unknown stops between the known ones at `from` and `to` evenly.
fn fill_between(stops: &mut [Option<f32>], from: usize, to: usize) {
    let (Some(start), Some(end)) = (stops[from], stops[to]) else { return };
    let gaps = (to - from) as f32;
    for (step, stop) in stops[from + 1..to].iter_mut().enumerate() {
        *stop = Some(start + (end - start) * (step + 1) as f32 / gaps);
    }
}

/// The character boundary in `stops` nearest `x`.
pub fn column_at(stops: &[f32], x: f32) -> usize {
    let after = stops.partition_point(|stop| *stop < x);
    if after == 0 {
        return 0;
    }
    if after == stops.len() {
        return stops.len() - 1;
    }

    if x - stops[after - 1] < stops[after] - x { after - 1 } else { after }
}

/// How many columns of indent each line has. A tab is [`TAB_SIZE`] columns. A line of only white
/// space takes the indent of the line before it, so guides run through blank lines.
pub fn indents(lines: &[CodeLine]) -> Vec<usize> {
    let mut out = Vec::with_capacity(lines.len());
    let mut previous = 0;
    for line in lines {
        let mut columns = 0;
        let mut blank = true;
        for ch in line.iter().flat_map(|token| token.text.chars()) {
            match ch {
                ' ' => columns += 1,
                '\t' => columns += TAB_SIZE,
                _ => {
                    blank = false;
                    break;
                }
            }
        }
        if !blank {
            previous = columns;
        }
        out.push(previous);
    }

    out
}

/// The columns indent guides are drawn at for `indent` columns of indent: one per full level.
pub fn guide_columns(indent: usize) -> impl Iterator<Item = usize> {
    (0..indent / TAB_SIZE).map(|level| level * TAB_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::unpacker::viewer::plain_lines;

    #[test]
    fn a_click_lands_on_the_nearest_boundary() {
        let stops = [0.0, 7.0, 21.0, 28.0];
        assert_eq!(column_at(&stops, -3.0), 0);
        assert_eq!(column_at(&stops, 3.0), 0);
        assert_eq!(column_at(&stops, 4.0), 1);
        assert_eq!(column_at(&stops, 15.0), 2, "a wide character's far half goes after it");
        assert_eq!(column_at(&stops, 99.0), 3);
    }

    #[test]
    fn indents_count_tabs_and_carry_over_blank_lines() {
        let lines = plain_lines("a\n    b\n\n  \n\tc\nd");
        assert_eq!(indents(&lines), vec![0, 4, 4, 4, 2, 0]);
        assert_eq!(guide_columns(4).collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(guide_columns(3).collect::<Vec<_>>(), vec![0]);
    }
}
