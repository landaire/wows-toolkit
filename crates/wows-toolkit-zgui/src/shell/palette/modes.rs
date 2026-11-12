//! The modes the root list leads into, ready to offer with
//! [`PaletteControl::offer_mode`](crate::shell::palette::PaletteControl::offer_mode).

use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::force_reindex::ReindexScope;
use wows_toolkit_replay_data::force_reindex::VersionCount;
use wows_toolkit_replay_data::force_reindex::version_said;
use wows_toolkit_replay_data::paths::shown_path;
use wows_toolkit_viewmodel::palette;
use wows_toolkit_viewmodel::palette::IndexList;
use wows_toolkit_viewmodel::palette::IndexLookup;
use wows_toolkit_viewmodel::palette::SeededSearch;
use wows_toolkit_viewmodel::text_filter::FilterKey;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_detached;
use zgui::task::spawn_local;

use crate::services::Services;
use crate::shell::ReplayRequest;
use crate::shell::Shell;
use crate::shell::palette::ModeQuery;
use crate::shell::palette::ModeRow;
use crate::shell::palette::ModeRows;
use crate::shell::palette::ModeSource;
use crate::shell::palette::ModeSpec;
use crate::shell::palette::PaletteRun;
use crate::shell::palette::ShipParamIndex;
use crate::shell::palette::TabRequest;
use crate::shell::palette::chosen;
use crate::text_marks::marks;
use crate::text_marks::marks_within;

/// Players the replay index has seen whose name matches the query, best first, each leading to
/// the battles they were in.
pub fn players(services: Services) -> ModeSpec {
    index_mode(services, IndexList::Players)
}

/// Ships the reader has played whose name matches the query, best first, each leading to the
/// reader's own battles in it.
pub fn my_ships(services: Services) -> ModeSpec {
    index_mode(services, IndexList::MyShips)
}

/// Reads the list once each time the mode is entered and ranks it for every query typed.
fn index_mode(services: Services, list: IndexList) -> ModeSpec {
    let read: Rc<RefCell<Option<Rc<IndexLookup>>>> = Rc::default();
    let held = Rc::clone(&read);
    let fetch = move |needle: String| -> ModeQuery {
        let held = Rc::clone(&held);
        Box::pin(async move {
            let cached = held.borrow().clone();
            let lookup = match cached {
                Some(lookup) => lookup,
                None => match load(services, list).await {
                    Ok(lookup) => {
                        let lookup = Rc::new(lookup);
                        *held.borrow_mut() = Some(Rc::clone(&lookup));
                        lookup
                    }
                    Err(unavailable) => return unavailable,
                },
            };
            let query = FilterQuery::new(&needle);

            ModeRows::Ready(
                palette::seeded_searches(&lookup, &query).into_iter().map(|found| seeded_row(found, &query)).collect(),
            )
        })
    };
    let forget = move || drop(read.borrow_mut().take());

    ModeSpec { source: ModeSource::Query(Rc::new(fetch)), on_enter: Some(Rc::new(forget)) }
}

/// Reads `list` from the replay index, or says why it could not.
async fn load(services: Services, list: IndexList) -> Result<IndexLookup, ModeRows> {
    let Some(pool) = services.settings.pool() else {
        return Err(failed("The replay index is not open"));
    };

    match services.on_tokio(async move { list.load(&pool).await }).await {
        Ok(Ok(lookup)) => Ok(lookup),
        Ok(Err(err)) => {
            tracing::warn!(%err, "palette: mode query failed");
            Err(failed(err))
        }
        Err(err) => {
            tracing::warn!(%err, "palette: mode query did not complete");
            Err(failed(err))
        }
    }
}

fn seeded_row(found: SeededSearch, query: &FilterQuery) -> ModeRow {
    ModeRow {
        marks: marks(query, &found.label),
        ..ModeRow::new(found.label, PaletteRun::Request(TabRequest::Search(found.query)))
    }
}

fn failed(reason: impl std::fmt::Display) -> ModeRows {
    ModeRows::Unavailable(t!("ui.search.failed", reason = reason).into_owned())
}

