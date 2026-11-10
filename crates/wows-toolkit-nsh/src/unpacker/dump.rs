//! Exporting a build's GameParams and writing one decoded assets.bin
//! prototype to a file.

use std::path::PathBuf;

use nsh_base::menu;
use nsh_base::menu_item;
use nsh_core::file_dialog::FileFilter;
use nsh_core::file_dialog::SaveDialog;
use nsh_core::prelude::*;
use wows_toolkit_viewmodel::unpacker::game_params;
use wows_toolkit_viewmodel::unpacker::game_params::GameParamsFormat;
use wows_toolkit_viewmodel::unpacker::viewer;
use wowsunpack::vfs::VfsPath;

use super::Tab;
use super::error::UnpackerError;
use super::load::BuildNumber;
use super::ui::ButtonStyle;
use super::ui::button;
use super::ui::caption;
use super::ui::class;
use super::ui::tip;

/// The build a dump reads.
#[derive(Clone)]
pub struct DumpSource {
    pub vfs: VfsPath,
    pub build: BuildNumber,
}

/// The export menu: each format of the whole tree, then the raw formats
/// again limited to the base entry. Disabled until a build is loaded,
/// rather than opening a menu whose every item would fail.
pub fn dump_menu(tab: Tab) -> AnyElement {
    let loaded = move || tab.dump_source_loaded();
    let item = move |format: GameParamsFormat, base_only: bool| {
        let label = format.label();
        let accessibility_label = if base_only {
            format!("{} {label}", tab.text("ui.unpacker.base_only"))
        } else {
            format!("{} {label}", tab.text("ui.unpacker.whole_tree"))
        };
        menu_item()
            .accessibility_label(accessibility_label)
            .app_class(class("min-h-7 px-2 rounded-(--radius) justify-center hover:bg-accent"))
            .on_select(move || dump_game_params(tab, format, base_only))
            .child(text(label).app_class(class("text-xs text-popover-foreground")))
            .into_element()
    };
    let heading =
        |label: String| text(label).app_class(class("px-2 pt-1 text-xs font-semibold text-popover-foreground"));

    let mut items = vec![heading(tab.text("ui.unpacker.whole_tree")).into_element()];
    items.extend(GameParamsFormat::ALL.into_iter().map(|format| item(format, false)));
    items.push(heading(tab.text("ui.unpacker.base_only")).into_element());
    // The base-entry cut only applies to the raw tree.
    items.extend(
        GameParamsFormat::ALL.into_iter().filter(|format| !format.is_minimal()).map(|format| item(format, true)),
    );

    menu()
        .trigger(
            button(tab.text("ui.unpacker.dump_parameters"), ButtonStyle::Secondary)
                .disabled(move || !loaded())
                .tooltip(tip(move || {
                    if loaded() {
                        tab.text("ui.unpacker.dump_parameters_tooltip")
                    } else {
                        tab.text("ui.unpacker.load_a_build")
                    }
                })),
        )
        .popup_class("min-w-52 p-1 gap-1 rounded-(--radius) border border-border bg-popover")
        .children(items)
        .into_element()
}

/// Asks where to save, then writes the build's parameters in `format` off
/// the engine thread.
fn dump_game_params(tab: Tab, format: GameParamsFormat, base_only: bool) {
    let Some(source) = tab.dump_source() else {
        tab.set_status(tab.text("ui.unpacker.load_a_build"));
        return;
    };
    // The minimal formats write the toolkit's own decoded set, which is the
    // same file for the whole tree; "base parameters only" cuts the raw tree.
    let base_only = base_only && !format.is_minimal();
    let stem = if format.is_minimal() { "MinGameParams" } else { "GameParams" };
    let file_name = format!("{stem}.{}", format.extension());

    spawn(async move {
        let Some(path) = pick_save(&file_name, format.label(), format.extension(), "Save game parameters", tab).await
        else {
            return;
        };
        tab.set_status("Writing...".to_owned());

        let written = blocking(move || write_game_params(&source, &path, format, base_only))
            .await
            .map_err(|source| UnpackerError::Task { source })
            .and_then(|written| written);
        tab.set_status(match written {
            Ok(()) => tab.text("ui.unpacker.parameters_written"),
            Err(error) => error.to_string(),
        });
    });
}

fn write_game_params(
    source: &DumpSource,
    path: &std::path::Path,
    format: GameParamsFormat,
    base_only: bool,
) -> Result<(), UnpackerError> {
    // The minimal formats write the toolkit's own decoded parameters, read
    // from the shared cache when this build was decoded before.
    if format.is_minimal() {
        let params = wows_toolkit_replay_data::load::load_game_params(&source.vfs, source.build.0)
            .map_err(|source_error| UnpackerError::DecodeParams { build: source.build, source: source_error })?;
        game_params::write_value(&params, path, format)?;
        return Ok(());
    }

    game_params::dump_pickled(&source.vfs, path, format, base_only)?;
    Ok(())
}

/// Decodes an assets.bin prototype and writes the JSON where the user asks.
pub fn extract_as_json(tab: Tab, path: VfsPath) {
    let name = path.filename();
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem).to_owned();

    spawn(async move {
        let title = tab.text("ui.unpacker.extract_as_json");
        let Some(target) = pick_save(&format!("{stem}.json"), "JSON", "json", &title, tab).await else {
            return;
        };

        let written = blocking(move || {
            let json = viewer::decode_to_json(&path)?;
            std::fs::write(&target, json).map_err(|source| UnpackerError::Write { path: target.clone(), source })
        })
        .await
        .map_err(|source| UnpackerError::Task { source })
        .and_then(|written| written);
        tab.set_status(match written {
            Ok(()) => tab.text("ui.unpacker.prototype_written"),
            Err(error) => error.to_string(),
        });
    });
}

/// A save dialog for one file; `None` when the user cancels, or when the
/// dialog fails, which the status line reports.
async fn pick_save(file_name: &str, filter: &str, extension: &str, title: &str, tab: Tab) -> Option<PathBuf> {
    let picked = pick_save_path(
        SaveDialog::new()
            .file_name(file_name.to_owned())
            .filter(FileFilter::new(filter.to_owned(), [extension.to_owned()]))
            .title(title.to_owned())
            .button_label("Save"),
    )
    .await;

    match picked {
        Ok(path) => path,
        Err(reason) => {
            tab.set_status(UnpackerError::Dialog { reason }.to_string());
            None
        }
    }
}

/// The status of the last export, beside the menu.
pub fn dump_status(tab: Tab) -> AnyElement {
    caption(move || tab.status_text()).accessible(true).role(AccessibilityRole::Alert).into_element()
}
