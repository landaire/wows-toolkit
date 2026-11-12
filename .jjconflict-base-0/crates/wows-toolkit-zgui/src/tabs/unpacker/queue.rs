//! The extraction queue as a panel beside the listing, with every control that acts on it: the
//! destination, the queue's contents, and the button that writes them.

use landaire_ui::TABULAR_CLASS;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::unpacker::extract::PrototypeOutput;
use zgui::component;
use zgui::css;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::InputProps;
use zgui_ui::ProgressProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui::VirtualListProps;
use zgui_ui::prelude::Binding;

use crate::icons::AppIconProps;
use crate::tabs::unpacker::model::UnpackerModel;

const SHEET: &str = "wt-unpacker-queue";

const CSS: &str = css!(
    ".wt-queue { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-queue__destination {
        display: flex;
        flex-direction: column;
        flex: none;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-queue__dim { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-queue__faint {
        font-size: 12px;
        color: var(--wt-color-text-faint);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .wt-queue__row { display: flex; align-items: center; gap: 4px; }
    .wt-queue__field { flex: 1 1 auto; min-width: 0; }
    .wt-queue__heading { display: flex; flex: none; align-items: center; gap: 4px; padding: 4px 8px; }
    .wt-queue__count { flex: 1 1 auto; }
    .wt-queue__list { flex: 1 1 auto; min-height: 0; }
    .wt-queue__empty { padding: 8px; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-queue-entry { display: flex; align-items: center; gap: 4px; height: 24px; padding: 0 8px; }
    .wt-queue-entry:nth-child(even) { background-color: var(--lui-color-faint); }
    .wt-queue-entry__path {
        flex: 1 1 auto;
        min-width: 0;
        font-size: 12px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .wt-queue__run {
        display: flex;
        flex-direction: column;
        flex: none;
        gap: 4px;
        padding: 4px 8px;
        border-top: 1px solid var(--lui-color-border);
    }
    .wt-queue__extract { display: flex; flex: 1 1 auto; }
    .wt-queue__extract-button { flex: 1 1 auto; }
    .wt-queue__slot { height: 16px; min-width: 0; }
    .wt-queue__bar { height: 8px; }"
);

/// One queued entry's row.
const ENTRY_HEIGHT: f32 = 24.0;

#[component]
pub fn QueuePanel(model: UnpackerModel) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let count = Signal::derive_local(move || model.queue.with(|queue| queue.len()));
    let busy = Signal::derive_local(move || model.extract.with(|state| state.is_busy()));
    let has_output_dir = Signal::derive_local(move || !model.output_dir.with(String::is_empty));
    let decode = Signal::derive_local(move || Checked::from(model.prototypes.get() == PrototypeOutput::DecodeToJson));
    let extract_label = move || match count.get() {
        0 => t!("ui.unpacker.extract").into_owned(),
        1 => t!("ui.unpacker.extract_one").into_owned(),
        count => t!("ui.unpacker.extract_many", count = count).into_owned(),
    };
    let status = move || model.extract.with(|state| state.status()).unwrap_or_default();
    let fraction = Signal::derive_local(move || {
        model.extract.with(|state| state.progress().map(|progress| f64::from(progress.fraction()) * 100.0))
    });
    let writing = move || model.extract.with(|state| state.progress().map(|progress| progress.file.clone()));
    let entries = move || {
        if count.get() == 0 {
            return AnyView::new(view! {
                text(class = "wt-queue__empty", attr:data-testid = "unpacker-queue-empty") {
                    {t!("ui.unpacker.queue_empty").into_owned()}
                }
            });
        }
        AnyView::new(view! {
            VirtualList(
                count = count,
                row_size = ENTRY_HEIGHT,
                label = t!("ui.unpacker.extraction_queue").into_owned(),
                class = "wt-queue__list",
                attr:data-testid = "unpacker-queue-list",
                row = move |ix: usize| entry_row(model, ix)
            )
        })
    };
    let output_dir = model.output_dir;

    view! {
        column(class = "wt-queue", attr:data-testid = "unpacker-queue-panel") {
            column(class = "wt-queue__destination") {
                text(class = "wt-queue__dim") {{t!("ui.unpacker.extraction_output").into_owned()}}
                row(class = "wt-queue__row") {
                    Input(
                        value = output_dir,
                        placeholder = t!("ui.unpacker.output_dir_hint").into_owned(),
                        label = t!("ui.unpacker.extraction_output").into_owned(),
                        class = "wt-queue__field",
                        attr:data-testid = "unpacker-output-dir",
                        on:focus_out = move |_| model.store_output_dir(output_dir.get_untracked().trim().to_string()),
                        on:key_down = move |ev: &mut zgui::view::EventCx<'_, zgui::view::events::KeyDown>| {
                            if ev.key == zgui::vocab::Key::Named(zgui::vocab::NamedKey::Enter) {
                                model.store_output_dir(output_dir.get_untracked().trim().to_string());
                            }
                        }
                    )
                    Tooltip {
                        TooltipTrigger {
                            Button(
                                variant = ButtonVariant::Outline,
                                size = ButtonSize::IconSm,
                                a11y:label = t!("ui.unpacker.browse").into_owned(),
                                attr:data-testid = "unpacker-browse",
                                on:click = move |_| model.browse_for_output_dir()
                            ) {
                                AppIcon(glyph = Glyph::FolderOpen)
                            }
                        }
                        TooltipContent {{t!("ui.unpacker.browse").into_owned()}}
                    }
                }
                Tooltip {
                    TooltipTrigger {
                        row(class = "wt-queue__row") {
                            Checkbox(
                                checked = Binding::controlled(decode, move |checked: Checked| {
                                    model.prototypes.set(if checked == Checked::Yes {
                                        PrototypeOutput::DecodeToJson
                                    } else {
                                        PrototypeOutput::Raw
                                    });
                                }),
                                a11y:label = t!("ui.unpacker.decode_prototypes").into_owned(),
                                attr:data-testid = "unpacker-decode-prototypes"
                            )
                            text(class = "wt-queue__dim") {{t!("ui.unpacker.decode_prototypes").into_owned()}}
                        }
                    }
                    TooltipContent {{t!("ui.unpacker.decode_prototypes_tooltip").into_owned()}}
                }
            }
            row(class = "wt-queue__heading") {
                text(class = format!("wt-queue__dim wt-queue__count {TABULAR_CLASS}")) {
                    {move || t!("ui.unpacker.queued_count", count = count.get()).into_owned()}
                }
                Button(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::Xs,
                    disabled = Signal::derive_local(move || count.get() == 0 || busy.get()),
                    attr:data-testid = "unpacker-queue-clear-all",
                    on:click = move |_| model.clear_queue()
                ) {
                    {t!("ui.unpacker.clear_all").into_owned()}
                }
            }
            {entries}
            column(class = "wt-queue__run") {
                row(class = "wt-queue__row") {
                    Tooltip {
                        TooltipTrigger(class = "wt-queue__extract") {
                            Button(
                                size = ButtonSize::Sm,
                                class = "wt-queue__extract-button",
                                disabled = Signal::derive_local(move || count.get() == 0 || busy.get() || !has_output_dir.get()),
                                attr:data-testid = "unpacker-extract",
                                on:click = move |_| model.start_extraction()
                            ) {
                                {extract_label}
                            }
                        }
                        TooltipContent {
                            {move || if has_output_dir.get() {
                                extract_label()
                            } else {
                                t!("ui.unpacker.choose_directory").into_owned()
                            }}
                        }
                    }
                    Show(when = move || busy.get()) {
                        Button(
                            size = ButtonSize::Sm,
                            variant = ButtonVariant::Outline,
                            attr:data-testid = "unpacker-cancel",
                            on:click = move |_| model.cancel_extraction()
                        ) {
                            {t!("ui.buttons.cancel").into_owned()}
                        }
                    }
                }
                // Each slot is there whether or not a run is going, so a run starting does not
                // push the queue up the panel.
                text(class = "wt-queue__dim wt-queue__slot", attr:data-testid = "unpacker-extract-status") {{status}}
                box(class = "wt-queue__bar") {
                    Show(when = move || fraction.get().is_some()) {
                        Progress(value = fraction, attr:data-testid = "unpacker-extract-progress")
                    }
                }
                text(class = "wt-queue__faint wt-queue__slot", attr:data-testid = "unpacker-extract-file") {
                    {move || writing().unwrap_or_default()}
                }
            }
        }
    }
}

/// One queued entry. Read through the queue, since a row is kept by its position while entries
/// before it are removed.
fn entry_row(model: UnpackerModel, ix: usize) -> impl IntoView {
    let entry = move || model.queue.with(|queue| queue.entries().get(ix).cloned());
    let shown =
        move || entry().map(|entry| format!("res/{}", entry.as_str().trim_start_matches('/'))).unwrap_or_default();

    view! {
        row(class = "wt-queue-entry", attr:data-testid = format!("unpacker-queue-entry-{ix}")) {
            text(class = "wt-queue-entry__path") {{shown}}
            Tooltip {
                TooltipTrigger {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconXs,
                        a11y:label = t!("ui.unpacker.remove_from_queue").into_owned(),
                        attr:data-testid = format!("unpacker-queue-remove-{ix}"),
                        on:click = move |_| {
                            if let Some(entry) = model.queue.with_untracked(|queue| queue.entries().get(ix).cloned()) {
                                model.unqueue(&entry);
                            }
                        }
                    ) {
                        AppIcon(glyph = Glyph::X)
                    }
                }
                TooltipContent {{t!("ui.unpacker.remove_from_queue").into_owned()}}
            }
        }
    }
}
