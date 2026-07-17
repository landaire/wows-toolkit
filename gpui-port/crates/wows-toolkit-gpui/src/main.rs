use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;

struct Shell;

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui_component::v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child("wows-toolkit-gpui")
    }
}

fn main() {
    let app = gpui_platform::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_component::init(cx);
        gpui_tokio::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(200.), px(120.)),
                size: size(px(1200.), px(800.)),
            })),
            window_min_size: Some(size(px(640.), px(480.))),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|_cx| Shell);
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
