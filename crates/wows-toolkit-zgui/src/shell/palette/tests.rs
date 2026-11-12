//! The palette driven through the shell, as the reader drives it.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use futures::channel::oneshot;
use landaire_ui::Appearance;
use landaire_ui::MARK_CLASS;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use wows_toolkit_viewmodel::palette::PaletteMode;
use zgui::reactive::Mounted;
use zgui::reactive::prelude::GetUntracked;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::shell::palette::Layer;
use crate::shell::palette::ModeQuery;
use crate::shell::palette::ModeRow;
use crate::shell::palette::ModeRows;
use crate::shell::palette::ModeSource;
use crate::shell::palette::ModeSpec;
use crate::shell::palette::PaletteControl;
use crate::shell::palette::PaletteRun;
use crate::shell::palette::ShipParamIndex;
use crate::shell::palette::builtin::TEST_MODES;
use crate::shell::palette::builtin::TestModes;
use crate::shell::palette::modes::ArmorShipRow;

/// The dialog itself, open or closing.
const PALETTE: &str = "palette";
const ROOT: &str = "palette-root";
const BACK: &str = "palette-back";

/// Lets a key press settle, and the dialog's entry or exit play out.
fn settle(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(400));
}

fn press(window: &mut HeadlessWindow, key: NamedKey) {
    window.press_key(key, Modifiers::NONE);
    settle(window);
}

fn shortcut(window: &mut HeadlessWindow, key: char) {
    window.press_character(key, Modifiers::PRIMARY);
    settle(window);
}

fn type_text(window: &mut HeadlessWindow, text: &str) {
    window.type_text(text);
    settle(window);
}

fn shown(window: &HeadlessWindow, id: &str) -> bool {
    window.by_test_id(id).is_some()
}

/// What a screen reader is told the element `id` is called.
fn label(window: &mut HeadlessWindow, id: &str) -> Option<String> {
    let element = window.by_test_id(id)?;
    window.a11y(element)?.label
}

fn open_window(modes: TestModes) -> HeadlessWindow {
    TEST_MODES.set(modes);
    open_shell(HeadlessOptions::new(REVIEW_SIZE))
}

/// The palette opens on the shortcuts the egui app takes, ctrl+k and ctrl+p, on its root list,
/// and Escape in the empty field closes it.
#[test]
fn the_palette_opens_on_the_shortcut_and_closes_on_escape() {
    let mut window = open_window(TestModes::default());
    assert!(!shown(&window, PALETTE), "closed until it is asked for");

    for key in ['k', 'p'] {
        shortcut(&mut window, key);
        assert!(shown(&window, ROOT), "ctrl+{key} opens the root list");
        assert!(shown(&window, "palette-item-go-to-stats"), "which lists its entries");

        press(&mut window, NamedKey::Escape);
        assert!(!shown(&window, PALETTE), "Escape in the empty root list closes it");
    }
}

/// Every tab, the settings and the shell's own commands are on the root list; the Search tab is
/// offered as the advanced search rather than by its name.
#[test]
fn the_root_list_offers_every_tab_and_the_shells_commands() {
    let mut window = open_window(TestModes::default());
    shortcut(&mut window, 'k');

    for id in [
        "go-to-replay-inspector",
        "go-to-launcher",
        "go-to-stats",
        "go-to-player-tracker",
        "go-to-armor-viewer",
        "go-to-unpacker",
        "open-settings",
        "advanced-search",
        "mode-players",
        "mode-my-ships",
        "games-i-died-in",
        "games-i-won",
        "open-replay-file",
        "index-all-replays",
        "refresh-persisted-data",
        "copy-latest-log",
        "copy-bug-report-info",
        "set-theme-system",
        "set-theme-dark",
        "set-theme-light",
    ] {
        let id = format!("palette-item-{id}");
        assert!(shown(&window, &id), "{id} is offered");
        assert!(label(&mut window, &id).is_some_and(|label| !label.trim().is_empty()), "{id} is named");
    }
    assert!(!shown(&window, "palette-item-go-to-search"));
}

/// The first entry is highlighted as the list opens, so Enter runs it, and the arrow keys walk
/// the list.
#[test]
fn enter_runs_the_highlighted_entry_and_the_arrows_walk() {
    let mut window = open_window(TestModes::default());

    shortcut(&mut window, 'k');
    press(&mut window, NamedKey::ArrowDown);
    press(&mut window, NamedKey::Enter);
    assert!(!shown(&window, PALETTE), "Enter runs the highlighted entry");
    assert!(shown(&window, "tab-page-launcher"), "the second entry, one step down from the first");

    shortcut(&mut window, 'k');
    press(&mut window, NamedKey::Enter);
    assert!(!shown(&window, PALETTE), "an untouched list runs its first entry");
}

