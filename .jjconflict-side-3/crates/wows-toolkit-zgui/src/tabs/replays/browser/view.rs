//! One listing on screen: its rows, what it says while it has none, the rows' menu and the hover
//! preview.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use landaire_ui::BranchClick;
use landaire_ui::LeafLead;
use landaire_ui::RowHeight;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::TreeMenu;
use landaire_ui::TreeProps;
use landaire_ui::TreeRow;
use landaire_ui::TreeStripes;
use rust_i18n::t;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::force_reindex::Confirm;
use wows_toolkit_replay_data::paths::shown_path;
use wows_toolkit_viewmodel::listing_row::LineGlyph;
use wows_toolkit_viewmodel::listing_row::LinePart;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::personal_rating::chip_hue;
use wows_toolkit_viewmodel::personal_rating::chip_text;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui::ProgressProps;
use zgui_ui::SpinnerProps;

use crate::icons::AppIconProps;
use crate::preview::HoverPreview;
use crate::preview::PreviewTarget;
use crate::preview::card::HoverPreviewCardProps;
use crate::services::GameDataState;
use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::replays::browser::BrowserEvents;
use crate::tabs::replays::browser::ReplayBrowserEvent;
use crate::tabs::replays::browser::controls;
use crate::tabs::replays::browser::files;
use crate::tabs::replays::browser::listing::Listing;
use crate::tabs::replays::browser::listing::ListingSource;
use crate::tabs::replays::browser::listing::ScanStatus;
use crate::tabs::replays::browser::thumbnails::Thumbnail;
use crate::tabs::replays::browser::thumbnails::Thumbnails;
use crate::tabs::replays::browser::tree::BrowserTree;
use crate::tabs::replays::browser::tree::LeafRow;
use crate::tabs::replays::browser::tree::NodeKey;
use crate::theme::rating_band_name;

const SHEET: &str = "wt-replay-listing";

