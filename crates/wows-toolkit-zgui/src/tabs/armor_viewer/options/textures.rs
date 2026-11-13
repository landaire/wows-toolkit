//! The textures section: each material of the scene's meshes with the textures it references, their
//! format and size, and the files in the packages that hold them.
//!
//! The game looks a texture up by its path, so a file at the same path under the build's mod
//! folder replaces it. A texture row's menu copies that path, shows the file in the Resource
//! Unpacker and extracts the packaged files to start from. Hovering a material row highlights its
//! meshes; the material under the pointer in the viewport is marked here, and the viewport menu
//! opens the section at it.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_armor::hull_picking::HullMeshIndex;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::textures::LocatedTexture;
use wows_toolkit_armor::textures::PropertyTexture;
use wows_toolkit_armor::textures::SceneMaterial;
use wows_toolkit_armor::textures::SceneTextures;
use wows_toolkit_armor::textures::TextureLookup;
use wows_toolkit_armor::visibility::SidebarHighlightKey;
use wows_toolkit_viewmodel::formatting::byte_size;
use wows_toolkit_viewmodel::unpacker::extract::ExtractError;
use wowsunpack::export::material_textures::PropertyName;
use wowsunpack::export::material_textures::TextureFiles;
use wowsunpack::export::material_textures::TextureFilesError;
use wowsunpack::game_data::mod_folder;
use wowsunpack::vfs::VfsPath;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Track;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateUntracked;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::view::install_stylesheet;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogDescriptionProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;
use zgui_ui::ContextMenuContentProps;
use zgui_ui::ContextMenuProps;
use zgui_ui::ContextMenuTriggerProps;
use zgui_ui::MenuItemProps;

use crate::services::GameDataState;
use crate::services::Job;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::palette::TabRequest;
use crate::tabs::armor_viewer::library::ModelsState;
use crate::tabs::armor_viewer::options::SectionCx;
use crate::tabs::armor_viewer::options::filter::MarkedLabel;
use crate::tabs::armor_viewer::options::filter::RailFilter;
use crate::tabs::armor_viewer::options::visibility::chevron;
use crate::tabs::armor_viewer::options::visibility::hoverable_row;
use crate::tabs::armor_viewer::options::visibility::no_ship_loaded;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;

const SHEET: &str = "wt-armor-textures";

const CSS: &str = css!(
    ".wt-armor-textures { display: flex; flex-direction: column; gap: 4px; width: 100%; }
    .wt-armor-textures__dim { font-size: 12px; color: var(--wt-color-text-dim); white-space: normal; }
    .wt-armor-textures__pointer { font-size: 12px; font-weight: 600; color: var(--lui-color-accent); }
    .wt-armor-textures__material { display: flex; flex-direction: column; gap: 2px; border-radius: 4px; }
    .wt-armor-textures__material[data-hovered] { background-color: var(--lui-color-selection); }
    .wt-armor-textures__head { display: flex; align-items: center; gap: 4px; min-width: 0; }
    .wt-armor-textures__name {
        min-width: 0;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        font-size: 13px;
    }
    .wt-armor-textures__count { flex: none; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-textures__children { display: flex; flex-direction: column; gap: 4px; padding-left: 20px; }
    .wt-armor-textures__texture { display: flex; flex-direction: column; min-width: 0; padding: 2px 4px; border-radius: 4px; }
    .wt-armor-textures__texture:hover { background-color: var(--lui-color-card); }
    .wt-armor-textures__role { font-size: 11px; color: var(--wt-color-text-dim); }
    .wt-armor-textures__file {
        min-width: 0;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        font-size: 13px;
    }
    .wt-armor-textures__trigger { display: flex; flex-direction: column; width: 100%; }"
);

/// A mesh whose material the section opens at, as the viewport menu chose it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TextureFocus {
    pub viewport: ArmorViewportState,
    /// The scene the mesh belongs to, by identity as [`ArmorViewportState::scene_key`] names it.
    pub scene: usize,
    pub mesh: HullMeshIndex,
}