/// Typing narrows the root list to the labels that hold the query, and Enter runs the entry,
/// closing the palette.
#[test]
fn typing_narrows_the_root_list_and_enter_runs_the_entry() {
    let mut window = open_window(TestModes::default());
    assert!(!shown(&window, "tab-page-stats"), "the stats tab is built once it is shown");

    shortcut(&mut window, 'k');
    type_text(&mut window, "STATS");
    assert!(shown(&window, "palette-item-go-to-stats"));
    assert!(!shown(&window, "palette-item-go-to-launcher"), "an entry that does not hold the query goes");
    assert!(!shown(&window, "palette-item-set-theme-dark"));

    press(&mut window, NamedKey::Enter);
    assert!(!shown(&window, PALETTE), "running an entry closes the palette");
    assert!(shown(&window, "tab-page-stats"), "going to a tab by name shows it");

    shortcut(&mut window, 'k');
    type_text(&mut window, "zzzz");
    assert!(shown(&window, ROOT));
    assert!(window.by_class("zui-combobox__empty").is_some(), "a query nothing holds says so");
    press(&mut window, NamedKey::Escape);
    assert!(shown(&window, ROOT), "Escape empties a typed query before it closes anything");
    assert!(shown(&window, "palette-item-go-to-stats"), "and the whole list is back");
}

/// A mode replaces the root list under a header leading back. Backspace in its empty field, or
/// Escape once the query is empty, returns to the root list.
#[test]
fn a_mode_returns_to_the_root_list() {
    let mut window = open_window(TestModes::default());
    shortcut(&mut window, 'k');
    type_text(&mut window, "search players");
    press(&mut window, NamedKey::Enter);
    // The players are read from the replay index on the tokio runtime.
    settle_in_real_time(&mut window);

    assert!(shown(&window, PALETTE), "entering a mode leaves a palette up");
    assert!(shown(&window, "palette-mode-players"), "the players list replaces the root list");
    assert!(shown(&window, BACK));
    assert!(!shown(&window, ROOT));
    assert!(window.by_class("zui-combobox__empty").is_some(), "an empty index has no players to list");

    press(&mut window, NamedKey::Backspace);
    assert!(shown(&window, ROOT), "Backspace in the empty field returns to the root list");
    assert!(!shown(&window, BACK));

    type_text(&mut window, "my matches");
    press(&mut window, NamedKey::Enter);
    assert!(shown(&window, "palette-mode-my-ships"));
    type_text(&mut window, "x");
    press(&mut window, NamedKey::Escape);
    assert!(shown(&window, "palette-mode-my-ships"), "Escape empties a typed query first");
    press(&mut window, NamedKey::Escape);
    assert!(shown(&window, ROOT), "then returns to the root list");
    press(&mut window, NamedKey::Escape);
    assert!(!shown(&window, PALETTE), "and from there closes the palette");

    shortcut(&mut window, 'k');
    window
        .click(window.by_test_id("palette-item-mode-players").expect("the players entry is listed"))
        .expect("the entry can be clicked");
    settle(&mut window);
    assert!(shown(&window, "palette-mode-players"), "clicking a mode's entry enters it");
    window.click(window.by_test_id(BACK).expect("the header leads back")).expect("the back button can be clicked");
    settle(&mut window);
    assert!(shown(&window, ROOT), "the header's back button returns to the root list");
}

fn ships() -> Vec<ArmorShipRow> {
    [("PASB018", "Montana", "U.S.A."), ("PJSB018", "Yamato", "Japan"), ("PZSD109", "Sm\u{e5}land", "Europe")]
        .into_iter()
        .map(|(index, name, nation)| {
            ArmorShipRow::new(ShipParamIndex(index.to_owned()), name.to_owned(), "X", "Battleship", nation)
        })
        .collect()
}

