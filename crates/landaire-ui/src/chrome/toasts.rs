use std::borrow::Cow;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::Children;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::Toast;
use zgui_ui::ToastCorner;
use zgui_ui::ToastId;
use zgui_ui::ToastKind;
use zgui_ui::ToastQueue;
use zgui_ui::ToasterProps;
use zgui_ui::use_toaster;

const SHEET: &str = "landaire-ui-toasts";

/// The mark sits on the message's first line rather than beside the middle of a message that
/// wraps; the title's line is one and a half times its size. The stack stays a list, each message
/// readable, rather than collapsing into a deck while the pointer is elsewhere: a message the
/// reader has to hover to read is one they miss.
const CSS: &str = css!(
    ".lui-toasts .zui-toast {
        align-items: flex-start;
    }
    .lui-toasts .zui-toast__icon {
        height: 1.5em;
    }
    .lui-toasts.zui-toast__region[data-expanded=\"false\"][data-corner=\"top-right\"] .zui-toast__slot,
    .lui-toasts.zui-toast__region[data-expanded=\"false\"][data-corner=\"top-left\"] .zui-toast__slot {
        transform: translateX(var(--zui-toast-swipe, 0px)) translateY(var(--zui-toast-offset, 0px));
    }
    .lui-toasts.zui-toast__region[data-expanded=\"false\"][data-corner=\"bottom-right\"] .zui-toast__slot,
    .lui-toasts.zui-toast__region[data-expanded=\"false\"][data-corner=\"bottom-left\"] .zui-toast__slot {
        transform: translateX(var(--zui-toast-swipe, 0px)) translateY(calc(-1 * var(--zui-toast-offset, 0px)));
    }
    .lui-toasts.zui-toast__region[data-expanded=\"false\"] .zui-toast__slot[data-front=\"false\"] .zui-toast > * {
        opacity: 1;
    }"
);

/// What a message is about, so the next message about the same thing replaces it and
/// [`Toasts::withdraw`] can take it down.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ToastKey(Cow<'static, str>);

impl ToastKey {
    pub fn new(key: impl Into<Cow<'static, str>>) -> Self {
        Self(key.into())
    }
}

/// How a message reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToastTone {
    /// Something asked for happened.
    Ok,
    /// Something asked for did not happen.
    Failed,
    /// Something worth reading that nobody asked for.
    Info,
    /// Something is wrong, but nothing was lost.
    Warn,
}

impl ToastTone {
    const fn kind(self) -> ToastKind {
        match self {
            Self::Ok => ToastKind::Success,
            Self::Failed => ToastKind::Error,
            Self::Info => ToastKind::Info,
            Self::Warn => ToastKind::Warning,
        }
    }
}

/// How long a message stays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastStay {
    /// A few seconds, or until dismissed.
    #[default]
    Briefly,
    /// Until it is dismissed or withdrawn, for a state the reader has to act on.
    UntilWithdrawn,
}

/// One message, built up and then shown with [`Toasts::show`].
#[derive(Clone, Debug)]
pub struct Notice {
    tone: ToastTone,
    message: String,
    key: Option<ToastKey>,
    stay: ToastStay,
    action: Option<NoticeAction>,
}

impl Notice {
    pub fn new(tone: ToastTone, message: impl Into<String>) -> Self {
        Self { tone, message: message.into(), key: None, stay: ToastStay::Briefly, action: None }
    }

    pub fn ok(message: impl Into<String>) -> Self {
        Self::new(ToastTone::Ok, message)
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(ToastTone::Failed, message)
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self::new(ToastTone::Info, message)
    }

    pub fn warn(message: impl Into<String>) -> Self {
        Self::new(ToastTone::Warn, message)
    }

    /// The same message about `key`: it replaces whatever message about `key` is up.
    pub fn keyed(self, key: ToastKey) -> Self {
        Self { key: Some(key), ..self }
    }

    /// The same message, staying until it is dismissed or withdrawn.
    pub fn sticky(self) -> Self {
        Self { stay: ToastStay::UntilWithdrawn, ..self }
    }

    /// The same message with a button that runs `run` and then takes the message down.
    pub fn action(self, label: impl Into<String>, run: impl Fn() + 'static) -> Self {
        Self { action: Some(NoticeAction { label: label.into(), run: std::rc::Rc::new(run) }), ..self }
    }