impl TextureFocus {
    /// The mesh under `viewport`'s pointer, when it has one.
    pub fn at_pointer(viewport: ArmorViewportState) -> Option<Self> {
        let mesh = viewport.hovered_hull_mesh_untracked()?;
        let armor = viewport.shown_untracked()?;

        Some(Self { viewport, scene: Arc::as_ptr(&armor) as usize, mesh })
    }
}

/// The viewport menu's entry that opens the section at the material under the pointer.
pub fn show_textures_item(workspace: ArmorWorkspace, viewport: ArmorViewportState) -> Option<AnyView> {
    let focus = TextureFocus::at_pointer(viewport)?;

    Some(AnyView::new(view! {
        MenuItem(
            attr:data-testid = "armor-menu-show-textures",
            on_select = UnsyncCallback::new(move |()| workspace.show_textures(focus))
        ) {
            {t!("ui.armor.textures.show_textures").into_owned()}
        }
    }))
}

/// The section's body: `None` when the rail's filter keeps nothing in it.
pub fn body(cx: &SectionCx) -> Option<AnyView> {
    install_stylesheet(SHEET, CSS);
    let filter = cx.filter.clone();
    let Some(armor) = cx.viewport.shown_untracked() else {
        return (!filter.is_active()).then(no_ship_loaded);
    };

    if filter.is_active() {
        // The rail is built again once the textures are read, so the filter can match them.
        cx.workspace.texture_reads.track();
        let Some(inventory) = armor.texture_inventory.get() else {
            read_inventory(cx.workspace, &armor);
            return None;
        };
        if !inventory.materials().iter().any(|material| MaterialRows::of(material, &filter).is_some()) {
            return None;
        }
    }

    let reads = cx.workspace.texture_reads;
    let ready = {
        let armor = Arc::clone(&armor);
        Memo::new(move |_| {
            reads.track();
            armor.texture_inventory.get().is_some()
        })
    };
    if armor.texture_inventory.get().is_none() {
        read_inventory(cx.workspace, &armor);
    }
    let section = Section {
        workspace: cx.workspace,
        viewport: cx.viewport,
        shell: Shell::current(),
        armor,
        filter,
        ready,
        expanded: RwSignal::new_local(HashSet::new()),
        focused: RwSignal::new_local(None),
        focused_row: NodeRef::new(),
    };
    follow_focus(&section);

    let hovered = {
        let section = section.clone();
        Memo::new(move |_| {
            if !section.ready.get() {
                return None;
            }
            let mesh = section.viewport.hovered_hull_mesh()?;
            section.armor.texture_inventory.get()?.material_of(mesh)
        })
    };
    let pointer = {
        let section = section.clone();
        move || {
            let index = hovered.get()?;
            let material = section.armor.texture_inventory.get()?.materials().get(index)?;
            let said = t!("ui.armor.textures.under_pointer", material = material_name(material)).into_owned();
            Some(
                view! { text(class = "wt-armor-textures__pointer", attr:data-testid = "armor-textures-pointer") {{said}} },
            )
        }
    };
    let rows = move || section.rows(hovered);

    Some(AnyView::new(view! {
        column(class = "wt-armor-textures", attr:data-testid = "armor-textures") {
            text(class = "wt-armor-textures__dim") {{t!("ui.armor.textures.hint").into_owned()}}
            {pointer}
            {rows}
        }
    }))
}

/// What the section's rows are drawn from.
#[derive(Clone)]
struct Section {
    workspace: ArmorWorkspace,
    viewport: ArmorViewportState,
    shell: Shell,
    armor: Arc<LoadedShipArmor>,
    filter: RailFilter,
    /// Set once the scene's textures have been read.
    ready: Memo<bool>,
    /// The open materials, by position in the scene's list.
    expanded: RwSignal<HashSet<usize>, LocalStorage>,
    /// The material the viewport menu opened the section at, scrolled into view once drawn.
    focused: RwSignal<Option<usize>, LocalStorage>,
    focused_row: NodeRef,
}