/// The game versions the listing holds, newest first, each running what `reindex` makes of a
/// forced re-index of its replays.
pub fn reindex_versions(
    versions: Signal<Vec<VersionCount>, LocalStorage>,
    reindex: impl Fn(ReindexScope) -> PaletteRun + 'static,
) -> ModeSpec {
    let filter = move |query: &str| versions.with(|rows| version_rows(rows, query, &reindex));

    ModeSpec { source: ModeSource::Local(Rc::new(filter)), on_enter: None }
}

/// Every version in the order given when the query is inactive, else those whose
/// `major.minor.patch` matches it, best first.
fn version_rows(rows: &[VersionCount], query: &str, reindex: &dyn Fn(ReindexScope) -> PaletteRun) -> ModeRows {
    if rows.is_empty() {
        return ModeRows::Unavailable(t!("ui.reindex.no_versions").into_owned());
    }

    let query = FilterQuery::new(query);
    let keyed = rows.iter().map(|row| (FilterKey::new(&version_said(&row.version)), row));
    let listed = query
        .rank(keyed, |(key, _)| key)
        .into_iter()
        .map(|(_, row)| {
            let said = version_said(&row.version);
            let label = t!("ui.reindex.version_row", version = said, count = row.replays).into_owned();
            let run = reindex(ReindexScope::Version(row.version));
            ModeRow { marks: marks_within(&query, &label, &said), ..ModeRow::new(label, run) }
        })
        .collect();

    ModeRows::Ready(listed)
}

/// "Browse..." and the directories in `history`, most recent first, each opened in the replays
/// tab. A directory that is gone is marked once entering the mode has looked for it.
pub fn replay_folders(shell: Shell, history: Signal<Vec<PathBuf>, LocalStorage>) -> ModeSpec {
    let missing = RwSignal::new_local(Vec::<PathBuf>::new());
    let owner = Owner::current().expect("a mode is offered inside a component");
    let checking = StoredValue::new_local(None::<Task>);
    let checked = StoredValue::new(0_u64);
    let browse = PaletteRun::action(move || browse_folder(shell));
    let open = move |path: PathBuf| {
        PaletteRun::action(move || shell.request_replay(ReplayRequest::OpenRecentFolder(path.clone())))
    };
    let filter = move |query: &str| {
        history.with(|paths| missing.with(|missing| folder_rows(paths, missing, query, &browse, &open)))
    };

    let check = move || {
        let paths = history.get_untracked();
        checked.update_value(|checked| *checked += 1);
        let this = checked.get_value();
        // Spawned under the offering scope rather than the key press that enters the mode, so
        // the check ends with the offer. Off the UI thread, since a directory on an offline share
        // can take many seconds to answer.
        let task = owner.with(|| {
            spawn_local(async move {
                let gone: Vec<PathBuf> =
                    blocking(move || paths.into_iter().filter(|path| !path.is_dir()).collect()).await;
                if checked.try_get_value() == Some(this) {
                    missing.try_set(gone);
                }
            })
        });
        if let Some(Some(earlier)) = checking.try_update_value(|slot| slot.replace(task)) {
            earlier.cancel();
        }
    };

    ModeSpec { source: ModeSource::Local(Rc::new(filter)), on_enter: Some(Rc::new(check)) }
}

/// "Browse..." first, then the directories: most recent first for an inactive query, else those
/// whose shown path matches it, best first. `open` makes what choosing one directory does.
fn folder_rows(
    paths: &[PathBuf],
    missing: &[PathBuf],
    query: &str,
    browse: &PaletteRun,
    open: &dyn Fn(PathBuf) -> PaletteRun,
) -> ModeRows {
    let browse = ModeRow {
        glyph: Some(Glyph::FolderOpen),
        ..ModeRow::new(t!("ui.replay_folders.browse").into_owned(), browse.clone())
    };

    let query = FilterQuery::new(query);
    let keyed = paths.iter().map(|path| {
        let shown = shown_path(path);
        (FilterKey::new(&shown), path)
    });
    let recent = query.rank(keyed, |(key, _)| key).into_iter().map(|(_, path)| {
        let label = folder_label(path);
        let shown = shown_path(path);
        ModeRow {
            // The row is kept by its path; the name is marked where it also matches.
            marks: marks(&query, &label),
            detail_marks: marks(&query, &shown),
            detail: Some(shown),
            glyph: Some(Glyph::Folder),
            missing: missing.contains(path),
            ..ModeRow::new(label, open(path.clone()))
        }
    });

    ModeRows::Ready(std::iter::once(browse).chain(recent).collect())
}