/// Ctrl+P, the ship entry, a ship's name and Enter open that ship in the Armor Viewer. On the
/// Armor Viewer, ctrl+p goes straight to the ship list.
#[test]
fn the_palette_opens_any_ship_in_the_armor_viewer() {
    let mut window = open_window(TestModes { ships: Some(ships()), ..TestModes::default() });

    shortcut(&mut window, 'p');
    assert!(shown(&window, ROOT), "ctrl+p elsewhere opens the root list");
    type_text(&mut window, "open ship");
    press(&mut window, NamedKey::Enter);
    assert!(shown(&window, "palette-mode-armor-ships"), "the ship entry opens the ship list");
    assert!(shown(&window, "palette-row-2"), "an empty search lists every ship");

    type_text(&mut window, "smal");
    assert!(
        label(&mut window, "palette-row-0").is_some_and(|label| label.starts_with("Sm\u{e5}land")),
        "the search folds diacritics"
    );
    assert!(!shown(&window, "palette-row-1"), "and lists only the match");

    press(&mut window, NamedKey::Escape);
    assert!(shown(&window, "palette-mode-armor-ships"), "Escape empties the query and stays in the ship list");
    type_text(&mut window, "yamat");
    press(&mut window, NamedKey::Enter);
    assert!(!shown(&window, PALETTE), "choosing a ship closes the palette");
    assert!(shown(&window, "tab-page-armor-viewer"), "the Armor Viewer is brought forward");

    shortcut(&mut window, 'p');
    assert!(shown(&window, "palette-mode-armor-ships"), "on the Armor Viewer ctrl+p opens the ship list");
    press(&mut window, NamedKey::Escape);
    assert!(shown(&window, ROOT), "Escape in the ship list returns to the root list");
    press(&mut window, NamedKey::Escape);
    assert!(!shown(&window, PALETTE), "Escape in the root list closes it");

    shortcut(&mut window, 'k');
    assert!(shown(&window, ROOT), "ctrl+k opens the root list on any tab");
    press(&mut window, NamedKey::Escape);
    shortcut(&mut window, 'p');
    press(&mut window, NamedKey::Backspace);
    assert!(shown(&window, ROOT), "Backspace in an empty ship search returns to the root list");
}

/// A row marks the letters of its name the query matched, and an empty query marks nothing.
#[test]
fn rows_mark_the_letters_the_query_matched() {
    let mut window = open_window(TestModes { ships: Some(ships()), ..TestModes::default() });
    shortcut(&mut window, 'k');
    type_text(&mut window, "open ship");
    press(&mut window, NamedKey::Enter);
    assert!(marked(&window).is_empty(), "an empty query marks nothing");

    type_text(&mut window, "ymt");
    assert_eq!(marked(&window), ["Y", "m", "t"], "each letter where it matched");

    press(&mut window, NamedKey::Escape);
    type_text(&mut window, "smal");
    assert_eq!(marked(&window), ["Sm\u{e5}l"], "a folded letter is marked as written");
}

/// The marked pieces the rows show. The list builds each row twice, the second time in the
/// catalogue it walks, so the pieces come in two equal halves.
fn marked(window: &HeadlessWindow) -> Vec<String> {
    let mut pieces = window.texts_by_class(MARK_CLASS);
    let copy = pieces.split_off(pieces.len() / 2);
    assert_eq!(pieces, copy, "the catalogue holds the same rows");
    pieces
}

/// With no game data the ship list is still where ctrl+p on the Armor Viewer goes, and says why
/// it is empty rather than falling back to the root list.
#[test]
fn ctrl_p_on_the_armor_viewer_without_game_data_opens_the_ship_list() {
    let mut window = open_window(TestModes::default());
    shortcut(&mut window, 'k');
    window
        .click(window.by_test_id("palette-item-go-to-armor-viewer").expect("the Armor Viewer is listed"))
        .expect("the entry can be clicked");
    settle(&mut window);
    assert!(shown(&window, "tab-page-armor-viewer"));

    shortcut(&mut window, 'p');
    assert!(shown(&window, "palette-mode-armor-ships"), "ctrl+p opens the ship list");
    assert!(!shown(&window, ROOT));
}

