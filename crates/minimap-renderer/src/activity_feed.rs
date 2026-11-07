//! Layout of the stats panel activity feed, shared by the pixmap and egui
//! backends so both wrap, stack, and animate rows identically. Backends
//! supply text measurement; all lengths are in the backend's pixel units,
//! which for the constants below are canvas pixels before any window scale.

use std::borrow::Cow;

use crate::draw_command::ActivityFeedKind;
use crate::draw_command::KillFeedEntry;

/// Seconds of displayed time a row takes to ease into place when a newer
/// entry pushes it.
pub const REFLOW_SECONDS: f32 = 0.2;

/// Horizontal space between the panel edge and row text.
pub const SIDE_PADDING: f32 = 8.0;
/// Vertical space between the panel edge and the first or last row.
pub const AREA_INSET: f32 = 4.0;
/// Horizontal space between the panel edge and a row's background.
pub const BAND_INSET: f32 = 4.0;
/// Space above and below a row's lines, inside its background.
pub const ROW_PADDING: f32 = 2.0;
pub const NAME_LINE_HEIGHT: f32 = 19.0;
pub const MESSAGE_LINE_HEIGHT: f32 = 17.0;
pub const NAME_FONT_SIZE: f32 = 14.0;
pub const MESSAGE_FONT_SIZE: f32 = 13.0;
pub const ICON_SIZE: f32 = 16.0;
/// Space between adjacent runs on a line.
pub const ITEM_GAP: f32 = 3.0;
pub const KILL_MARKER: &str = "|";
pub const KILL_MARKER_RGB: [u8; 3] = [140, 140, 140];
/// Fill behind [`crate::draw_command::RowShade::Banded`] rows.
pub const BANDED_ROW_RGB: [u8; 3] = [70, 80, 100];
pub const BANDED_ROW_ALPHA: f32 = 0.35;

/// Game seconds that pass per second of displayed time.
///
/// Video export and playback speed-ups run the game clock faster than the
/// viewer's, and the reflow animation is paced in the viewer's time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackRate(f32);

impl PlaybackRate {
    pub const REAL_TIME: Self = Self(1.0);

    /// `None` unless the rate is finite and positive.
    pub const fn new(game_seconds_per_second: f32) -> Option<Self> {
        if game_seconds_per_second.is_finite() && game_seconds_per_second > 0.0 {
            Some(Self(game_seconds_per_second))
        } else {
            None
        }
    }

    pub fn game_seconds_per_second(self) -> f32 {
        self.0
    }
}

/// How far an entry `age_seconds` of game time old has eased into place,
/// from 0 (just arrived) to 1 (settled).
pub fn reveal(age_seconds: f32, rate: PlaybackRate) -> f32 {
    let span = REFLOW_SECONDS * rate.0;
    let progress = (age_seconds / span).clamp(0.0, 1.0);

    // Cubic ease-out: fast start, gentle stop.
    1.0 - (1.0 - progress).powi(3)
}

/// Which neighbour an icon must share a line with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconBind {
    Previous,
    Next,
}

/// One run of a feed row, in reading order.
#[derive(Debug, Clone)]
pub enum FlowItem<'a, S, I> {
    /// Text that may break between words, or between characters when one
    /// word is wider than the line.
    Text {
        text: Cow<'a, str>,
        style: S,
    },
    Icon {
        icon: I,
        width: f32,
        bind: IconBind,
    },
}

/// Placed text borrows from its item's text when that text is borrowed, so a
/// layout can outlive the items it was made from.
#[derive(Debug, Clone, PartialEq)]
pub enum PlacedContent<'a, S, I> {
    Text { text: Cow<'a, str>, style: S },
    Icon { icon: I },
}

/// A run placed at `x` from the start of line `line`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedPiece<'a, S, I> {
    pub line: usize,
    pub x: f32,
    pub content: PlacedContent<'a, S, I>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlowLayout<'a, S, I> {
    pub pieces: Vec<PlacedPiece<'a, S, I>>,
    /// Number of lines used; zero when there was nothing to place.
    pub lines: usize,
}

/// Height and animation progress of one feed row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowExtent {
    pub height: f32,
    /// See [`reveal`].
    pub reveal: f32,
}

/// A visible feed row, `top` measured from the top of the feed area. Rows
/// may extend past either edge while animating; backends clip to the area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StackedRow {
    pub index: usize,
    pub top: f32,
    pub opacity: f32,
}