impl Section {
    fn rows(&self, hovered: Memo<Option<usize>>) -> Vec<AnyView> {
        if !self.ready.get() {
            let waiting = match self.workspace.library.models() {
                ModelsState::Failed => t!("ui.models.unavailable"),
                ModelsState::NotStarted | ModelsState::Loading | ModelsState::Ready(_) => {
                    t!("ui.armor.textures.reading")
                }
            };
            return vec![AnyView::new(view! {
                text(class = "wt-armor-textures__dim", attr:data-testid = "armor-textures-reading") {{waiting.into_owned()}}
            })];
        }
        let Some(inventory) = self.armor.texture_inventory.get() else {
            return Vec::new();
        };
        let expanded = self.expanded.get();
        let focused = self.focused.get();

        let mut rows = vec![summary(inventory)];
        for (index, material) in inventory.materials().iter().enumerate() {
            let Some(listed) = MaterialRows::of(material, &self.filter) else {
                continue;
            };
            let open = self.filter.is_active() || expanded.contains(&index);
            rows.push(self.material_row(index, material, listed, open, focused == Some(index), hovered));
        }

        rows
    }

    fn material_row(
        &self,
        index: usize,
        material: &SceneMaterial,
        listed: MaterialRows,
        open: bool,
        focused: bool,
        hovered: Memo<Option<usize>>,
    ) -> AnyView {
        let MaterialRows { label, textures } = listed;
        let expanded = self.expanded;
        let caret = chevron(
            format!("armor-textures-expand-{index}"),
            open,
            (!self.filter.is_active()).then(|| {
                Box::new(move || {
                    expanded.update(|expanded| {
                        if !expanded.remove(&index) {
                            expanded.insert(index);
                        }
                    });
                }) as Box<dyn Fn()>
            }),
        );
        let title = label.view();
        let count = textures.len().to_string();
        let head = AnyView::new(view! {
            row(class = "wt-armor-textures__head") {
                {caret}
                box(class = "wt-armor-textures__name") {{title}}
                text(class = "wt-armor-textures__count") {{count}}
            }
        });
        let head = hoverable_row(
            format!("armor-textures-material-{index}"),
            self.viewport,
            SidebarHighlightKey::HullMeshes(material.mesh_names.clone()),
            head,
        );
        let head = match material.mfm_path.clone() {
            Some(path) => {
                let shell = self.shell;
                with_menu(head, move || vec![copy_item(shell, "armor-textures-copy-material", vec![path.clone()])])
            }
            None => head,
        };

        let children = open.then(|| {
            let rows: Vec<AnyView> = textures
                .into_iter()
                .enumerate()
                .map(|(position, texture)| {
                    self.texture_row(format!("armor-textures-texture-{index}-{position}"), texture)
                })
                .collect();
            view! { column(class = "wt-armor-textures__children") {{rows}} }
        });
        let node = if focused { self.focused_row } else { NodeRef::new() };

        AnyView::new(view! {
            column(
                class = "wt-armor-textures__material",
                node_ref = node,
                attr:data-hovered = move || (hovered.get() == Some(index)).then(String::new)
            ) {
                {head}
                {children}
            }
        })
    }

    fn texture_row(&self, test_id: String, texture: TextureRow) -> AnyView {
        let TextureRow { role, file, details, tooltip, path, files } = texture;
        let file = file.view();
        let content = AnyView::new(view! {
            column(class = "wt-armor-textures__texture", attr:data-testid = test_id) {
                text(class = "wt-armor-textures__role") {{role}}
                Tooltip {
                    TooltipTrigger {
                        box(class = "wt-armor-textures__file") {{file}}
                    }
                    TooltipContent {{tooltip.clone()}}
                }
                text(class = "wt-armor-textures__dim") {{details}}
            }
        });
        let Some(path) = path else {
            return content;
        };

        let (shell, workspace) = (self.shell, self.workspace);
        let items = move || {
            let mut items = vec![copy_item(shell, "armor-textures-copy-path", vec![path.clone()])];
            if !files.is_empty() {
                items.push(override_item(shell, workspace.services, files.clone()));
                items.push(unpacker_item(shell, path.clone()));
                items.push(extract_item(shell, workspace, files.clone()));
            }
            items
        };

        with_menu(content, items)
    }
}

