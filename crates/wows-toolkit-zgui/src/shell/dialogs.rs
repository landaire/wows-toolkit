//! Dialogs opened from anywhere in the window rather than declared beside what opens them: the
//! answer to a background job, the update offer, the crash report.

use std::rc::Rc;

use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Children;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui_ui::DialogProps;

/// Names one open dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DialogId(u64);

/// Builds what one dialog shows. Called once, when the dialog opens.
type BuildDialog = Rc<dyn Fn(DialogHandle) -> AnyView>;

#[derive(Clone)]
struct OpenDialog {
    id: DialogId,
    build: BuildDialog,
}

/// The window's hosted dialogs, newest on top.
#[derive(Clone, Copy)]
pub struct Dialogs {
    open: RwSignal<Vec<OpenDialog>, LocalStorage>,
    next: StoredValue<u64>,
}

impl Dialogs {
    pub fn new() -> Self {
        Self { open: RwSignal::new_local(Vec::new()), next: StoredValue::new(0) }
    }

    /// Opens a dialog whose content `build` makes. The content is a [`HostedDialog`] or a zgui-ui
    /// `AlertDialog` whose closing calls [`DialogHandle::close`].
    pub fn open(self, build: impl Fn(DialogHandle) -> AnyView + 'static) -> DialogHandle {
        let id = DialogId(self.next.get_value());
        self.next.update_value(|next| *next += 1);
        self.open.update(|open| open.push(OpenDialog { id, build: Rc::new(build) }));

        DialogHandle { dialogs: self, id }
    }

    /// How many hosted dialogs are open.
    pub fn count(self) -> usize {
        self.open.get().len()
    }
}

impl Default for Dialogs {
    fn default() -> Self {
        Self::new()
    }
}

/// One open dialog.
#[derive(Clone, Copy)]
pub struct DialogHandle {
    dialogs: Dialogs,
    id: DialogId,
}

impl DialogHandle {
    pub fn close(self) {
        let id = self.id;
        self.dialogs.open.update(|open| open.retain(|dialog| dialog.id != id));
    }

    /// Closes the dialog when the zgui-ui dialog it shows says it closed.
    pub fn on_open_change(self) -> UnsyncCallback<bool> {
        UnsyncCallback::new(move |open: bool| {
            if !open {
                self.close();
            }
        })
    }
}

/// A zgui-ui `Dialog` that is open for as long as it is hosted.
#[component]
pub fn HostedDialog(handle: DialogHandle, children: Children) -> impl IntoView {
    let open = RwSignal::new_local(true);

    view! {
        Dialog(open = open, on_open_change = handle.on_open_change()) {
            {children.into_view_once()}
        }
    }
}

/// Where the window's hosted dialogs are built.
#[component]
pub fn DialogHost(dialogs: Dialogs) -> impl IntoView {
    view! {
        for dialog in move || dialogs.open.get(), key = |dialog: &OpenDialog| dialog.id {
            {(dialog.build)(DialogHandle { dialogs, id: dialog.id })}
        }
    }
}