/// What a directory row is called: the directory's own name.
fn folder_label(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => shown_path(path),
    }
}

/// Asks for a directory of replays and lists it in the replays tab.
fn browse_folder(shell: Shell) {
    let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.replay.open_directory"));
    // Detached: the palette that asked has closed before the reader answers.
    spawn_detached(async move {
        let Some(directory) = chosen(shell, asked).await else { return };
        shell.request_replay(ReplayRequest::OpenFolder(directory));
    });
}

/// One ship the ship mode lists.
#[derive(Clone, Debug)]
pub struct ArmorShipRow {
    param_index: ShipParamIndex,
    display_name: String,
    key: FilterKey,
    /// The name with tier, class and nation, which is what the row shows.
    label: String,
}

impl ArmorShipRow {
    /// `tier`, `class` and `nation` as the row says them: the tier in roman numerals, the class
    /// and the nation by name.
    pub fn new(param_index: ShipParamIndex, display_name: String, tier: &str, class: &str, nation: &str) -> Self {
        let key = FilterKey::new(&display_name);
        let label = t!("ui.palette.armor_ship_row", name = display_name, tier = tier, class = class, nation = nation)
            .into_owned();

        Self { param_index, display_name, key, label }
    }
}

/// The ship catalogue as the ship mode sees it.
#[derive(Clone, Debug)]
pub enum ShipList {
    /// Still loading; the mode fills in once it lands.
    Pending,
    Ready(Rc<[ArmorShipRow]>),
    /// Why there is no catalogue to list.
    Unavailable(String),
}

/// Every ship in `ships`, best match first, each opened in the Armor Viewer. Entering the mode
/// runs `load`, which starts the catalogue load the list waits on.
pub fn armor_ships(ships: Signal<ShipList, LocalStorage>, load: impl Fn() + 'static) -> ModeSpec {
    let filter = move |query: &str| ships.with(|list| ship_rows(list, query));

    ModeSpec { source: ModeSource::Local(Rc::new(filter)), on_enter: Some(Rc::new(load)) }
}

