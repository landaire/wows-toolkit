//! Drawing of a player's damage mix: the stacked bar under a damage figure,
//! its tooltip listing, and the colour key of each kind.

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use wows_toolkit_replay_data::damage_mix::DamageKind;
use wows_toolkit_replay_data::damage_mix::DamageLine;
use wows_toolkit_replay_data::damage_mix::DamageMix;
use wows_toolkit_replay_data::model::separate_number;

/// Height of the stacked bar under a damage figure.
const BAR_HEIGHT: Pixels = px(4.);

/// Side of the colour key square beside a listed type.
pub const SWATCH_SIZE: f32 = 8.0;

/// The kind's colour, the same wherever the kind is drawn. A light theme
/// darkens every colour by one factor so a thin bar stands off the panel.
pub fn color(kind: DamageKind) -> Hsla {
    let [r, g, b] = kind.rgb();
    let color: Hsla = rgb(u32::from_be_bytes([0, r, g, b])).into();
    if crate::theme::is_dark_mode() { color } else { Hsla { l: color.l * 0.72, ..color } }
}

/// The segments across the whole width given, in full colour under the
/// figure: the types stay recognisable and the figure never sits on a
/// coloured ground, so it reads the same on every panel and in every
/// player colour.
pub fn stacked_bar(mix: &DamageMix) -> impl IntoElement {
    let last = mix.segments().len() - 1;
    h_flex().w_full().h(BAR_HEIGHT).flex_none().overflow_hidden().children(mix.proportions().enumerate().map(
        |(index, (kind, proportion))| {
            let piece = div().id(("damage-segment", index)).test_support().h_full().bg(color(kind));
            // The last takes what is left, so rounding leaves no gap at the end.
            if index == last { piece.flex_1() } else { piece.flex_none().w(relative(proportion)) }
        },
    ))
}

/// Every line with its colour key, amount and share of the total, for a
/// tooltip.
pub fn listing(mix: &DamageMix, lines: &[DamageLine]) -> AnyElement {
    let column = || v_flex().gap_px();
    let mut keys = column();
    let mut amounts = column().items_end();
    let mut percents = column().items_end();
    for line in lines {
        keys = keys.child(h_flex().gap_1().child(swatch(line.kind)).child(line.label));
        amounts = amounts.child(separate_number(line.amount));
        percents = percents.child(format!("{:.0}%", mix.percent(line.amount)));
    }

    h_flex().gap_2().items_start().text_xs().child(keys).child(amounts).child(percents).into_any_element()
}

/// The colour key square for `kind`.
pub fn swatch(kind: DamageKind) -> impl IntoElement {
    div().flex_none().size(px(SWATCH_SIZE)).bg(color(kind))
}

#[cfg(test)]
mod tests {
    use wows_toolkit_replay_data::damage_mix::DamageKind;
    use wows_toolkit_replay_data::damage_mix::DamageLine;
    use wows_toolkit_replay_data::damage_mix::DamageMix;

    fn line(kind: DamageKind, amount: u64) -> DamageLine {
        DamageLine { kind, label: "", amount }
    }

    /// The stacked bar spans the width it is given and splits it in each
    /// type's proportion of the player's damage.
    #[gpui_kit::test]
    fn a_stack_splits_its_width_in_each_types_proportion(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use gpui_kit::ParentElement as _;
        use gpui_kit::Styled as _;
        use gpui_kit::test::TestWindowExt as _;

        struct Bar(DamageMix);

        impl gpui_kit::Render for Bar {
            fn render(
                &mut self,
                _window: &mut gpui_kit::Window,
                _cx: &mut gpui_kit::Context<Self>,
            ) -> impl gpui_kit::IntoElement {
                gpui_kit::div().w(gpui_kit::px(500.)).child(super::stacked_bar(&self.0))
            }
        }

        let lines = [line(DamageKind::Ap, 30_000), line(DamageKind::He, 10_000), line(DamageKind::Fire, 10_000)];
        let mix = DamageMix::of(&lines).expect("damage was dealt");
        cx.update(gpui_kit::init);
        let window = cx.open_window(gpui_kit::size(gpui_kit::px(600.), gpui_kit::px(100.)), |_window, _cx| Bar(mix));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let widths =
                [0usize, 1, 2].map(|index| window.find(("damage-segment", index)).bounds().size.width.as_f32());
            for (width, expected) in widths.iter().zip([300., 100., 100.]) {
                assert!((width - expected).abs() < 0.5, "{widths:?}");
            }
        })
        .expect("the window is open");
    }
}