/// One material's label and the texture rows the filter keeps, `None` when it keeps nothing.
struct MaterialRows {
    label: MarkedLabel,
    textures: Vec<TextureRow>,
}

impl MaterialRows {
    fn of(material: &SceneMaterial, filter: &RailFilter) -> Option<Self> {
        let name = material_name(material);
        let named = filter.keeps(&name);
        let within = filter.within(&name);
        let textures: Vec<TextureRow> = texture_rows(material)
            .into_iter()
            .filter(|row| within.keeps(&row.role) || row.path.as_deref().is_some_and(|path| within.keeps(path)))
            .map(|row| TextureRow { file: within.marked(row.file.text.clone()), ..row })
            .collect();
        if filter.is_active() && !named && textures.is_empty() {
            return None;
        }

        Some(Self { label: filter.marked(name), textures })
    }
}

/// What one texture row shows and acts on.
struct TextureRow {
    /// What the texture is for.
    role: String,
    /// The texture's file name.
    file: MarkedLabel,
    /// Format, size and file count, or why they are unknown.
    details: String,
    /// The full path, then each file with its size.
    tooltip: String,
    /// The referenced VFS path, `None` when the path store lacks it.
    path: Option<String>,
    /// Every file holding the texture, largest first. Empty when none is in the packages.
    files: Vec<String>,
}

/// The rows of `material`'s textures, the name-matched albedo last.
fn texture_rows(material: &SceneMaterial) -> Vec<TextureRow> {
    let properties = material.properties.iter().flatten().map(|PropertyTexture { property, texture }| {
        let role = role(*property);
        match texture {
            TextureLookup::Found(located) => located_row(role, located),
            TextureLookup::Unresolved(path_id) => {
                let unresolved = t!("ui.armor.textures.unresolved").into_owned();
                TextureRow {
                    role,
                    file: MarkedLabel::plain(format!("0x{path_id:016X}")),
                    tooltip: unresolved.clone(),
                    details: unresolved,
                    path: None,
                    files: Vec::new(),
                }
            }
        }
    });
    let matched = material
        .name_matched_albedo
        .iter()
        .map(|located| located_row(t!("ui.armor.textures.role_name_matched").into_owned(), located));

    properties.chain(matched).collect()
}

fn located_row(role: String, located: &LocatedTexture) -> TextureRow {
    let file = located.path.rsplit('/').next().unwrap_or(&located.path).to_string();
    let mut tooltip = located.path.clone();
    let (details, files) = match &located.files {
        Ok(files) => {
            for file in &files.files {
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                tooltip.push_str(&format!("\n{name}  {}  {}", file.size, byte_size(file.bytes)));
            }
            (details(files), files.files.iter().map(|file| file.path.clone()).collect())
        }
        Err(TextureFilesError::Absent { .. }) => (t!("ui.armor.textures.absent").into_owned(), Vec::new()),
        // The header is unreadable but the file is there to override.
        Err(TextureFilesError::Header { path }) => {
            (t!("ui.armor.textures.unreadable").into_owned(), vec![path.clone()])
        }
    };

    TextureRow { role, file: MarkedLabel::plain(file), details, tooltip, path: Some(located.path.clone()), files }
}

/// "BC7RgbaUnorm, 1024x1024, 2 files, 1.3 MiB": what the game shows at its highest texture
/// quality, and what the files take.
fn details(files: &TextureFiles) -> String {
    let format = files.format.map_or_else(|| t!("ui.armor.textures.unknown_format").into_owned(), |f| f.to_string());
    // The list always holds the referenced file, so it has a largest.
    let size = files.largest().map(|size| size.to_string()).unwrap_or_default();
    let bytes = byte_size(files.files.iter().map(|file| file.bytes).sum());

    match files.files.len() {
        1 => t!("ui.armor.textures.details_one", format = format, size = size, bytes = bytes).into_owned(),
        count => {
            t!("ui.armor.textures.details", format = format, size = size, count = count, bytes = bytes).into_owned()
        }
    }
}

