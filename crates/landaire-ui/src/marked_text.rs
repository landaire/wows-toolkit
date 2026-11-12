use std::ops::Range;
use std::rc::Rc;

use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

const SHEET: &str = "landaire-ui-marked-text";

/// The class on each marked piece of a [`MarkedText`].
pub const MARK_CLASS: &str = "lui-mark";

const CSS: &str = css!(
    ".lui-mark {
        font-weight: 700;
        background-color: color-mix(in srgb, var(--lui-color-accent) 30%, transparent);
    }"
);

/// A run of text with some of it marked, such as the letters a filter matched.
///
/// `ranges` are byte ranges of `text` on character boundaries, ascending and apart. A range
/// outside the text or off a boundary is dropped rather than drawn.
#[component]
pub fn MarkedText(
    #[prop(into)] text: String,
    ranges: Rc<[Range<usize>]>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let pieces = pieces(&text, &ranges)
        .into_iter()
        .map(|(range, marked)| {
            let piece = text[range].to_owned();
            if marked {
                AnyView::new(view! { text(class = MARK_CLASS) {{piece}} })
            } else {
                AnyView::new(view! { text {{piece}} })
            }
        })
        .collect::<Vec<AnyView>>();

    view! {
        text({..attrs}, class = class) {{pieces}}
    }
}

/// `text` cut at the edges of `ranges`, each piece with whether it is marked.
fn pieces(text: &str, ranges: &[Range<usize>]) -> Vec<(Range<usize>, bool)> {
    let mut pieces = Vec::with_capacity(ranges.len() * 2 + 1);
    let mut at = 0;
    for range in ranges {
        let fits = at <= range.start
            && range.start < range.end
            && range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end);
        if !fits {
            continue;
        }
        if at < range.start {
            pieces.push((at..range.start, false));
        }
        pieces.push((range.clone(), true));
        at = range.end;
    }
    if at < text.len() {
        pieces.push((at..text.len(), false));
    }

    pieces
}

#[cfg(test)]
mod tests {
    use super::pieces;

    #[test]
    fn marks_split_the_text_into_pieces() {
        assert_eq!(pieces("Yamato", &[0..1, 2..4]), [(0..1, true), (1..2, false), (2..4, true), (4..6, false)]);
        assert_eq!(pieces("Yamato", &[]), [(0..6, false)]);
        assert_eq!(pieces("Yamato", std::slice::from_ref(&(0..6))), [(0..6, true)]);
    }

    #[test]
    fn ranges_that_do_not_fit_are_dropped() {
        // "u" with an umlaut is two bytes; 2..3 starts inside it.
        assert_eq!(pieces("N\u{fc}rn", &[2..3, 3..4, 1..2, 9..10]), [(0..3, false), (3..4, true), (4..5, false)]);
    }
}