/// An icon in a feed row's name line.
#[derive(Debug, Clone, Copy)]
pub enum FeedIcon<'e> {
    KillShip { species: &'e str, color: [u8; 3], friendly: bool },
    ChatShip { species: &'e str, color: [u8; 3] },
    DeathCause(&'e KillFeedEntry),
}

#[derive(Debug, Clone, Copy)]
enum AtomKind {
    /// Byte range of a word and its trailing spaces within the item text.
    Text {
        start: usize,
        end: usize,
    },
    Icon,
}

#[derive(Debug, Clone, Copy)]
struct Atom {
    item: usize,
    kind: AtomKind,
    width: f32,
    /// Width without trailing spaces, which hang past a line end.
    trimmed: f32,
}

#[derive(Debug, Clone, Copy)]
struct Draft {
    line: usize,
    x: f32,
    item: usize,
    kind: AtomKind,
}

struct Placer<'i, 'a, S, I, M> {
    items: &'i [FlowItem<'a, S, I>],
    max_width: f32,
    gap: f32,
    measure: M,

    line: usize,
    x: f32,
    line_item: Option<usize>,
    drafts: Vec<Draft>,
}

impl<'i, S, I, M> Placer<'i, '_, S, I, M>
where
    S: Copy,
    M: FnMut(S, &str) -> f32,
{
    fn item_text(&self, item: usize) -> Option<(&'i str, S)> {
        let items: &'i [FlowItem<'_, S, I>] = self.items;
        match &items[item] {
            FlowItem::Text { text, style } => Some((text.as_ref(), *style)),
            FlowItem::Icon { .. } => None,
        }
    }

    fn lead_gap(&self, item: usize) -> f32 {
        match self.line_item {
            Some(previous) if previous != item => self.gap,
            _ => 0.0,
        }
    }

    fn newline(&mut self) {
        self.line += 1;
        self.x = 0.0;
        self.line_item = None;
    }

    fn put(&mut self, item: usize, kind: AtomKind, width: f32) {
        let lead = self.lead_gap(item);
        self.x += lead;

        // Words of one text run on one line draw as a single string.
        let merged = match (self.drafts.last_mut(), kind) {
            (Some(last), AtomKind::Text { start, end })
                if last.line == self.line && last.item == item && self.line_item == Some(item) =>
            {
                match &mut last.kind {
                    AtomKind::Text { end: last_end, .. } if *last_end == start => {
                        *last_end = end;
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        if !merged {
            self.drafts.push(Draft { line: self.line, x: self.x, item, kind });
        }

        self.x += width;
        self.line_item = Some(item);
    }

    fn unit_width(&self, unit: &[Atom]) -> f32 {
        let mut width = 0.0;
        for (idx, atom) in unit.iter().enumerate() {
            if idx > 0 && unit[idx - 1].item != atom.item {
                width += self.gap;
            }
            width += if idx + 1 == unit.len() { atom.trimmed } else { atom.width };
        }

        width
    }

    fn place_unit(&mut self, unit: &[Atom]) {
        let Some(first) = unit.first() else { return };

        let width = self.unit_width(unit);
        if self.x > 0.0 && self.x + self.lead_gap(first.item) + width > self.max_width {
            self.newline();
        }

        if self.x + self.lead_gap(first.item) + width <= self.max_width {
            for atom in unit {
                self.put(atom.item, atom.kind, atom.width);
            }
            return;
        }

        // Wider than a whole line: give up the binding and break inside it.
        for atom in unit {
            self.place_atom(*atom);
        }
    }

    fn place_atom(&mut self, atom: Atom) {
        if self.x > 0.0 && self.x + self.lead_gap(atom.item) + atom.trimmed > self.max_width {
            self.newline();
        }

        let fits = self.x + self.lead_gap(atom.item) + atom.trimmed <= self.max_width;
        match atom.kind {
            AtomKind::Text { start, end } if !fits => self.break_word(atom.item, start, end),
            _ => self.put(atom.item, atom.kind, atom.width),
        }
    }

    /// Splits a word wider than the line between characters.
    fn break_word(&mut self, item: usize, mut start: usize, end: usize) {
        let Some((text, style)) = self.item_text(item) else { return };

        while start < end {
            let rest = &text[start..end];
            let avail = self.max_width - self.x - self.lead_gap(item);
            let rest_trimmed = (self.measure)(style, rest.trim_end());
            if rest_trimmed <= avail {
                let width = (self.measure)(style, rest);
                self.put(item, AtomKind::Text { start, end }, width);
                return;
            }

            let boundaries: Vec<usize> = rest.char_indices().skip(1).map(|(idx, _)| idx).chain([rest.len()]).collect();
            let mut low = 0usize;
            let mut high = boundaries.len();
            while low < high {
                let mid = (low + high) / 2;
                if (self.measure)(style, rest[..boundaries[mid]].trim_end()) <= avail {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }

            let take = match low {
                0 if self.x > 0.0 => {
                    self.newline();
                    continue;
                }
                // Not even one character fits an empty line; place it anyway.
                0 => boundaries[0],
                count => boundaries[count - 1],
            };
            let width = (self.measure)(style, &rest[..take]);
            self.put(item, AtomKind::Text { start, end: start + take }, width);
            start += take;
            if start < end {
                self.newline();
            }
        }
    }
}

/// Breaks `items` into lines no wider than `max_width`, with `gap` between
/// adjacent items on a line. `measure` returns the advance width of a string.
pub fn flow<'a, S, I, M>(items: &[FlowItem<'a, S, I>], max_width: f32, gap: f32, mut measure: M) -> FlowLayout<'a, S, I>
where
    S: Copy,
    I: Copy,
    M: FnMut(S, &str) -> f32,
{
    let mut atoms: Vec<Atom> = Vec::new();
    for (item, flow_item) in items.iter().enumerate() {
        match flow_item {
            FlowItem::Text { text, style } => {
                let (text, style) = (text.as_ref(), *style);
                for (start, end) in word_spans(text) {
                    let word = &text[start..end];
                    let width = measure(style, word);
                    let trimmed = measure(style, word.trim_end());
                    atoms.push(Atom { item, kind: AtomKind::Text { start, end }, width, trimmed });
                }
            }
            &FlowItem::Icon { width, .. } => {
                atoms.push(Atom { item, kind: AtomKind::Icon, width, trimmed: width });
            }
        }
    }

    let mut no_break_after = vec![false; atoms.len()];
    for (idx, atom) in atoms.iter().enumerate() {
        let FlowItem::Icon { bind, .. } = items[atom.item] else { continue };
        match bind {
            IconBind::Next if idx + 1 < atoms.len() => no_break_after[idx] = true,
            IconBind::Previous if idx > 0 => no_break_after[idx - 1] = true,
            // Nothing on that side to bind to.
            _ => {}
        }
    }

    let mut placer = Placer { items, max_width, gap, measure, line: 0, x: 0.0, line_item: None, drafts: Vec::new() };
    let mut unit_start = 0;
    for idx in 0..atoms.len() {
        if !no_break_after[idx] || idx + 1 == atoms.len() {
            placer.place_unit(&atoms[unit_start..=idx]);
            unit_start = idx + 1;
        }
    }

    let lines = placer.drafts.last().map_or(0, |draft| draft.line + 1);
    let pieces = placer
        .drafts
        .iter()
        .map(|draft| {
            let content = match (&items[draft.item], draft.kind) {
                (FlowItem::Text { text, style }, AtomKind::Text { start, end }) => {
                    let text = match text {
                        Cow::Borrowed(text) => Cow::Borrowed(&text[start..end]),
                        Cow::Owned(text) => Cow::Owned(text[start..end].to_owned()),
                    };
                    PlacedContent::Text { text, style: *style }
                }
                (FlowItem::Icon { icon, .. }, _) => PlacedContent::Icon { icon: *icon },
                (FlowItem::Text { .. }, AtomKind::Icon) => unreachable!("text items only yield text atoms"),
            };
            PlacedPiece { line: draft.line, x: draft.x, content }
        })
        .collect();

    FlowLayout { pieces, lines }
}

/// The runs of an entry's name line, in reading order: who killed whom, or
/// who spoke. `style` turns a run's text and color into the backend's text
/// style; icons `drawable` rejects are left out.
pub fn header_items<'e, S>(
    kind: &'e ActivityFeedKind,
    icon_width: f32,
    mut style: impl FnMut(&str, [u8; 3]) -> S,
    mut drawable: impl FnMut(&FeedIcon<'e>) -> bool,
) -> Vec<FlowItem<'e, S, FeedIcon<'e>>> {
    let mut items = Vec::new();
    let mut text = |items: &mut Vec<FlowItem<'e, S, FeedIcon<'e>>>, text: Cow<'e, str>, color: [u8; 3]| {
        let style = style(&text, color);
        items.push(FlowItem::Text { text, style });
    };
    let mut icon = |items: &mut Vec<FlowItem<'e, S, FeedIcon<'e>>>, icon: FeedIcon<'e>, bind: IconBind| {
        if drawable(&icon) {
            items.push(FlowItem::Icon { icon, width: icon_width, bind });
        }
    };

    match kind {
        ActivityFeedKind::Kill(kill) => {
            text(&mut items, Cow::Borrowed(KILL_MARKER), KILL_MARKER_RGB);
            if let Some(name) = &kill.killer_name {
                text(&mut items, Cow::Borrowed(name), kill.killer_color);
            }
            if let Some(species) = &kill.killer_species {
                let ship = FeedIcon::KillShip { species, color: kill.killer_color, friendly: kill.killer_is_friendly };
                icon(&mut items, ship, IconBind::Previous);
            }
            icon(&mut items, FeedIcon::DeathCause(kill), IconBind::Next);
            text(&mut items, Cow::Borrowed(&kill.victim_name), kill.victim_color);
            if let Some(species) = &kill.victim_species {
                let ship = FeedIcon::KillShip { species, color: kill.victim_color, friendly: kill.victim_is_friendly };
                icon(&mut items, ship, IconBind::Previous);
            }
        }
        ActivityFeedKind::Chat(chat) => {
            if !chat.clan_tag.is_empty() {
                let label = Cow::Owned(format!("[{}]", chat.clan_tag));
                text(&mut items, label, chat.clan_color.unwrap_or(chat.team_color));
            }
            text(&mut items, Cow::Borrowed(&chat.player_name), chat.team_color);
            if let Some(species) = &chat.ship_species {
                // The icon belongs with the ship name it depicts when there is one.
                let bind = if chat.ship_name.is_some() { IconBind::Next } else { IconBind::Previous };
                icon(&mut items, FeedIcon::ChatShip { species, color: chat.team_color }, bind);
            }
            if let Some(ship_name) = &chat.ship_name {
                text(&mut items, Cow::Borrowed(ship_name), chat.team_color);
            }
        }
    }

    items
}

/// Height of a row with `name_lines` lines of names and, for chat,
/// `message_lines` lines of message. An empty message keeps one line.
pub fn row_height(name_lines: usize, message_lines: Option<usize>) -> f32 {
    let message = message_lines.map_or(0.0, |lines| lines.max(1) as f32 * MESSAGE_LINE_HEIGHT);

    2.0 * ROW_PADDING + name_lines.max(1) as f32 * NAME_LINE_HEIGHT + message
}

/// Byte ranges of each word in `text` together with the spaces after it.
/// Leading spaces join the first word.
fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    let mut seen_word = false;
    let mut in_trailing_space = false;
    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            in_trailing_space |= seen_word;
            continue;
        }
        if in_trailing_space {
            spans.push((start, idx));
            start = idx;
            in_trailing_space = false;
        }
        seen_word = true;
    }
    if start < text.len() {
        spans.push((start, text.len()));
    }

    spans
}

