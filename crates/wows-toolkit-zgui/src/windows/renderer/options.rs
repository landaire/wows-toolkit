//! The gear on the transport: what the viewport draws, what an export is encoded with, and saving both as the defaults.

use landaire_ui::Toasts;
use rust_i18n::t;
use wows_minimap_renderer::VideoCodec;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::replay_render::export::encoder_status;
use wows_toolkit_replay_data::replay_render::options::TOGGLES;
use wows_toolkit_replay_data::replay_render::options::Toggle;
use zgui::component;
use zgui::css;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::LabelProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::view::TransportButtonProps;

/// The sheet every popover and reading of this slice installs: they share the check rows and the
/// panel look.
pub(super) const SHEET: &str = "wt-renderer-options";

pub(super) const CSS: &str = css!(
    ".wt-renderer-options__panel { width: 240px; max-height: 420px; overflow-y: auto; padding: 8px; }
    .wt-renderer-options__list { display: flex; flex-direction: column; gap: 2px; }
    .wt-renderer-options__check { display: flex; flex-direction: row; align-items: center; gap: 6px; padding: 2px 0; font-size: 12px; }
    .wt-renderer-options__heading { padding-top: 12px; font-size: 11px; font-weight: 700; color: var(--wt-color-text-dim); }
    .wt-renderer-options__caption { padding-top: 4px; font-size: 11px; color: var(--wt-color-text-dim); }
    .wt-renderer-options__codecs { display: flex; flex-direction: column; align-items: stretch; gap: 2px; }
    .wt-renderer-options__save { padding-top: 8px; }"
);

/// The gear. Refused until the bake lands: the options until then are the ones it is baking
/// under, not the reader's, and saving them as the defaults would store those over what they chose.
#[component]
pub fn OptionsPopover(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        if move || session.is_ready() {
            OptionsGear(session = session)
        } else {
            // Refused, so it is never pressed: only the gear's place and look are wanted.
            TransportButton(glyph = Glyph::Settings, label_key = "ui.renderer.settings.title",
                test_id = "replay-renderer-settings-toggle", disabled = Signal::stored_local(true),
                on_press = UnsyncCallback::new(|()| {}))
        }
    }
}

/// The gear once the track is here, and what it opens.
#[component]
fn OptionsGear(session: RendererSession) -> impl IntoView {
    let label = t!("ui.renderer.settings.title").into_owned();
    let named = label.clone();

    view! {
        Popover {
            PopoverTrigger(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconSm,
                attr:data-testid = "replay-renderer-settings-toggle",
                a11y:label = label.clone()
            ) {
                Tooltip {
                    TooltipTrigger {
                        AppIcon(glyph = Glyph::Settings)
                    }
                    TooltipContent {{named.clone()}}
                }
            }
            PopoverContent(
                placement = Placement::new(Side::Top, Align::End),
                class = "wt-renderer-options__panel",
                attr:data-testid = "replay-renderer-settings-list"
            ) {
                column(class = "wt-renderer-options__list") {
                    for toggle in move || TOGGLES.iter().collect::<Vec<_>>(), key = |toggle: &&Toggle| toggle.id {
                        LayerToggle(session = session, toggle = toggle)
                    }
                }
                ExportSettingsSection(session = session)
                SaveDefaultsButton(session = session)
            }
        }
    }
}

/// One of the display toggles.
#[component]
fn LayerToggle(session: RendererSession, toggle: &'static Toggle) -> impl IntoView {
    let checked = Signal::derive_local(move || {
        session.layers.with(|layers| (toggle.read)(&layers.options, layers.show_dead_ships))
    });
    let on_change = UnsyncCallback::new(move |on: bool| {
        session.set_layers(|layers| (toggle.write)(&mut layers.options, &mut layers.show_dead_ships, on));
    });

    view! {
        CheckRow(label = t!(toggle.label_key).into_owned(), test_id = toggle.id.to_owned(), checked = checked, on_change = on_change)
    }
}