    fn into_toast(self) -> Toast {
        let toast = Toast::new(self.message).kind(self.tone.kind());
        let toast = match self.stay {
            ToastStay::Briefly => toast,
            ToastStay::UntilWithdrawn => toast.persistent(),
        };

        match self.action {
            Some(action) => toast.action(action.label, move || (action.run)()),
            None => toast,
        }
    }
}

/// The messages of one window, reached from anywhere inside its [`ToastHost`].
#[derive(Clone, Copy)]
pub struct Toasts {
    queue: ToastQueue,
    keyed: StoredValue<HashMap<ToastKey, ToastId>, LocalStorage>,
}

impl Toasts {
    /// The enclosing window's messages. Called from a component's body; `None` outside a
    /// [`ToastHost`].
    pub fn current() -> Option<Self> {
        use_local_context::<Self>()
    }

    pub fn ok(self, message: impl Into<String>) -> ToastId {
        self.show(Notice::ok(message))
    }

    pub fn failed(self, message: impl Into<String>) -> ToastId {
        self.show(Notice::failed(message))
    }

    pub fn info(self, message: impl Into<String>) -> ToastId {
        self.show(Notice::info(message))
    }

    pub fn warn(self, message: impl Into<String>) -> ToastId {
        self.show(Notice::warn(message))
    }

    /// Puts `notice` up. A keyed notice takes the place of the message up about the same key,
    /// which leaves as the new one arrives, so a burst of one event shows only its latest.
    pub fn show(self, notice: Notice) -> ToastId {
        let key = notice.key.clone();
        if let Some(key) = key.as_ref() {
            self.withdraw(key);
        }

        let id = self.queue.push(notice.into_toast());
        if let Some(key) = key {
            self.keyed.update_value(|keyed| {
                keyed.insert(key, id);
            });
        }

        id
    }

    /// Takes down the message about `key`, if one is up.
    pub fn withdraw(self, key: &ToastKey) {
        if let Some(id) = self.keyed.try_update_value(|keyed| keyed.remove(key)).flatten() {
            self.queue.dismiss(id);
        }
    }
}

/// A button on a message.
#[derive(Clone)]
struct NoticeAction {
    label: String,
    run: std::rc::Rc<dyn Fn()>,
}

impl std::fmt::Debug for NoticeAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("NoticeAction").field("label", &self.label).finish_non_exhaustive()
    }
}

/// Distinguishes one window's host from another's in [`HOSTS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HostId(u64);

thread_local! {
    /// Every host built on this thread, which is every window's.
    static HOSTS: RefCell<Vec<(HostId, Toasts)>> = const { RefCell::new(Vec::new()) };
    static NEXT_HOST: Cell<u64> = const { Cell::new(0) };
}

/// Where a window's messages appear, and what [`Toasts::current`] finds. One per window, around
/// the window's content.
#[component]
pub fn ToastHost(
    /// What the region is called, for a screen reader.
    #[prop(into, default = String::from("Notifications"))]
    label: String,
    /// What the control that dismisses a message is called, for a screen reader.
    #[prop(into, default = String::from("Dismiss"))]
    dismiss_label: String,
    /// How many messages may be up at once; the oldest leave to make room.
    #[prop(default = 3)]
    limit: usize,
    #[prop(default = ToastCorner::TopRight)] corner: ToastCorner,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        Toaster(
            corner = corner,
            limit = limit,
            label = label,
            dismiss_label = dismiss_label,
            class = "lui-toasts"
        ) {
            Publish {
                {children.into_view_once()}
            }
        }
    }
}

/// Takes down the message about `key` in every window that has one up.
pub fn withdraw_everywhere(key: &ToastKey) {
    let hosts: Vec<Toasts> = HOSTS.with(|hosts| hosts.borrow().iter().map(|(_, toasts)| *toasts).collect());
    for toasts in hosts {
        toasts.withdraw(key);
    }
}

/// Publishes the queue the enclosing toaster made, as [`Toasts`], to what it holds.
#[component]
fn Publish(children: Children) -> impl IntoView {
    if let Some(queue) = use_toaster() {
        let toasts = Toasts { queue, keyed: StoredValue::new_local(HashMap::new()) };
        provide_local_context(toasts);

        let id = HostId(NEXT_HOST.with(|next| next.replace(next.get() + 1)));
        HOSTS.with(|hosts| hosts.borrow_mut().push((id, toasts)));
        on_cleanup_local(move || HOSTS.with(|hosts| hosts.borrow_mut().retain(|(host, _)| *host != id)));
    }

    children.into_view_once()
}