/// What a texture property is for, in the reader's words where the property is a common one.
fn role(property: PropertyName) -> String {
    let key = match property {
        PropertyName::Known("diffuseMap") => "ui.armor.textures.role_albedo",
        PropertyName::Known("normalMap") => "ui.armor.textures.role_normal",
        PropertyName::Known("metallicGlossMap") => "ui.armor.textures.role_metal_gloss",
        PropertyName::Known("ambientOcclusionMap") => "ui.armor.textures.role_ambient_occlusion",
        PropertyName::Known("detailMap") => "ui.armor.textures.role_detail",
        PropertyName::Known(_) | PropertyName::Unknown(_) => return property.to_string(),
    };

    format!("{} ({property})", t!(key))
}

/// The material's file name, or a stand-in for meshes with none.
fn material_name(material: &SceneMaterial) -> String {
    match &material.mfm_path {
        Some(path) => path.rsplit('/').next().unwrap_or(path).to_string(),
        None => t!("ui.armor.textures.no_material").into_owned(),
    }
}

/// "12 materials, 40 textures", counting each texture once however many materials share it.
fn summary(inventory: &SceneTextures) -> AnyView {
    let textures: HashSet<&str> =
        inventory.materials().iter().flat_map(SceneMaterial::located).map(|located| located.path.as_str()).collect();
    let said = t!("ui.armor.textures.summary", materials = inventory.materials().len(), textures = textures.len())
        .into_owned();

    AnyView::new(view! { text(class = "wt-armor-textures__dim", attr:data-testid = "armor-textures-summary") {{said}} })
}

/// Reads `armor`'s textures off the UI thread once the model library is ready, which is what
/// resolves the materials. A scene is read once, however many rails ask for it.
fn read_inventory(workspace: ArmorWorkspace, armor: &Arc<LoadedShipArmor>) {
    let (library, services, reads) = (workspace.library, workspace.services, workspace.texture_reads);
    let armor = Arc::clone(armor);
    let reading = RenderEffect::new(move |started: Option<bool>| {
        if started == Some(true) {
            return true;
        }
        let ModelsState::Ready(models) = library.models() else {
            return false;
        };
        if armor.texture_inventory.get().is_some() {
            return true;
        }
        let scene = Arc::as_ptr(&armor) as usize;
        // Untracked: only the end of a read changes what a reader shows.
        if !reads.try_update_untracked(|reads| reads.insert(scene)).unwrap_or(false) {
            return true;
        }
        let armor = Arc::clone(&armor);
        spawn_local(async move {
            let job = services.jobs.start(Job { name: t!("ui.armor.textures.reading").into_owned(), progress: None });
            blocking(move || {
                armor.textures(&models.library);
            })
            .await;
            drop(job);
            reads.try_update(|reads| {
                reads.remove(&scene);
            });
        });

        true
    });
    on_cleanup_local(move || drop(reading));
}

/// Opens the material the viewport menu asked for, once the textures are read, and scrolls it into
/// view once it is drawn. The focus is kept, so a rebuilt rail opens it again.
fn follow_focus(section: &Section) {
    let opening = {
        let section = section.clone();
        RenderEffect::new(move |_| {
            if !section.ready.get() {
                return;
            }
            let Some(focus) = section.workspace.texture_focus.get() else { return };
            if !focus.viewport.is(section.viewport) || focus.scene != Arc::as_ptr(&section.armor) as usize {
                return;
            }
            let Some(index) =
                section.armor.texture_inventory.get().and_then(|inventory| inventory.material_of(focus.mesh))
            else {
                return;
            };
            section.expanded.update(|expanded| {
                expanded.insert(index);
            });
            section.focused.set(Some(index));
        })
    };
    let (focused, row) = (section.focused, section.focused_row);
    let scrolling = RenderEffect::new(move |_| {
        if focused.get().is_some() && row.get().is_some() {
            row.scroll_to(ScrollTarget::IntoViewStart, ScrollBehavior::Instant);
        }
    });
    on_cleanup_local(move || {
        drop(opening);
        drop(scrolling);
    });
}