// The row is the theme's font size (12.5px) times 2.8, as the other front end draws it: the
// identity line, the smaller stats line under it, and a little space around both. The
// thumbnail's ground is Ocean's water, so a thumbnail on its way does not flash.
const CSS: &str = css!(
    ".wt-listing {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
    }
    .wt-listing .lui-tree-area, .wt-listing .lui-tree { flex: 1; min-height: 0; }
    .wt-listing__state {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 8px;
        font-size: 14px;
    }
    .wt-listing__state-hint { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-listing__loading {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 8px;
        padding: 16px 8px;
        font-size: 12px;
        color: var(--wt-color-text-dim);
    }
    .wt-listing__loading .zui-progress { width: 160px; }
    .wt-row {
        position: relative;
        display: flex;
        flex: 1;
        align-items: center;
        gap: 4px;
        height: 100%;
        min-width: 0;
        padding-right: 4px;
    }
    .wt-row-arrived { animation: wt-row-arrival 1200ms cubic-bezier(0.5, 1, 0.89, 1); }
    [reduced-motion] .wt-row-arrived { animation: none; }
    @keyframes wt-row-arrival {
        from { background-color: color-mix(in srgb, var(--lui-color-accent) 30%, transparent); }
        to { background-color: transparent; }
    }
    .wt-row__edge { flex: none; width: 3px; align-self: stretch; }
    .wt-row__edge--win { background-color: var(--wt-color-win); }
    .wt-row__edge--loss { background-color: var(--wt-color-loss); }
    .wt-row__edge--draw { background-color: var(--wt-color-draw); }
    .wt-row__thumb-slot { display: flex; flex: none; }
    .wt-row__thumb {
        flex: none;
        width: 27px;
        height: 27px;
        border-radius: 2px;
        overflow: hidden;
        background-color: #06272e;
    }
    .wt-row__thumb--missing {
        background-color: var(--lui-color-widget);
        border: 1px solid var(--lui-color-border);
    }
    .wt-row__thumb image { width: 27px; height: 27px; }
    .wt-row__lines {
        display: flex;
        flex-direction: column;
        justify-content: center;
        flex: 1;
        min-width: 0;
        overflow: hidden;
    }
    .wt-row__identity {
        display: flex;
        align-items: center;
        gap: 4px;
        min-width: 0;
    }
    .wt-row__label {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-row__group-label {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-row__division { color: var(--wt-color-division); }
    .wt-row__new {
        flex: none;
        padding: 0 4px;
        border-radius: 2px;
        border: 1px solid var(--lui-color-accent);
        font-size: 12px;
        color: var(--lui-color-accent);
    }
    .wt-row__spacer { flex: 1; }
    .wt-row__pr {
        flex: none;
        padding: 0 4px;
        border-radius: 2px;
        font-size: 12px;
        font-weight: 700;
    }
    .wt-row__part { white-space: pre; }
    .wt-row__stats {
        display: flex;
        align-items: center;
        font-size: 12px;
        color: var(--wt-color-text-dim);
        white-space: nowrap;
        overflow: hidden;
    }
    .wt-hover-facts { display: flex; flex-direction: column; max-width: 384px; }
    .wt-hover-fact { display: flex; gap: 8px; align-items: flex-start; font-size: 12px; }
    .wt-hover-fact__label { flex: none; width: 58px; color: var(--wt-color-text-dim); }
    .wt-hover-fact__value { flex: 1; min-width: 0; }"
);

/// The row height: the theme font size of 12.5px times 2.8.
const ROW_HEIGHT: RowHeight = RowHeight(35.0);

/// One listing: its rows once it has some, what it is doing until then.
#[component]
pub fn ListingView(listing: Listing, events: BrowserEvents) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let preview = HoverPreview::new();
    let thumbnails = Thumbnails::new();
    follow_thumbnails(listing, thumbnails, services);

    let body = move || {
        let status = listing.status.get();
        let waiting_on_names = matches!(services.game_data.state(), GameDataState::Loading { .. })
            && listing.source.with(|source| *source == ListingSource::Install);
        match status {
            ScanStatus::Loading => AnyView::new(view! { ScanLoading(listing = listing) }),
            ScanStatus::NoInstall => AnyView::new(view! {
                box(class = "wt-listing__state", attr:data-testid = "replay-listing-no-install") {
                    // Not in the catalog: the other front end says it in English as well.
                    text(class = "wt-listing__state-hint") {"World of Warships directory is not set"}
                }
            }),
            ScanStatus::Empty(dir) => AnyView::new(view! {
                column(class = "wt-listing__state", attr:data-testid = "replay-listing-empty") {
                    text {{t!("ui.replay.no_replays_in", folder = shown_path(&dir)).into_owned()}}
                    text(class = "wt-listing__state-hint") {{t!("ui.replay.drop_folder_hint").into_owned()}}
                }
            }),
            ScanStatus::Loaded if waiting_on_names => {
                let said = match services.game_data.state() {
                    GameDataState::Loading { version: Some(version) } => {
                        t!("ui.replay.loading_game_data_version", version = version).into_owned()
                    }
                    _ => t!("ui.replay.loading_game_data").into_owned(),
                };
                AnyView::new(view! {
                    column(class = "wt-listing__loading", attr:data-testid = "replay-listing-loading") {
                        Spinner(label = said.clone())
                        text {{said}}
                    }
                })
            }
            ScanStatus::Loaded => AnyView::new(view! {
                ListingRows(listing = listing, events = events.clone(), preview = preview, thumbnails = thumbnails)
            }),
        }
    };
    let facts = move || {
        let path = preview.watched()?;
        let hover = listing.tree.with(|tree| tree.leaf(&path).map(|leaf| leaf.hover.clone()))?;
        Some(view! {
            column(class = "wt-hover-facts") {
                for fact in move || hover.clone(), key = |fact: &wows_toolkit_viewmodel::listing_row::HoverFact| fact.label.clone() {
                    row(class = "wt-hover-fact") {
                        text(class = "wt-hover-fact__label") {{fact.label}}
                        text(class = "wt-hover-fact__value") {{fact.value}}
                    }
                }
            }
        })
    };

    view! {
        column(class = "wt-listing", attr:data-testid = "replay-listing", on:pointer_leave = move |_| preview.leave()) {
            {body}
            HoverPreviewCard(preview = preview) {
                {facts}
            }
        }
    }
}

/// Keeps the thumbnails of the listed maps loaded from the newest build open.
fn follow_thumbnails(listing: Listing, thumbnails: Thumbnails, services: Services) {
    let effect = RenderEffect::new(move |_| {
        let cache = services.game_data.cache();
        // The newest build changes as builds load, which the state follows.
        let _ = services.game_data.state();
        let spaces: HashSet<String> =
            listing.files.with(|files| files.iter().map(|file| file.listed.map_name.clone()).collect());
        thumbnails.want(cache.as_ref(), spaces);
    });
    on_cleanup_local(move || drop(effect));
}

/// The scan's spinner, with how far it has read once it knows how much there is.
#[component]
fn ScanLoading(listing: Listing) -> impl IntoView {
    let caption = move || match listing.progress.get() {
        Some(progress) if progress.total > 0 => {
            t!("ui.replay.scanning_count", done = progress.done, total = progress.total).into_owned()
        }
        _ => t!("ui.replay.scanning").into_owned(),
    };
    let fraction = move || {
        listing
            .progress
            .get()
            .filter(|progress| progress.total > 0)
            .map(|progress| progress.done as f64 / progress.total as f64 * 100.0)
    };

    view! {
        column(class = "wt-listing__loading", attr:data-testid = "replay-listing-scan") {
            Spinner(label = t!("ui.replay.scanning").into_owned())
            text(class = TABULAR_CLASS) {{caption}}
            Show(when = move || fraction().is_some()) {
                Progress(value = Signal::derive_local(fraction))
            }
        }
    }
}

/// The rows, as a tree grouped the way the reader chose.
#[component]
fn ListingRows(
    listing: Listing,
    events: BrowserEvents,
    preview: HoverPreview,
    thumbnails: Thumbnails,
) -> impl IntoView {
    let shell = Shell::current();
    let model: Signal<Rc<BrowserTree>, LocalStorage> = listing.tree.into();
    let activate_events = events.clone();
    let on_activate = UnsyncCallback::new(move |key: NodeKey| {
        let NodeKey::Leaf(path) = key else { return };
        listing.mark_opened(&path);
        activate_events.send(ReplayBrowserEvent::Open(path));
    });
    let menu_events = events.clone();
    let menu = TreeMenu::new(move |key: &NodeKey| row_menu(shell, listing, menu_events.clone(), key));

    view! {
        Tree(
            model = model,
            row = move |row: TreeRow<NodeKey>| row_view(listing, preview, thumbnails, row),
            row_height = ROW_HEIGHT,
            expanded = listing.expanded,
            selection = listing.selection,
            branch_click = BranchClick::SelectAndToggle,
            stripes = TreeStripes::Leaves,
            leaf_lead = LeafLead::Nothing,
            on_activate = on_activate,
            context_menu = menu,
            label = t!("ui.replay.listing_caption").into_owned(),
            attr:data-testid = "replay-listing-rows"
        )
    }
}

fn row_view(listing: Listing, preview: HoverPreview, thumbnails: Thumbnails, row: TreeRow<NodeKey>) -> AnyView {
    match row.key {
        NodeKey::Group(id) => {
            let label =
                move || listing.tree.with(|tree| tree.group(&id).map(|group| group.label.clone()).unwrap_or_default());
            AnyView::new(view! {
                row(class = "wt-row") {
                    box(class = "wt-row__edge")
                    text(class = "wt-row__group-label") {{label}}
                }
            })
        }
        NodeKey::Leaf(path) => {
            let anchor = row.anchor;
            let leaf_path = path.clone();
            let content = move || {
                let leaf = listing.tree.with(|tree| tree.leaf(&leaf_path).cloned())?;
                Some(leaf_view(listing, thumbnails, leaf_path.clone(), leaf))
            };
            let enter = move |_: &mut EventCx<'_, events::PointerEnter>| {
                let map_name = listing.tree.with_untracked(|tree| tree.leaf(&path).map(|leaf| leaf.map_name.clone()));
                preview.enter(PreviewTarget { path: path.clone(), map_name, anchor });
            };
            AnyView::new(view! {
                box(class = "wt-row-host", style:display = "contents", on:pointer_enter = enter) {
                    {content}
                }
            })
        }
    }
}