/// What an export is encoded with, under the display toggles as the egui renderer arranges them.
#[component]
fn ExportSettingsSection(session: RendererSession) -> impl IntoView {
    let settings = move || session.export.with(|export| export.settings);
    let prefer_cpu = Signal::derive_local(move || settings().prefer_cpu);
    let pre_battle = Signal::derive_local(move || settings().include_pre_battle);
    let codecs: Vec<VideoCodec> = encoder_status().supported_codecs().collect();

    view! {
        column {
            text(class = "wt-renderer-options__heading") {{t!("ui.renderer.settings.export_settings").into_owned()}}
            CheckRow(
                label = t!("ui.renderer.settings.prefer_cpu").into_owned(),
                test_id = "replay-renderer-prefer-cpu".to_owned(),
                tooltip = t!("ui.renderer.settings.prefer_cpu_tooltip").into_owned(),
                checked = prefer_cpu,
                on_change = UnsyncCallback::new(move |on: bool| session.export.update(|export| export.settings.prefer_cpu = on))
            )
            CheckRow(
                label = t!("ui.renderer.settings.include_pre_battle").into_owned(),
                test_id = "replay-renderer-include-pre-battle".to_owned(),
                tooltip = t!("ui.renderer.settings.include_pre_battle_tooltip").into_owned(),
                checked = pre_battle,
                on_change = UnsyncCallback::new(move |on: bool| {
                    session.export.update(|export| export.settings.include_pre_battle = on);
                })
            )
            text(class = "wt-renderer-options__caption") {{t!("ui.renderer.settings.codec").into_owned()}}
            column(class = "wt-renderer-options__codecs") {
                CodecChoice(session = session, codec = None, test_id = "replay-renderer-codec-auto".to_owned())
                // Only what this machine can actually encode with.
                for codec in move || codecs.clone(), key = |codec: &VideoCodec| *codec {
                    CodecChoice(session = session, codec = Some(codec), test_id = format!("replay-renderer-codec-{}", codec as usize))
                }
            }
        }
    }
}

/// One codec to encode with, or `None` for the encoder's own pick.
#[component]
fn CodecChoice(session: RendererSession, codec: Option<VideoCodec>, test_id: String) -> impl IntoView {
    let picked = Signal::derive_local(move || session.export.with(|export| export.settings.codec == codec));
    let label = move || match codec {
        Some(codec) => codec.display_name().to_owned(),
        // Auto names the codec it would pick, so the choice is not blind.
        None => {
            let prefer_cpu = session.export.with(|export| export.settings.prefer_cpu);
            format!(
                "{} ({})",
                t!("ui.renderer.settings.codec_auto"),
                encoder_status().best_codec(prefer_cpu).display_name()
            )
        }
    };
    let named = label();
    let place = NodeRef::new();

    view! {
        box(node_ref = place, on:focus_in = move |_| reveal(place)) {
            Toggle(
                // Pressing the chosen one again leaves it chosen: one codec is always picked.
                pressed = Binding::controlled(picked, move |_| session.export.update(|export| export.settings.codec = codec)),
                size = ToggleSize::Sm,
                label = named,
                attr:data-testid = test_id
            ) {
                {label}
            }
        }
    }
}

/// Keeps what this viewport is showing as what the next one opens with.
#[component]
fn SaveDefaultsButton(session: RendererSession) -> impl IntoView {
    let toasts = Toasts::current();
    let place = NodeRef::new();

    view! {
        box(class = "wt-renderer-options__save", node_ref = place, on:focus_in = move |_| reveal(place)) {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "replay-renderer-save-defaults",
                on:click = move |_| session.spawn(async move {
                    let saved = session.save_defaults().await;
                    // A window with no message host has nowhere to say it.
                    let Some(toasts) = toasts else { return };
                    if saved {
                        toasts.ok(t!("ui.renderer.settings.save_defaults_done").into_owned());
                    } else {
                        toasts.failed(t!("ui.renderer.settings.save_defaults_failed").into_owned());
                    }
                })
            ) {
                {t!("ui.renderer.settings.save_defaults").into_owned()}
            }
        }
    }
}

/// A checkbox and its name, either of which toggles it, with an optional hover explaining it.
#[component]
pub(super) fn CheckRow(
    label: String,
    test_id: String,
    #[prop(into, optional)] tooltip: Option<String>,
    #[prop(into)] checked: Signal<bool, LocalStorage>,
    on_change: UnsyncCallback<bool>,
) -> impl IntoView {
    let place = NodeRef::new();
    let name = NodeRef::new();
    let control = NodeRef::new();
    let ticked = Signal::derive_local(move || Checked::from(checked.get()));
    let row = move || {
        view! {
            row(class = "wt-renderer-options__check", node_ref = place, on:focus_in = move |_| reveal(place)) {
                Checkbox(
                    checked = Binding::controlled(ticked, move |next: Checked| on_change.run(next.is_checked())),
                    node_ref = control,
                    labelled_by = name,
                    attr:data-testid = test_id.clone()
                )
                // A label only focuses its control; a press on the name here toggles it as well,
                // as the GPUI and egui checkboxes do.
                Label(node_ref = name, control = control, on:click = move |_| on_change.run(!checked.get_untracked())) {
                    {label.clone()}
                }
            }
        }
    };

    match tooltip {
        None => AnyView::new(row()),
        Some(tooltip) => AnyView::new(view! {
            Tooltip {
                TooltipTrigger {
                    {row()}
                }
                TooltipContent {{tooltip.clone()}}
            }
        }),
    }
}

/// Scrolls a control the keyboard moved to into its scrolled panel's view, which focus alone does
/// not do.
fn reveal(place: NodeRef) {
    place.scroll_to(ScrollTarget::IntoView, ScrollBehavior::Instant);
}