/// `content` as the trigger of a context menu holding what `items` builds as it opens.
fn with_menu(content: AnyView, items: impl Fn() -> Vec<AnyView> + 'static) -> AnyView {
    let items: Rc<dyn Fn() -> Vec<AnyView>> = Rc::new(items);

    AnyView::new(view! {
        ContextMenu {
            ContextMenuTrigger(class = "wt-armor-textures__trigger") {{content}}
            ContextMenuContent {
                {shared(&items)}
            }
        }
    })
}

/// A builder over `items` that the menu content can hold.
fn shared(items: &Rc<dyn Fn() -> Vec<AnyView>>) -> impl Fn() -> Vec<AnyView> + 'static {
    let items = Rc::clone(items);

    move || items()
}

/// Copies `paths`, one per line.
fn copy_item(shell: Shell, test_id: &'static str, paths: Vec<String>) -> AnyView {
    AnyView::new(view! {
        MenuItem(attr:data-testid = test_id, on_select = UnsyncCallback::new(move |()| copy_paths(shell, &paths))) {
            {t!("ui.armor.textures.copy_path").into_owned()}
        }
    })
}

/// Copies where each of `files` goes to replace the packaged one.
fn override_item(shell: Shell, services: Services, files: Vec<String>) -> AnyView {
    let label = match files.len() {
        1 => t!("ui.armor.textures.copy_override").into_owned(),
        count => t!("ui.armor.textures.copy_overrides", count = count).into_owned(),
    };
    let copy = move |()| match override_paths(services, &files) {
        Ok(paths) => copy_paths(shell, &paths),
        Err(reason) => {
            shell.toasts().warn(reason);
        }
    };

    AnyView::new(view! {
        MenuItem(attr:data-testid = "armor-textures-copy-override", on_select = UnsyncCallback::new(copy)) {{label}}
    })
}

fn unpacker_item(shell: Shell, path: String) -> AnyView {
    let reveal = move |()| shell.palette.send(shell, TabRequest::RevealInUnpacker { path: path.clone() });

    AnyView::new(view! {
        MenuItem(attr:data-testid = "armor-textures-show-in-unpacker", on_select = UnsyncCallback::new(reveal)) {
            {t!("ui.armor.textures.show_in_unpacker").into_owned()}
        }
    })
}

fn extract_item(shell: Shell, workspace: ArmorWorkspace, files: Vec<String>) -> AnyView {
    let extract = move |()| {
        let Some(bundle) = workspace.library.ready_untracked() else { return };
        let vfs = bundle.assets.vfs().clone();
        let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.armor.textures.extract_title"));
        let files = files.clone();
        let services = workspace.services;
        spawn_local(async move {
            let Some(folder) = shell.chosen_path(asked).await else { return };
            let placements = match placements(&files, &folder) {
                Ok(placements) => placements,
                Err(err) => {
                    shell.toasts().warn(t!("ui.armor.textures.extract_failed", reason = err.to_string()));
                    return;
                }
            };
            let existing = placements.iter().filter(|placement| placement.destination.exists()).count();
            let extraction = Extraction { shell, services, vfs, folder, placements };
            match existing {
                0 => extraction.run(),
                existing => confirm_replace(extraction, existing),
            }
        });
    };

    AnyView::new(view! {
        MenuItem(attr:data-testid = "armor-textures-extract", on_select = UnsyncCallback::new(extract)) {
            {t!("ui.armor.textures.extract").into_owned()}
        }
    })
}