/// What a replay row draws: the outcome edge, the map, the identity line with its tags and the
/// stats line.
fn leaf_view(listing: Listing, thumbnails: Thumbnails, path: PathBuf, leaf: LeafRow) -> AnyView {
    let edge = match leaf.outcome {
        MatchOutcome::Win => "wt-row__edge wt-row__edge--win",
        MatchOutcome::Loss => "wt-row__edge wt-row__edge--loss",
        MatchOutcome::Draw => "wt-row__edge wt-row__edge--draw",
        MatchOutcome::Unknown => "wt-row__edge",
    };
    let arrived_path = path.clone();
    let arrived = Signal::derive_local(move || listing.arrivals.with(|arrivals| arrivals.contains(&arrived_path)));
    let space = leaf.map_name.clone();
    let thumb = move || match thumbnails.get(&space) {
        Some(Thumbnail::Ready(bytes)) => AnyView::new(view! {
            box(class = "wt-row__thumb") { image(src = bytes.url()) }
        }),
        Some(Thumbnail::Missing) => AnyView::new(view! { box(class = "wt-row__thumb wt-row__thumb--missing") }),
        Some(Thumbnail::Loading) | None => AnyView::new(view! { box(class = "wt-row__thumb") }),
    };
    let division = leaf.in_division.then(|| view! { AppIcon(glyph = Glyph::Users, class = "wt-row__division") });
    let rating = leaf.personal_rating.map(|rating| {
        let said = format!("{:.0}", rating.pr);
        let style = rating_style(rating.category);
        view! {
            box(class = "wt-row__spacer")
            text(class = format!("wt-row__pr {TABULAR_CLASS}"), style = style, a11y:label = said.clone()) {{said.clone()}}
        }
    });
    let stats: Vec<AnyView> = leaf
        .stats
        .into_iter()
        .map(|part| match part {
            LinePart::Text(text) => AnyView::new(view! { text(class = "wt-row__part") {{text}} }),
            LinePart::Glyph(glyph) => AnyView::new(view! { AppIcon(glyph = line_glyph(glyph)) }),
        })
        .collect();

    AnyView::new(view! {
        row(class = "wt-row", class:wt-row-arrived = arrived, attr:data-testid = format!("replay-row-{}", path.display())) {
            box(class = edge)
            box(class = "wt-row__thumb-slot") {{thumb}}
            column(class = "wt-row__lines") {
                row(class = "wt-row__identity") {
                    text(class = "wt-row__label") {{leaf.label}}
                    {division.map(IntoView::into_view)}
                    Show(when = move || arrived.get()) {
                        text(class = "wt-row__new", attr:data-testid = "replay-row-new") {{t!("ui.replay.new_tag").into_owned()}}
                    }
                    {rating.map(IntoView::into_view)}
                }
                row(class = "wt-row__stats") {{stats}}
            }
        }
    })
}

