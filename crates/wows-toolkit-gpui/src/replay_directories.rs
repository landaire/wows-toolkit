use super::*;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::tooltip::Tooltip;

const PINNED_DIRECTORIES: &str = "gpui.pinned_replay_directories";

pub(super) struct DirectoryTab {
    pub root: PathBuf,
    pub inspector: Entity<ReplayInspectorView>,
    pub pinned: bool,
    _subscriptions: Vec<Subscription>,
}

impl App {
    pub(super) fn active_replay_inspector(&self) -> Entity<ReplayInspectorView> {
        self.replay_directories
            .iter()
            .find(|tab| self.active_tab == AppTab::ReplayDirectory(tab.inspector.entity_id()))
            .map(|tab| tab.inspector.clone())
            .unwrap_or_else(|| self.replay_inspector.clone())
    }

    pub(super) fn adopt_directory_autoload(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings_mut() {
            settings.auto_load_latest_replay = enabled;
        }
        self.replay_inspector.update(cx, |view, cx| view.adopt_autoload(enabled, cx));
        for tab in &self.replay_directories {
            tab.inspector.update(cx, |view, cx| view.adopt_autoload(enabled, cx));
        }
    }
    pub(super) fn tab_order(&self) -> Vec<AppTab> {
        let mut tabs = vec![AppTab::ReplayInspector];
        tabs.extend(self.replay_directories.iter().map(|tab| AppTab::ReplayDirectory(tab.inspector.entity_id())));
        tabs.extend(AppTab::ALL.into_iter().skip(1));
        tabs
    }

    pub(super) fn directory_tab_label(
        &self,
        tab: AppTab,
        attention: bool,
        selected: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let directory =
            self.replay_directories.iter().find(|entry| tab == AppTab::ReplayDirectory(entry.inspector.entity_id()));
        let label = directory.map(|entry| shortened_path(&entry.root)).unwrap_or_else(|| tab.label());
        let row = h_flex()
            .gap_1()
            .items_center()
            .when(selected, |row| row.text_color(cx.theme().tab_active_foreground).font_weight(FontWeight::SEMIBOLD))
            .when(attention, |row| row.text_color(cx.theme().danger))
            .child(crate::icons::icon(tab.glyph()))
            .child(label);
        let Some(directory) = directory else { return row.into_any_element() };
        let id = directory.inspector.entity_id();
        let pinned = directory.pinned;
        let pins_ready = self.directories_loaded;
        let full_path = directory.root.display().to_string();
        let app = cx.entity();
        let close = cx.entity();
        row.id(SharedString::from(format!("replay-directory-{id:?}")))
            .tooltip(move |window, cx| Tooltip::new(full_path.clone()).build(window, cx))
            .child(if pinned {
                div().text_xs().child("[Pinned]").into_any_element()
            } else {
                Button::new(SharedString::from(format!("close-directory-{id:?}")))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip("Close directory")
                    .on_click(move |_event, _window, cx| {
                        cx.stop_propagation();
                        close.update(cx, |this, cx| this.close_directory_tab(id, cx));
                    })
                    .into_any_element()
            })
            .context_menu(move |menu, _window, _cx| {
                let pin = app.clone();
                let close = app.clone();
                menu.item(
                    PopupMenuItem::new(if pinned { "Unpin directory" } else { "Pin directory" })
                        .disabled(!pins_ready)
                        .on_click(move |_event, window, cx| {
                            pin.update(cx, |this, cx| this.pin_directory(id, !pinned, window, cx));
                        }),
                )
                .item(PopupMenuItem::new("Close directory").disabled(pinned).on_click(
                    move |_event, _window, cx| {
                        close.update(cx, |this, cx| this.close_directory_tab(id, cx));
                    },
                ))
            })
            .into_any_element()
    }