fn ship_rows(list: &ShipList, query: &str) -> ModeRows {
    match list {
        ShipList::Pending => ModeRows::Loading(t!("ui.palette.ships_loading").into_owned()),
        ShipList::Unavailable(reason) => {
            ModeRows::Unavailable(t!("ui.palette.ships_unavailable", reason = reason).into_owned())
        }
        ShipList::Ready(rows) => {
            let query = FilterQuery::new(query);
            let listed = query
                .rank(rows.iter(), |row| &row.key)
                .into_iter()
                .map(|row| {
                    let request = TabRequest::ViewArmor {
                        param_index: row.param_index.clone(),
                        display_name: row.display_name.clone(),
                    };
                    ModeRow {
                        marks: marks_within(&query, &row.label, &row.display_name),
                        ..ModeRow::new(row.label.clone(), PaletteRun::Request(request))
                    }
                })
                .collect();

            ModeRows::Ready(listed)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use wowsunpack::data::Version;

    use super::*;

    fn ready(rows: ModeRows) -> Vec<ModeRow> {
        match rows {
            ModeRows::Ready(rows) => rows,
            ModeRows::Loading(said) | ModeRows::Unavailable(said) => panic!("rows were expected, not {said:?}"),
        }
    }

    fn act(row: &ModeRow) {
        match &row.run {
            PaletteRun::Action(action) => action(),
            PaletteRun::Request(request) => panic!("an action was expected, not {request:?}"),
        }
    }

    /// Every version in order for an empty query, else the matches best first; each row
    /// re-indexes its own version.
    #[test]
    fn versions_rank_by_their_number() {
        let rows = [
            VersionCount { version: Version::base(14, 3, 0), replays: 4 },
            VersionCount { version: Version::base(14, 2, 1), replays: 1 },
            VersionCount { version: Version::base(13, 4, 0), replays: 7 },
        ];
        let chosen = |query: &str| -> Vec<ReindexScope> {
            let asked = Rc::new(RefCell::new(Vec::new()));
            let reindex = {
                let asked = Rc::clone(&asked);
                move |scope: ReindexScope| {
                    let asked = Rc::clone(&asked);
                    PaletteRun::action(move || asked.borrow_mut().push(scope.clone()))
                }
            };
            ready(version_rows(&rows, query, &reindex)).iter().for_each(act);

            asked.take()
        };
        let version = |major, minor, patch| ReindexScope::Version(Version::base(major, minor, patch));

        assert_eq!(chosen(""), [version(14, 3, 0), version(14, 2, 1), version(13, 4, 0)]);
        assert_eq!(chosen("14.2"), [version(14, 2, 1)]);
        // A "4" that starts a component ranks above one inside "14".
        assert_eq!(chosen("4"), [version(13, 4, 0), version(14, 3, 0), version(14, 2, 1)]);
        assert!(chosen("15").is_empty());
    }

    /// A listing that names no version says so rather than listing nothing.
    #[test]
    fn no_versions_is_said() {
        let rows = version_rows(&[], "", &|_| PaletteRun::action(|| {}));

        assert!(matches!(rows, ModeRows::Unavailable(_)));
    }

    /// "Browse..." stays first, the directories that hold the query follow most recent first, and
    /// one that is gone is marked.
    #[test]
    fn folders_keep_browse_first_and_mark_the_missing() {
        let paths = [PathBuf::from("/r/Prism"), PathBuf::from("/r/Ranked games")];
        let missing = [PathBuf::from("/r/Prism")];
        let opened = Rc::new(RefCell::new(Vec::new()));
        let open = {
            let opened = Rc::clone(&opened);
            move |path: PathBuf| {
                let opened = Rc::clone(&opened);
                PaletteRun::action(move || opened.borrow_mut().push(path.clone()))
            }
        };
        let browse = PaletteRun::action(|| {});
        let listed = |query: &str| ready(folder_rows(&paths, &missing, query, &browse, &open));

        let all = listed("");
        let labels: Vec<&str> = all.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, [t!("ui.replay_folders.browse").as_ref(), "Prism", "Ranked games"]);
        assert_eq!(all[0].glyph, Some(Glyph::FolderOpen));
        assert!(all[1].missing, "a directory that is gone is marked");
        assert!(!all[2].missing);
        assert_eq!(all[2].detail.as_deref(), Some(shown_path(Path::new("/r/Ranked games")).as_str()));

        let ranked = listed(" RANKED ");
        assert_eq!(ranked.len(), 2, "Browse and the one match");
        act(&ranked[1]);
        assert_eq!(*opened.borrow(), [PathBuf::from("/r/Ranked games")]);

        assert_eq!(listed("zzz").len(), 1, "Browse is listed whatever is typed");
    }

    /// The ship list says it is loading or why it has nothing, and once ready ranks the names and
    /// opens the chosen ship in the Armor Viewer.
    #[test]
    fn ships_wait_for_the_catalogue_then_rank_by_name() {
        assert!(matches!(ship_rows(&ShipList::Pending, ""), ModeRows::Loading(_)));
        let ModeRows::Unavailable(said) = ship_rows(&ShipList::Unavailable("no install".to_owned()), "") else {
            panic!("an unavailable catalogue says why");
        };
        assert!(said.contains("no install"), "{said}");

        let ship = |index: &str, name: &str| {
            ArmorShipRow::new(ShipParamIndex(index.to_owned()), name.to_owned(), "X", "Cruiser", "Nation")
        };
        let list = ShipList::Ready(Rc::from([ship("PASC001", "Moskva"), ship("PASC002", "Des Moines")]));
        let opened: Vec<TabRequest> = ready(ship_rows(&list, "moines"))
            .into_iter()
            .map(|row| match row.run {
                PaletteRun::Request(request) => request,
                PaletteRun::Action(_) => panic!("a ship row asks the Armor Viewer"),
            })
            .collect();

        assert_eq!(
            opened,
            [TabRequest::ViewArmor {
                param_index: ShipParamIndex("PASC002".to_owned()),
                display_name: "Des Moines".to_owned(),
            }]
        );
    }
}
