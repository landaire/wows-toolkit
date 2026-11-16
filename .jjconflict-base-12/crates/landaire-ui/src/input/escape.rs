use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view::EventCx;
use zgui::view::NodeRef;
use zgui::view::events;
use zgui::view::focused_node;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

/// Escape in a filter field: it empties the field while the field holds text, and in an empty
/// field it goes on to whatever else answers Escape, such as the popover or drawer around it.
///
/// Give the field [`EscapeClears::node_ref`] and run [`EscapeClears::key_down`] from its key
/// handler. A surface that answers Escape before the field sees it, as a popover does, takes
/// [`EscapeClears::dismiss_on_escape`].
#[derive(Clone, Copy)]
pub struct EscapeClears {
    typed: Signal<bool, LocalStorage>,
    clear: UnsyncCallback<()>,
    field: NodeRef,
}

impl EscapeClears {
    /// For a field bound to `value`.
    pub fn new(value: RwSignal<String, LocalStorage>) -> Self {
        Self::with(Signal::derive_local(move || value.with(|text| !text.is_empty())), move || value.set(String::new()))
    }

    /// For a field whose text lives elsewhere: `typed` says whether it holds text, and `clear`
    /// empties it.
    pub fn with(typed: Signal<bool, LocalStorage>, clear: impl Fn() + 'static) -> Self {
        Self { typed, clear: UnsyncCallback::new(move |()| clear()), field: NodeRef::new() }
    }

    /// The handle to bind the field to.
    pub fn node_ref(self) -> NodeRef {
        self.field
    }

    /// Empties the field on a plain Escape while it holds text and stops the key there. Returns
    /// whether it did.
    pub fn key_down(self, ev: &mut EventCx<'_, events::KeyDown>) -> bool {
        if ev.key != Key::Named(NamedKey::Escape) || ev.modifiers != Modifiers::NONE || !self.typed.get_untracked() {
            return false;
        }
        self.clear.run(());
        ev.prevent_default();
        ev.stop_propagation();
        true
    }

    /// Whether Escape may close the surface around the field: not while the field has focus and
    /// holds text, since that Escape empties the field.
    pub fn dismiss_on_escape(self) -> Signal<bool, LocalStorage> {
        let focused = focused_node();
        let field = self.field;
        let typed = self.typed;
        Signal::derive_local(move || !(typed.get() && focused.get().is_some_and(|node| field.contains(node))))
    }
}