/// The icon a stats-line glyph is drawn as.
const fn line_glyph(glyph: LineGlyph) -> Glyph {
    match glyph {
        LineGlyph::Clock => Glyph::Clock,
        LineGlyph::Crosshair => Glyph::Crosshair,
        LineGlyph::Sword => Glyph::Sword,
    }
}

/// The rating chip's colours: the band's text colour on its hue at 46/255, through the theme's
/// properties, falling back to the dark theme's values.
fn rating_style(category: PersonalRatingCategory) -> String {
    let band = rating_band_name(category);
    let text = chip_text(category, true);
    let hue = chip_hue(category);

    format!(
        "color: var(--wt-pr-{band}-text, #{text:06x}); \
         background-color: color-mix(in srgb, var(--wt-pr-{band}-hue, #{hue:06x}) 18%, transparent);"
    )
}

/// The menu a secondary click on `key` opens.
fn row_menu(shell: Shell, listing: Listing, events: BrowserEvents, key: &NodeKey) -> AnyView {
    match key {
        NodeKey::Group(id) => {
            let paths =
                listing.tree.with_untracked(|tree| tree.group(id).map(|group| group.paths.clone()).unwrap_or_default());
            if paths.is_empty() {
                return AnyView::new(());
            }
            let count = paths.len();
            let (copied, listed, reindexed) = (paths.clone(), paths.clone(), paths);
            AnyView::new(view! {
                MenuItem(on_select = UnsyncCallback::new(move |()| files::copy_replay_files(shell.toasts(), &copied))) {
                    {t!("ui.replay.context.copy_replays", count = count).into_owned()}
                }
                MenuItem(on_select = UnsyncCallback::new(move |()| files::copy_paths(shell, &listed))) {
                    {t!("ui.replay.context.copy_paths", count = count).into_owned()}
                }
                MenuSeparator()
                MenuItem(on_select = UnsyncCallback::new({
                    let events = events.clone();
                    move |()| events.force_reindex(listing, &reindexed, Confirm::Always)
                })) {
                    {force_reindex_label(count)}
                }
            })
        }
        NodeKey::Leaf(path) => leaf_menu(shell, listing, events, path),
    }
}