    pub(super) fn open_directory_tab(
        &mut self,
        root: PathBuf,
        pinned: bool,
        activate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Canonical paths identify aliases of an existing directory. Keep missing
        // paths so a pinned removable volume is not discarded on startup.
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        if let Some(tab) = self.replay_directories.iter_mut().find(|tab| tab.root == root) {
            tab.pinned |= pinned;
            if activate {
                self.active_tab = AppTab::ReplayDirectory(tab.inspector.entity_id());
            }
            cx.notify();
            return;
        }
        let inspector = cx.new(|cx| ReplayInspectorView::new(window, cx));
        let context = self.replay_inspector.read(cx).directory_context();
        let locale = self.settings().and_then(|settings| settings.locale.clone());
        let subscriptions = vec![
            cx.subscribe(&inspector, |this, _view, event: &crate::replay_inspector::view::AutoloadChanged, cx| {
                this.adopt_directory_autoload(event.0, cx);
            }),
            cx.subscribe_in(
                &inspector,
                window,
                |this, _view, event: &crate::replay_inspector::view::CheckGameDataRequested, window, cx| {
                    if event.0.is_empty() {
                        crate::toast::info("Game data is available for the listed replays.", window, cx);
                    } else {
                        for build in &event.0 {
                            this.offered_builds.remove(&build.build);
                        }
                        this.offer_missing_game_data(event.0.clone(), window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &inspector,
                window,
                |this, _view, event: &crate::replay_inspector::view::OpenDirectoryRequested, window, cx| {
                    this.open_directory_tab(event.0.clone(), false, true, window, cx);
                },
            ),
            cx.subscribe_in(&inspector, window, |this, _view, event: &GameDataMissing, window, cx| {
                this.offer_missing_game_data(event.0.clone(), window, cx);
            }),
            cx.subscribe_in(
                &inspector,
                window,
                |this, _view, event: &crate::replay_inspector::view::SearchDirectory, window, cx| {
                    this.search_directory(event.0.clone(), window, cx);
                },
            ),
            cx.subscribe(&inspector, |this, _view, event: &ReplaySettingsChanged, cx| {
                this.edit_replay_settings(cx, |settings| *settings = event.0.clone());
            }),
            cx.subscribe_in(&inspector, window, |this, _view, _: &ConstantsUnfit, window, cx| {
                this.forget_constants_commit(window, cx);
            }),
            cx.subscribe(&inspector, |this, _view, event: &crate::replay_inspector::view::ShowArmorRequested, cx| {
                this.armor_owner = Some(_view.downgrade());
                this.active_tab = AppTab::ArmorViewer;
                this.armor_pane.update(cx, |pane, cx| {
                    pane.show_with_hits(
                        event.param_index.clone(),
                        event.display_name.clone(),
                        event.hits.clone(),
                        event.incoming.clone(),
                        cx,
                    )
                });
                cx.notify();
            }),
            cx.subscribe(&inspector, |this, _view, event: &crate::replay_inspector::view::ArmorFollowed, cx| {
                if this.armor_owner.as_ref().is_some_and(|owner| owner.entity_id() != _view.entity_id()) {
                    return;
                }
                this.armor_pane
                    .update(cx, |pane, cx| pane.follow_hits(event.at, event.hits.clone(), event.health.clone(), cx));
            }),
            cx.subscribe(&inspector, |this, _view, event: &crate::replay_inspector::view::SessionShared, cx| {
                this.share_session_with_board(event.0.clone(), cx);
            }),
            cx.subscribe_in(
                &inspector,
                window,
                |this, _view, _: &crate::replay_inspector::view::TacticsBoardRequested, window, cx| {
                    this.open_tactics_board(window, cx);
                },
            ),
        ];
        inspector.update(cx, |view, cx| {
            view.configure_directory(context, locale, window, cx);
            view.list_directory(root.clone(), cx);
        });
        if activate {
            self.active_tab = AppTab::ReplayDirectory(inspector.entity_id());
        }
        self.replay_directories.push(DirectoryTab { root, inspector, pinned, _subscriptions: subscriptions });
        cx.notify();
    }

    fn close_directory_tab(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.replay_directories.retain(|tab| tab.inspector.entity_id() != id || tab.pinned);
        if self.active_tab == AppTab::ReplayDirectory(id)
            && !self.replay_directories.iter().any(|tab| tab.inspector.entity_id() == id)
        {
            self.active_tab = AppTab::ReplayInspector;
        }
        cx.notify();
    }

    fn pin_directory(&mut self, id: EntityId, pinned: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.directories_loaded {
            return;
        }
        let Some(pool) = settings_store::pool(cx) else {
            crate::toast::failed("The settings database is not available.", window, cx);
            return;
        };
        let Some(tab) = self.replay_directories.iter_mut().find(|tab| tab.inspector.entity_id() == id) else { return };
        tab.pinned = pinned;
        let paths: Vec<PathBuf> =
            self.replay_directories.iter().filter(|tab| tab.pinned).map(|tab| tab.root.clone()).collect();
        let lock = self.directory_save_lock.clone();
        cx.spawn_in(window, async move |this, cx| {
            let _guard = lock.lock().await;
            let result = runtime::spawn(cx, async move {
                wows_toolkit_config::queries::set_setting(&pool, PINNED_DIRECTORIES, &paths).await
            })
            .await;
            if !matches!(result, Ok(Ok(()))) {
                let _ = this.update_in(cx, |_this, window, cx| {
                    crate::toast::failed("Pinned replay directories could not be saved.", window, cx);
                });
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn restore_directories(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.directories_loaded || self.directories_restoring {
            return;
        }
        let Some(pool) = settings_store::pool(cx) else { return };
        self.directories_restoring = true;
        cx.spawn_in(window, async move |this, cx| {
            let stored = runtime::spawn(cx, async move {
                wows_toolkit_config::queries::try_get_setting::<Vec<PathBuf>>(&pool, PINNED_DIRECTORIES).await
            })
            .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.directories_restoring = false;
                this.directories_loaded = matches!(&stored, Ok(Ok(_)));
                match stored {
                    Ok(Ok(Some(paths))) => {
                        for path in paths {
                            this.open_directory_tab(path, true, false, window, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => crate::toast::failed("Pinned replay directories could not be read.", window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn sync_directory_game_data(&mut self, cx: &mut Context<Self>) {
        let context = self.replay_inspector.read(cx).directory_context();
        for tab in &self.replay_directories {
            tab.inspector.update(cx, |view, cx| view.update_directory_game_data(context.clone(), cx));
        }
    }
}

fn shortened_path(root: &std::path::Path) -> String {
    let parts: Vec<_> = root.components().collect();
    let mut result = PathBuf::new();
    for (index, component) in parts.iter().enumerate() {
        match component {
            std::path::Component::Normal(name) if index + 1 < parts.len() => {
                if let Some(initial) = name.to_string_lossy().chars().next() {
                    result.push(initial.to_string());
                }
            }
            std::path::Component::Normal(name) => {
                let name = name.to_string_lossy();
                let chars: Vec<_> = name.chars().collect();
                if chars.len() > 24 {
                    let head: String = chars[..12].iter().collect();
                    let tail: String = chars[chars.len() - 12..].iter().collect();
                    result.push(format!("{head}..{tail}"));
                } else {
                    result.push(name.as_ref());
                }
            }
            _ => result.push(component.as_os_str()),
        }
    }
    let path = result.display().to_string();
    path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
}
