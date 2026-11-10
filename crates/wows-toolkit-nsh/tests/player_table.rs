//! The player table component over the fabricated sample report: its
//! headers, row expansion, row menu and the copy notice it sends.

use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;

use nsh_core::element::Root;
use nsh_core::prelude::*;
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use wows_toolkit_nsh::replay_inspector::columns::column_key;
use wows_toolkit_nsh::replay_inspector::document;
use wows_toolkit_nsh::replay_inspector::table::player_table;
use wows_toolkit_nsh::replay_inspector::test_support::sample_document;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::player_table::column_label;
use wows_toolkit_replay_data::sample::sample_model;

/// The table saves dragged widths; they go to a database of the test's own.
static TEST_STORAGE: LazyLock<PathBuf> = LazyLock::new(|| {
    let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-player-table-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).expect("the test settings directory is created");
    wows_toolkit_config::storage_override::set(path.clone());
    path
});

/// The table over the sample report, every column shown, with its last
/// notice beside it.
#[component]
fn Host() -> impl IntoElement {
    let id = document::insert(sample_document(Path::new("sample.wowsreplay"), &[]));
    let notice = signal(String::new());
    let columns: Vec<SharedString> = ReplayColumn::ALL.iter().map(|col| SharedString::from(column_key(*col))).collect();

    view()
        .w(px(1400.0))
        .h(px(800.0))
        .child(text(move || format!("notice: {}", notice.get())))
        .child(
            player_table()
                .document(Some(id))
                .columns(columns)
                .on_notice(move |text: &SharedString| notice.set(text.to_string())),
        )
}

fn start() -> Headless {
    LazyLock::force(&TEST_STORAGE);
    let mut app = Headless::start(EngineConfig::new(Root::of::<Host>())).expect("the engine starts");
    app.set_viewport(Size::new(Points(1400.0), Points(800.0))).expect("the desktop viewport is applied");
    app.first_render().expect("the table renders");
    app
}

fn self_name() -> String {
    sample_model().rows.into_iter().find(|row| row.is_self).expect("the sample has a self row").display_name
}

/// The frozen titles and the scrolling ones in view at the table's width.
#[test]
fn the_titles_in_view_are_labelled() {
    let mut app = start();

    for col in [ReplayColumn::Name, ReplayColumn::ShipName, ReplayColumn::Skills, ReplayColumn::Kills] {
        app.wait_for(&format!("label:{}", column_label(col))).expect("the header shows");
    }
}

#[test]
fn a_row_expands_and_collapses_by_its_control() {
    let mut app = start();
    let name = self_name();

    app.press(&format!("label:Expand {name}")).expect("the expand control presses");
    app.wait_for(&format!("label:Collapse {name}")).expect("the row expands");
    app.press(&format!("label:Collapse {name}")).expect("the collapse control presses");
    app.wait_for(&format!("label:Expand {name}")).expect("the row collapses");
}

#[test]
fn a_rows_menu_copies_the_row_and_says_so() {
    let mut app = start();

    app.press("label:Player actions >> nth=0").expect("the row actions open");
    app.press("label:Copy Row").expect("Copy Row is offered");
    app.wait_for("text:notice: Copied").expect("the copy is confirmed");
}

#[test]
fn a_header_press_sorts_without_losing_the_rows() {
    let mut app = start();
    let name = self_name();

    app.press(&format!("label:{}", column_label(ReplayColumn::Kills))).expect("the header presses");
    app.wait_for(&format!("label:Expand {name}")).expect("the rows are still shown");
}