fn leaf_menu(shell: Shell, listing: Listing, events: BrowserEvents, path: &Path) -> AnyView {
    let selection = listing.selected_paths();
    // Acting on the selection only when the row is part of it: a secondary click elsewhere acts
    // on that row alone.
    let batch =
        if selection.len() > 1 && selection.iter().any(|selected| selected == path) { selection } else { Vec::new() };
    let session = if batch.is_empty() { vec![path.to_path_buf()] } else { batch.clone() };
    let count = session.len();
    let path = path.to_path_buf();
    let wows_dir =
        Services::current().settings.read_untracked(|settings| settings.wows_dir.clone()).filter(|dir| !dir.is_empty());
    let cache = Services::current().game_data.cache_untracked();

    let item = {
        let events = events.clone();
        move |label: String, event: ReplayBrowserEvent| {
            let events = events.clone();
            view! {
                MenuItem(on_select = UnsyncCallback::new(move |()| events.send(event.clone()))) {{label}}
            }
        }
    };
    let batch_items = (!batch.is_empty()).then(|| {
        let many = batch.len();
        let (copied, listed) = (batch.clone(), batch.clone());
        view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| files::copy_replay_files(shell.toasts(), &copied))) {
                {t!("ui.replay.context.copy_replays", count = many).into_owned()}
            }
            MenuItem(on_select = UnsyncCallback::new(move |()| files::copy_paths(shell, &listed))) {
                {t!("ui.replay.context.copy_paths", count = many).into_owned()}
            }
            {item(t!("ui.replay.context.render_to_video_many", count = many).into_owned(), ReplayBrowserEvent::RenderManyToVideo(batch.clone()))}
            {item(t!("ui.replay.context.render_to_clipboard_many", count = many).into_owned(), ReplayBrowserEvent::RenderManyToClipboard(batch.clone()))}
            MenuSeparator()
        }
    });

    let opened = path.clone();
    let open = {
        let events = events.clone();
        move |event: ReplayBrowserEvent| {
            let events = events.clone();
            let opened = opened.clone();
            UnsyncCallback::new(move |()| {
                listing.mark_opened(&opened);
                events.send(event.clone());
            })
        }
    };
    let (set_label, add_label) = if count == 1 {
        (
            t!("ui.replay.context.set_session_stats_one").into_owned(),
            t!("ui.replay.context.add_session_stats_one").into_owned(),
        )
    } else {
        (
            t!("ui.replay.context.set_session_stats_many", count = count).into_owned(),
            t!("ui.replay.context.add_session_stats_many", count = count).into_owned(),
        )
    };
    let set_session = {
        let events = events.clone();
        let paths = session.clone();
        UnsyncCallback::new(move |()| {
            let events = events.clone();
            let paths = paths.clone();
            confirm(
                shell,
                t!("ui.replay.context.set_session_stats_one").into_owned(),
                t!("confirm.set_as_session_stats").into_owned(),
                move || events.send(ReplayBrowserEvent::SessionStats { paths: paths.clone(), replace: true }),
            );
        })
    };
    let reindex = {
        let events = events.clone();
        let paths = session.clone();
        UnsyncCallback::new(move |()| events.force_reindex(listing, &paths, Confirm::WhenNotPlain))
    };
    let (copied, listed, revealed) = (path.clone(), path.clone(), path.clone());
    let controls = cache.map(|cache| {
        let toasts = shell.toasts();
        view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| {
                let Some(newest) = cache.newest_loaded() else { return };
                match wows_toolkit_viewmodel::controls::read_scheme(newest.vfs()) {
                    Some(groups) => controls::open(shell, groups),
                    None => {
                        toasts.warn(t!("ui.replay.controls.unavailable").into_owned());
                    }
                }
            })) {
                {t!("ui.replay.context.show_replay_controls").into_owned()}
            }
        }
    });
    let in_game = wows_dir.map(|dir| {
        let replay = path.clone();
        view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| {
                let (dir, replay) = (PathBuf::from(&dir), replay.clone());
                let toasts = shell.toasts();
                confirm(
                    shell,
                    t!("ui.replay.context.open_in_game").into_owned(),
                    t!("confirm.open_in_game").into_owned(),
                    move || files::open_replay_in_game(toasts, &dir, &replay),
                );
            })) {
                {t!("ui.replay.context.open_in_game").into_owned()}
            }
        }
    });

    AnyView::new(view! {
        {batch_items.map(IntoView::into_view)}
        MenuItem(on_select = open(ReplayBrowserEvent::Open(path.clone()))) {{t!("ui.collab.open").into_owned()}}
        MenuItem(on_select = open(ReplayBrowserEvent::OpenInNewTab(path.clone()))) {
            {t!("ui.replay.context.open_in_new_tab").into_owned()}
        }
        {item(t!("ui.replay.context.render_replay").into_owned(), ReplayBrowserEvent::RenderReplay(path.clone()))}
        MenuSeparator()
        MenuItem(on_select = set_session) {{set_label}}
        {item(add_label, ReplayBrowserEvent::SessionStats { paths: session.clone(), replace: false })}
        MenuItem(on_select = reindex) {{force_reindex_label(count)}}
        MenuSeparator()
        MenuItem(on_select = UnsyncCallback::new(move |()| files::copy_replay_files(shell.toasts(), std::slice::from_ref(&copied)))) {
            {t!("ui.replay.context.copy_replay").into_owned()}
        }
        MenuItem(on_select = UnsyncCallback::new(move |()| files::copy_paths(shell, std::slice::from_ref(&listed)))) {
            {t!("ui.replay.context.copy_path").into_owned()}
        }
        MenuItem(on_select = UnsyncCallback::new(move |()| wows_toolkit_services::child_process::reveal_in_file_manager(&revealed))) {
            {t!("ui.replay.context.show_in_explorer").into_owned()}
        }
        {controls.map(IntoView::into_view)}
        {in_game.map(IntoView::into_view)}
    })
}

fn force_reindex_label(count: usize) -> String {
    if count == 1 {
        t!("ui.reindex.context_one").into_owned()
    } else {
        t!("ui.reindex.context_many", count = count).into_owned()
    }
}

/// Asks before running `then`, with a Cancel beside it.
fn confirm(shell: Shell, title: String, description: String, then: impl Fn() + 'static) {
    let then: Rc<dyn Fn()> = Rc::new(then);
    shell.dialogs.open(move |handle| {
        let open = zgui::reactive::RwSignal::new_local(true);
        let (title, description) = (StoredValue::new_local(title.clone()), StoredValue::new_local(description.clone()));
        let then = StoredValue::new_local(Rc::clone(&then));
        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "replay-listing-confirm") {
                    AlertDialogHeader {
                        AlertDialogTitle {{title.get_value()}}
                        AlertDialogDescription {{description.get_value()}}
                    }
                    AlertDialogFooter {
                        AlertDialogCancel {{t!("ui.buttons.no").into_owned()}}
                        AlertDialogAction(on:click = move |_| then.with_value(|then| then())) {{t!("ui.buttons.yes").into_owned()}}
                    }
                }
            }
        })
    });
}