/// Asks before `extraction` replaces the `existing` files already in its folder, which may be
/// the reader's own edits.
fn confirm_replace(extraction: Extraction, existing: usize) {
    let shell = extraction.shell;
    let folder = extraction.folder.display().to_string();
    shell.dialogs.open(move |handle| {
        let open = RwSignal::new_local(true);
        let description = t!("ui.armor.textures.replace_description", count = existing, folder = folder).into_owned();
        let description = StoredValue::new_local(description);
        let extraction = StoredValue::new_local(extraction.clone());
        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "armor-textures-replace-dialog") {
                    AlertDialogHeader {
                        AlertDialogTitle {{t!("ui.armor.textures.replace_title").into_owned()}}
                        AlertDialogDescription {{description.get_value()}}
                    }
                    AlertDialogFooter {
                        AlertDialogCancel {{t!("ui.buttons.cancel").into_owned()}}
                        AlertDialogAction(attr:data-testid = "armor-textures-replace-confirm", on:click = move |_| extraction.get_value().run()) {
                            {t!("ui.armor.textures.replace").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

/// Texture files chosen for extraction and where they go.
#[derive(Clone)]
struct Extraction {
    shell: Shell,
    services: Services,
    vfs: VfsPath,
    folder: PathBuf,
    placements: Vec<Placement>,
}

impl Extraction {
    fn run(self) {
        let Self { shell, services, vfs, folder, placements } = self;
        spawn_local(async move {
            let job =
                services.jobs.start(Job { name: t!("ui.armor.textures.extracting").into_owned(), progress: None });
            let written = blocking(move || write_placements(&vfs, &placements)).await;
            drop(job);
            match written {
                Ok(count) => {
                    let folder = folder.display().to_string();
                    shell.toasts().ok(t!("ui.armor.textures.extracted", count = count, folder = folder));
                }
                Err(err) => {
                    shell.toasts().warn(t!("ui.armor.textures.extract_failed", reason = err.to_string()));
                }
            }
        });
    }
}

fn copy_paths(shell: Shell, paths: &[String]) {
    let said = match paths {
        [path] => t!("ui.models.path_copied", path = path).into_owned(),
        paths => t!("ui.armor.textures.paths_copied", count = paths.len()).into_owned(),
    };
    shell.copy_text(paths.join("\n"));
    shell.toasts().ok(said);
}

/// Where each of `files` goes in the mod folder of the build the game data was read from.
fn override_paths(services: Services, files: &[String]) -> Result<Vec<String>, String> {
    let wows_dir = services
        .settings
        .read_untracked(|settings| settings.wows_dir.clone())
        .filter(|dir| !dir.is_empty())
        .ok_or_else(|| t!("ui.armor.load_settings").into_owned())?;
    let GameDataState::Ready(loaded) = services.game_data.state_untracked() else {
        return Err(t!("ui.armor.textures.no_build").into_owned());
    };
    let folder = mod_folder(Path::new(&wows_dir), loaded.build()).map_err(|err| err.to_string())?;

    placements(files, &folder)
        .map(|placements| placements.iter().map(|placement| placement.destination.display().to_string()).collect())
        .map_err(|err| err.to_string())
}

/// `vfs_path` under `folder`, joined part by part so the separators are the platform's. `None`
/// unless every part is a plain name, so the result cannot leave `folder` on any platform.
fn override_path(folder: &Path, vfs_path: &str) -> Option<PathBuf> {
    let mut path = folder.to_path_buf();
    let mut parts = 0;
    for part in vfs_path.split('/').filter(|part| !part.is_empty()) {
        if part == "." || part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        path.push(part);
        parts += 1;
    }

    (parts > 0).then_some(path)
}

/// A texture file and where extraction writes it.
#[derive(Clone)]
struct Placement {
    vfs_path: String,
    destination: PathBuf,
}

/// Where each of `files` lands under `folder`, at its VFS path.
fn placements(files: &[String], folder: &Path) -> Result<Vec<Placement>, TextureExtractError> {
    files
        .iter()
        .map(|file| {
            let destination =
                override_path(folder, file).ok_or_else(|| TextureExtractError::UnsafePath { path: file.clone() })?;
            Ok(Placement { vfs_path: file.clone(), destination })
        })
        .collect()
}

/// Why texture files could not be extracted.
#[derive(Debug, thiserror::Error)]
enum TextureExtractError {
    #[error("{path} is not a plain path inside the game data")]
    UnsafePath { path: String },
    #[error(transparent)]
    Extract(#[from] ExtractError),
}

/// Writes each file of `placements` from `vfs`, returning how many were written. A file is
/// written beside its destination and then moved over it, so a failed write leaves a file
/// already there as it was.
fn write_placements(vfs: &VfsPath, placements: &[Placement]) -> Result<usize, TextureExtractError> {
    for Placement { vfs_path, destination } in placements {
        let read_source = |source| ExtractError::ReadSource { path: vfs_path.clone(), source };
        let mut reader = vfs.join(vfs_path).and_then(|file| file.open_file()).map_err(read_source)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|source| ExtractError::CreateDir { path: parent.to_path_buf(), source })?;
        }
        let mut partial = destination.clone().into_os_string();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        let written = fs::File::create(&partial)
            .map_err(|source| ExtractError::CreateFile { path: partial.clone(), source })
            .and_then(|mut writer| {
                io::copy(&mut reader, &mut writer)
                    .map_err(|source| ExtractError::Write { path: partial.clone(), source })
            })
            .and_then(|_| {
                fs::rename(&partial, destination)
                    .map_err(|source| ExtractError::Write { path: destination.clone(), source })
            });
        if let Err(err) = written {
            // The write's failure is the one to report; a partial file left behind is harmless.
            let _ = fs::remove_file(&partial);
            return Err(err.into());
        }
    }

    Ok(placements.len())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn an_override_path_mirrors_the_game_path_under_the_mod_folder() {
        let folder = Path::new("wows").join("bin").join("1").join("res_mods");
        let expected: PathBuf = ["wows", "bin", "1", "res_mods", "content", "ship", "hull_a.dds"].iter().collect();

        assert_eq!(override_path(&folder, "content/ship/hull_a.dds"), Some(expected.clone()));
        assert_eq!(override_path(&folder, "/content/ship/hull_a.dds"), Some(expected));
    }

    #[test]
    fn an_override_path_never_leaves_the_mod_folder() {
        let folder = Path::new("res_mods");
        for path in ["../hull_a.dds", "content/./hull_a.dds", "content\\..\\..\\hull_a.dds", "C:/hull_a.dds", "/", ""] {
            assert_eq!(override_path(folder, path), None, "{path:?} is refused");
        }
    }

    #[test]
    fn extracted_files_land_at_their_game_paths_replacing_what_is_there() {
        let vfs = VfsPath::new(wowsunpack::vfs::MemoryFS::new());
        for (path, bytes) in [("content/ship/hull_a.dd0", b"large"), ("content/ship/hull_a.dds", b"small")] {
            let file = vfs.join(path).unwrap();
            file.parent().create_dir_all().unwrap();
            file.create_file().unwrap().write_all(bytes).unwrap();
        }
        let folder = tempfile::tempdir().unwrap();
        let dds = folder.path().join("content/ship/hull_a.dds");
        std::fs::create_dir_all(dds.parent().unwrap()).unwrap();
        std::fs::write(&dds, b"an older edit").unwrap();
        let files = ["content/ship/hull_a.dd0".to_string(), "content/ship/hull_a.dds".to_string()];

        let written = write_placements(&vfs, &placements(&files, folder.path()).unwrap()).unwrap();

        assert_eq!(written, 2);
        assert_eq!(std::fs::read(folder.path().join("content/ship/hull_a.dd0")).unwrap(), b"large");
        assert_eq!(std::fs::read(&dds).unwrap(), b"small");
        assert!(!folder.path().join("content/ship/hull_a.dds.part").exists(), "no partial file is left");
    }

    #[test]
    fn a_failed_extraction_keeps_the_file_already_there() {
        let vfs = VfsPath::new(wowsunpack::vfs::MemoryFS::new());
        let folder = tempfile::tempdir().unwrap();
        let dds = folder.path().join("content/ship/hull_a.dds");
        std::fs::create_dir_all(dds.parent().unwrap()).unwrap();
        std::fs::write(&dds, b"an older edit").unwrap();
        let files = ["content/ship/hull_a.dds".to_string()];

        let failed = write_placements(&vfs, &placements(&files, folder.path()).unwrap());

        assert!(failed.is_err(), "the file is not in the game data");
        assert_eq!(std::fs::read(&dds).unwrap(), b"an older edit");
    }
}