/// An answer to a query the reader has since changed is dropped, even when it arrives before
/// the pause after the change has ended.
#[test]
fn an_answer_to_a_replaced_query_is_dropped() {
    type Pending = Rc<RefCell<Vec<(String, oneshot::Sender<ModeRows>)>>>;
    let pending: Pending = Rc::default();
    let asked = Rc::clone(&pending);
    let players = move |needle: String| -> ModeQuery {
        let (answer, answered) = oneshot::channel();
        asked.borrow_mut().push((needle, answer));
        Box::pin(async move { answered.await.unwrap_or(ModeRows::Ready(Vec::new())) })
    };
    let answer = |needle: &str| {
        let at = pending.borrow().iter().position(|(asked, _)| asked == needle).expect("the query was read");
        let (_, answer) = pending.borrow_mut().remove(at);
        let row = ModeRow::new(format!("answer to {needle}"), PaletteRun::action(|| {}));
        // The task reading a replaced query may already be gone.
        let _ = answer.send(ModeRows::Ready(vec![row]));
    };
    let mut window = open_window(TestModes { players: Some(Rc::new(players)), ..TestModes::default() });

    shortcut(&mut window, 'k');
    type_text(&mut window, "search players");
    press(&mut window, NamedKey::Enter);
    assert!(shown(&window, "palette-mode-players"));

    window.type_text("a");
    window.advance(Duration::from_millis(200));
    window.type_text("b");
    window.advance(Duration::from_millis(10));
    answer("a");
    window.advance(Duration::from_millis(20));
    assert!(!shown(&window, "palette-row-0"), "the answer to the replaced query is not listed");

    window.advance(Duration::from_millis(200));
    answer("ab");
    window.advance(Duration::from_millis(20));
    assert_eq!(label(&mut window, "palette-row-0").as_deref(), Some("answer to ab"));
}

fn local_mode() -> ModeSpec {
    ModeSpec { source: ModeSource::Local(Rc::new(|_: &str| ModeRows::Ready(Vec::new()))), on_enter: None }
}

/// A mode open on an offer taken back opens again, as a new opening, from the latest offer left,
/// and returns to the root list once none is left.
#[test]
fn a_mode_whose_offer_is_taken_back_reopens_from_the_offer_left() {
    zgui::reactive::install().expect("the reactive runtime installs");
    let root = Mounted::new();
    let palette = root.with(PaletteControl::new);

    let first = Mounted::new();
    first.with(|| palette.offer_mode(PaletteMode::Players, local_mode()));
    palette.open_mode(PaletteMode::Players);
    let second = Mounted::new();
    second.with(|| palette.offer_mode(PaletteMode::Players, local_mode()));
    let opened = palette.opening.get_untracked().expect("the mode is open");

    first.unmount();
    let reopened = palette.opening.get_untracked().expect("the palette stays open");
    assert_eq!(reopened.layer, Layer::Mode(PaletteMode::Players));
    assert_ne!(reopened.id, opened.id, "the list is opened again");
    assert_ne!(reopened.offer, opened.offer, "from the offer left");
    assert!(reopened.offer.and_then(|offer| palette.offered_spec(offer)).is_some());

    second.unmount();
    assert_eq!(palette.layer_untracked(), Some(Layer::Root), "with no offer left, the root list");

    root.unmount();
}

/// The directory list puts "Browse..." first, and marks a recent directory that is gone once it
/// has been looked for.
#[test]
fn the_directory_list_marks_a_directory_that_is_gone() {
    let present = tempfile::tempdir().expect("a temporary directory can be made");
    let folders = vec![present.path().to_path_buf(), PathBuf::from("/nonexistent/old replays")];
    let mut window = open_window(TestModes { folders: Some(folders), ..TestModes::default() });

    shortcut(&mut window, 'k');
    type_text(&mut window, "open replay directory");
    press(&mut window, NamedKey::Enter);
    // The directories are looked for off the UI thread.
    settle_in_real_time(&mut window);

    assert!(shown(&window, "palette-mode-replay-folders"));
    assert!(shown(&window, "palette-row-2"), "Browse and both directories are listed");
    assert!(shown(&window, "palette-folder-missing"), "the directory that is gone is marked");

    type_text(&mut window, "old replays");
    assert!(shown(&window, "palette-row-1"), "Browse and the match");
    assert!(!shown(&window, "palette-row-2"));
}

/// Draws the root list and a mode's list over the shell, in Dark and Light.
#[test]
#[ignore = "draws on the graphics device; run by hand to review the palette"]
fn the_palette_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        let folders =
            vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from("/nonexistent/World_of_Warships/replays")];
        TEST_MODES.set(TestModes { folders: Some(folders), ..TestModes::default() });
        let mut window = open_shell(review_options(appearance));

        shortcut(&mut window, 'k');
        println!("{}", save_shot(&mut window, &format!("palette-root-{name}")).display());

        type_text(&mut window, "open replay directory");
        press(&mut window, NamedKey::Enter);
        settle_in_real_time(&mut window);
        println!("{}", save_shot(&mut window, &format!("palette-mode-{name}")).display());
    }
}
