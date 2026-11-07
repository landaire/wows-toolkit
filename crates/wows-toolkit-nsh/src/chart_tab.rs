use std::rc::Rc;

use nsh_base::plot::AxisPlacement;
use nsh_base::plot::Curve;
use nsh_base::plot::line;
use nsh_base::plot::plot;
use nsh_base::plot::plot_axis;
use nsh_core::element::RowAccessor;
use nsh_core::element::Rows;
use nsh_core::prelude::*;

const POINTS: usize = 500;
const SERIES: [(&str, u32); 3] = [("alpha", 0x4e9af1), ("beta", 0xf1a24e), ("gamma", 0x6fd36f)];

/// Three series of [`POINTS`] values each.
struct ChartData {
    series: [Vec<f64>; 3],
}

impl ChartData {
    fn new() -> Self {
        let wave = |phase: f64, scale: f64, offset: f64| {
            (0..POINTS)
                .map(|i| {
                    let t = i as f64 / 25.0;
                    offset + scale * (t + phase).sin() + 0.3 * scale * (t * 3.7 + phase).cos()
                })
                .collect::<Vec<f64>>()
        };

        Self { series: [wave(0.0, 40.0, 100.0), wave(1.3, 25.0, 60.0), wave(2.6, 15.0, 20.0)] }
    }

    fn rows(&self) -> Rows {
        Rows::from_columns(
            POINTS,
            SERIES.iter().zip(&self.series).map(|((name, _), values)| ((*name).into(), values.clone())),
        )
    }
}

pub fn chart_tab() -> impl IntoElement {
    let data = Rc::new(ChartData::new());
    let hovered = signal(None::<u32>);
    let label_data = Rc::clone(&data);

    let mut chart = plot()
        .data(data.rows())
        .y(RowAccessor::key(SERIES[0].0))
        .accessibility_label("Series chart")
        .flex_1()
        .ml(px(48.0))
        .mb(px(28.0))
        .mr(px(16.0))
        .mt(px(8.0))
        .on_hover_change(move |index: &Option<u32>| hovered.set(*index))
        .child(plot_axis().placement(AxisPlacement::Left).ticks(6u32))
        .child(plot_axis().placement(AxisPlacement::Bottom).ticks(10u32));

    for (name, color) in SERIES {
        chart = chart.child(
            line().y(RowAccessor::key(name)).curve(Curve::Linear).stroke_color(rgb(color)).stroke_width(px(1.5)),
        );
    }

    view()
        .flex_1()
        .gap(px(8.0))
        .p(px(8.0))
        .child(text(move || match hovered.get() {
            Some(index) => {
                let values = label_data.series.iter().map(|series| format!("{:.1}", series[index as usize]));
                format!("Hover index: {index} [{}]", values.collect::<Vec<_>>().join(", "))
            }
            None => "Hover index: none".to_owned(),
        }))
        .child(view().flex_1().child(chart))
}