/// The rows of a feed of `count` entries visible in an area `area_height`
/// tall, oldest first.
///
/// While the feed is shorter than the area it hangs from the top. Once full
/// it rests on the bottom, showing only rows that fit whole. An arriving row
/// fades in and, once the feed is full, grows in from the bottom edge,
/// pushing the rest up; rows pushed past the top fade out as they go.
/// `extent` is asked for each row once, newest first, and only as far back as
/// can be seen.
pub fn stack_rows(count: usize, area_height: f32, mut extent: impl FnMut(usize) -> RowExtent) -> Vec<StackedRow> {
    struct Measured {
        index: usize,
        /// Height of this row and all newer ones.
        stacked: f32,
        height: f32,
        reveal: f32,
    }

    let mut measured: Vec<Measured> = Vec::new();
    let mut stacked = 0.0f32;
    let mut pending = 0.0f32;
    for index in (0..count).rev() {
        let RowExtent { height, reveal } = extent(index);
        let reveal = reveal.clamp(0.0, 1.0);
        stacked += height;
        pending += height * (1.0 - reveal);
        measured.push(Measured { index, stacked, height, reveal });

        // Entries are oldest first, so older rows are settled too and sit
        // wholly above this one.
        if reveal >= 1.0 && area_height - stacked + pending <= 0.0 {
            break;
        }
    }

    let shown = stacked - pending;
    let mut rows = Vec::with_capacity(measured.len());
    if shown < area_height {
        let mut top = 0.0;
        for row in measured.iter().rev() {
            if top >= area_height {
                break;
            }
            if row.reveal > 0.0 {
                rows.push(StackedRow { index: row.index, top, opacity: row.reveal });
            }
            top += row.height;
        }
        return rows;
    }

    for row in measured.iter().rev() {
        let settled = area_height - row.stacked;
        let top = settled + pending;
        if top >= area_height || top + row.height <= 0.0 {
            continue;
        }

        // A row that will not fit once settled fades out over the distance
        // it has left to travel.
        let leaving = if settled >= 0.0 { 1.0 } else { (pending / -settled).min(1.0) };
        let opacity = leaving.min(row.reveal);
        if opacity <= 0.0 {
            continue;
        }
        rows.push(StackedRow { index: row.index, top, opacity });
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ten units per character, so widths are easy to reason about.
    fn measure(_: (), text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    fn text(text: &str) -> FlowItem<'_, (), char> {
        FlowItem::Text { text: Cow::Borrowed(text), style: () }
    }

    fn lines_of(layout: &FlowLayout<'_, (), char>) -> Vec<String> {
        let mut lines = vec![String::new(); layout.lines];
        for piece in &layout.pieces {
            let line = &mut lines[piece.line];
            if !line.is_empty() {
                line.push('|');
            }
            match &piece.content {
                PlacedContent::Text { text, .. } => line.push_str(text),
                PlacedContent::Icon { icon } => line.push(*icon),
            }
        }
        lines
    }

    #[test]
    fn short_row_stays_on_one_line() {
        let items = [text("Name"), FlowItem::Icon { icon: '#', width: 16.0, bind: IconBind::Previous }];
        let layout = flow(&items, 200.0, 2.0, measure);

        assert_eq!(lines_of(&layout), ["Name|#"]);
        assert_eq!(layout.pieces[1].x, 42.0);
    }

    #[test]
    fn text_wraps_between_words() {
        let items = [text("aaa bbb ccc")];
        let layout = flow(&items, 75.0, 2.0, measure);

        assert_eq!(lines_of(&layout), ["aaa bbb ", "ccc"]);
    }

    #[test]
    fn icon_moves_to_the_next_line_with_its_text() {
        let items = [text("killer"), FlowItem::Icon { icon: '#', width: 16.0, bind: IconBind::Next }, text("ship")];
        let layout = flow(&items, 100.0, 2.0, measure);

        assert_eq!(lines_of(&layout), ["killer", "#|ship"]);
    }

    #[test]
    fn word_wider_than_line_breaks_between_characters() {
        let items = [FlowItem::Text { text: Cow::Owned("abcdefghij".to_owned()), style: () }];
        let layout = flow(&items, 40.0, 2.0, measure);
        drop(items);

        assert_eq!(lines_of(&layout), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn empty_flow_has_no_lines() {
        assert_eq!(flow(&[text("")], 40.0, 2.0, measure).lines, 0);
    }

    fn settled(height: f32) -> RowExtent {
        RowExtent { height, reveal: 1.0 }
    }

    #[test]
    fn short_feed_hangs_from_the_top() {
        let rows = stack_rows(2, 100.0, |_| settled(20.0));

        assert_eq!(
            rows,
            [StackedRow { index: 0, top: 0.0, opacity: 1.0 }, StackedRow { index: 1, top: 20.0, opacity: 1.0 }]
        );
    }

    #[test]
    fn full_feed_rests_on_the_bottom_and_drops_rows_that_do_not_fit() {
        let rows = stack_rows(10, 50.0, |_| settled(20.0));

        assert_eq!(
            rows,
            [StackedRow { index: 8, top: 10.0, opacity: 1.0 }, StackedRow { index: 9, top: 30.0, opacity: 1.0 }]
        );
    }

    #[test]
    fn arriving_row_pushes_the_feed_up_as_it_settles() {
        let three_quarters = |index| if index == 9 { RowExtent { height: 20.0, reveal: 0.75 } } else { settled(20.0) };
        let rows = stack_rows(10, 50.0, three_quarters);

        // Row 7 is on its way out: settled it would sit at -10, it is at -5.
        assert_eq!(
            rows,
            [
                StackedRow { index: 7, top: -5.0, opacity: 0.5 },
                StackedRow { index: 8, top: 15.0, opacity: 1.0 },
                StackedRow { index: 9, top: 35.0, opacity: 0.75 },
            ]
        );
    }

    #[test]
    fn only_visible_rows_are_measured() {
        let mut asked = Vec::new();
        stack_rows(100, 50.0, |index| {
            asked.push(index);
            settled(20.0)
        });

        assert_eq!(asked, [99, 98, 97]);
    }

    #[test]
    fn reveal_is_paced_in_displayed_time() {
        let fast = PlaybackRate::new(10.0).expect("a positive rate");

        assert_eq!(reveal(0.0, fast), 0.0);
        assert_eq!(reveal(REFLOW_SECONDS * 10.0, fast), 1.0);
        assert!(reveal(REFLOW_SECONDS, PlaybackRate::REAL_TIME) >= 1.0);
        assert!(reveal(REFLOW_SECONDS, fast) < 0.5);
    }
}
