use std::cell::RefCell;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::time::Duration;

use accesskit_consumer::Node;
use accesskit_consumer::Tree;
use accesskit_consumer::TreeChangeHandler;
use zgui::geom::Css;
use zgui::geom::CssPx;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::geom::Size;
use zgui::platform::DecorationSource;
use zgui::platform::PlatformCx;
use zgui::platform::Surface;
use zgui::platform::SurfaceEvent;
use zgui::platform::WakeReason;
use zgui::prelude::IntoView;
use zgui::prelude::Zoom;
use zgui::render::RenderTarget;
use zgui::render::Renderer;
use zgui::runtime::AppError;
use zgui::runtime::Runtime;
use zgui::view::AttrName;
use zgui::view::ClassName;
use zgui::view::NodeId;
use zgui::view::OverlayLayer;
use zgui::vocab::KeyCode;
use zgui::vocab::KeyEvent;
use zgui::vocab::KeyState;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::PhysicalKey;
use zgui::vocab::PointerAction;
use zgui::vocab::PointerButton;
use zgui::vocab::PointerEvent;
use zgui::vocab::PointerId;
use zgui::vocab::PointerKind;
use zgui::vocab::Role;
use zgui::vocab::ScrollDelta;
use zgui::vocab::ScrollPhase;
use zgui::vocab::Timestamp;
use zgui::vocab::Toggled;
use zgui::vocab::WheelEvent;
use zgui_platform_headless::Harness;
use zgui_platform_headless::Headless;
use zgui_platform_headless::OffscreenSurface;
use zgui_render_wgpu::Builder;
use zgui_render_wgpu::Pixels;
use zgui_render_wgpu::WgpuRenderer;
use zgui_render_wgpu::wgpu;
use zgui_testkit_scene::CaptureRenderer;

use crate::error::HeadlessError;
use crate::palette::Appearance;

/// The attribute [`HeadlessWindow::by_test_id`] looks elements up by.
pub const TEST_ID_ATTRIBUTE: &str = "data-testid";

/// One frame of a sixty-hertz output, the step the virtual clock moves in.
const FRAME: Duration = Duration::from_micros(16_667);

/// How many turns of the loop running pending frames may take before it is left as it is.
const SETTLE_TURNS: u32 = 64;

/// A window's size in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
}

/// Device pixels per CSS pixel on the display, before zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayScale(pub f64);

/// What a headless window draws through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawing {
    /// Records each frame's scene and draws nothing. Needs no graphics device.
    Recorded,
    /// Draws on this machine's graphics device into a texture that can be read back.
    Device,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadlessOptions {
    pub size: WindowSize,
    pub scale: DisplayScale,
    pub zoom: Zoom,
    pub drawing: Drawing,
    /// The appearance the desktop reports, which `prefers-color-scheme` matches. `None` is a
    /// desktop that reports none.
    pub desktop: Option<Appearance>,
}

impl HeadlessOptions {
    pub fn new(size: WindowSize) -> Self {
        Self { size, scale: DisplayScale(1.0), zoom: Zoom::ONE, drawing: Drawing::Recorded, desktop: None }
    }
}

/// An element of the document a [`HeadlessWindow`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Element(pub NodeId);

/// What a screen reader is told about an element.
#[derive(Clone, Debug, PartialEq)]
pub struct A11yState {
    pub role: Role,
    pub label: Option<String>,
    pub value: Option<String>,
    pub selected: Option<bool>,
    pub checked: Option<Toggled>,
    pub expanded: Option<bool>,
    pub disabled: bool,
    pub focused: bool,
}

/// An application window over the headless platform, driven the way a compositor drives one,
/// with a virtual clock that moves only when told to.
pub struct HeadlessWindow {
    harness: Harness<Runtime>,
    scale: DisplayScale,
    pointer: Option<Point<CssPx, Css>>,
    a11y: Option<Tree>,
    exclusive: Exclusive,
}

impl HeadlessWindow {
    /// Opens `view` in a window and runs the frames that asks for.
    ///
    /// Any number of windows may be open on one thread. Windows on different threads open one
    /// at a time: the call waits while another thread has one open.
    pub fn open<F, V>(options: HeadlessOptions, view: F) -> Result<Self, HeadlessError>
    where
        F: FnMut() -> V + 'static,
        V: IntoView,
    {
        let exclusive = Exclusive::take();

        let factory: zgui::runtime::RendererFactory = match options.drawing {
            Drawing::Recorded => Box::new(|_: &Arc<dyn Surface>, _: RenderTarget| {
                Ok(Box::new(CaptureRenderer::new()) as Box<dyn Renderer>)
            }),
            Drawing::Device => Box::new(device_renderer),
        };
        let runtime = zgui::app()
            .with_size(options.size.width, options.size.height)
            .with_zoom(options.zoom)
            .with_renderer(factory)
            .into_handler(view)
            .map_err(|source| HeadlessError::App { source })?
            .into_runtime();

        let platform = Headless::new();
        platform.set_color_scheme(options.desktop.map(|appearance| match appearance {
            Appearance::Light => zgui::platform::ColorScheme::Light,
            Appearance::Dark => zgui::platform::ColorScheme::Dark,
        }));
        let mut harness = Harness::over(platform, runtime);
        if harness.app().windows().is_empty() {
            return Err(HeadlessError::NoWindow);
        }

        let scale = options.scale;
        let device = Size::new(
            DevicePx((f64::from(options.size.width) * scale.0) as f32),
            DevicePx((f64::from(options.size.height) * scale.0) as f32),
        );
        harness.deliver_to_first(SurfaceEvent::ScaleFactorChanged { scale_factor: scale.0, size: device });
        harness.settle(SETTLE_TURNS);

        let mut window = Self { harness, scale, pointer: None, a11y: None, exclusive };
        window.redraw_whole();

        Ok(window)
    }

    /// Moves the virtual clock on by `total` one output frame at a time, running the frames each
    /// step asks for, so a transition is sampled as a display would sample it.
    pub fn advance(&mut self, total: Duration) {
        self.wake_if_unrouted();

        let steps = total.as_nanos().div_ceil(FRAME.as_nanos()).max(1);
        for _ in 0..steps {
            self.harness.advance(FRAME);
            self.harness.settle(SETTLE_TURNS);
        }
    }

    /// The first element that carries [`TEST_ID_ATTRIBUTE`] set to `id`, overlays included.
    pub fn by_test_id(&self, id: &str) -> Option<Element> {
        self.by_attribute(TEST_ID_ATTRIBUTE, id)
    }

    /// The first element whose attribute `name` is `value`, overlays included, for a part a
    /// component marks with an attribute.
    pub fn by_attribute(&self, name: &str, value: &str) -> Option<Element> {
        let dom = self.window().dom();
        let attribute = AttrName::new(name);

        self.first_in_document(|node| dom.attribute(node, attribute).as_deref() == Some(value))
    }

    /// Every element whose attribute `name` is `value`, overlays included, in document order.
    pub fn all_by_attribute(&self, name: &str, value: &str) -> Vec<Element> {
        let dom = self.window().dom();
        let attribute = AttrName::new(name);

        let mut found = Vec::new();
        let mut pending = self.roots();
        pending.reverse();
        while let Some(node) = pending.pop() {
            if dom.attribute(node, attribute).as_deref() == Some(value) {
                found.push(Element(node));
            }
            pending.extend(dom.children(node).into_iter().rev());
        }

        found
    }

    /// The first element whose class list holds `class`, overlays included, for a part a
    /// component builds itself.
    pub fn by_class(&self, class: &str) -> Option<Element> {
        let dom = self.window().dom();
        let class = ClassName::new(class);

        self.first_in_document(|node| dom.classes(node).contains(&class))
    }

    /// Every element whose class list holds `class`, overlays included, in document order.
    pub fn all_by_class(&self, class: &str) -> Vec<Element> {
        let dom = self.window().dom();
        let class = ClassName::new(class);

        let mut seen = HashSet::new();
        let mut found = Vec::new();
        let mut pending = self.roots();
        pending.reverse();
        while let Some(node) = pending.pop() {
            if !seen.insert(node) {
                continue;
            }
            if dom.classes(node).contains(&class) {
                found.push(Element(node));
            }
            pending.extend(dom.children(node).into_iter().rev());
        }

        found
    }

    /// The text of every element whose class list holds `class`, overlays included, in document
    /// order.
    pub fn texts_by_class(&self, class: &str) -> Vec<String> {
        let dom = self.window().dom();
        let class = ClassName::new(class);

        // An overlay band's root may also lie below the page root, so a node is read once.
        let mut seen = HashSet::new();
        let mut texts = Vec::new();
        let mut pending = self.roots();
        pending.reverse();
        while let Some(node) = pending.pop() {
            if !seen.insert(node) {
                continue;
            }
            if dom.classes(node).contains(&class) {
                texts.push(dom.text_content(node));
            }
            pending.extend(dom.children(node).into_iter().rev());
        }

        texts
    }

    /// The first element, in reading order, whose accessible name is `label`.
    pub fn by_label(&mut self, label: &str) -> Option<Element> {
        self.find_a11y(|node| node.label().as_deref() == Some(label)).into_iter().next()
    }

    /// Every element with `role`, in reading order.
    pub fn by_role(&mut self, role: Role) -> Vec<Element> {
        self.find_a11y(|node| node.role() == role)
    }

    /// The first element with `role` whose accessible name is `label`.
    pub fn by_role_and_label(&mut self, role: Role, label: &str) -> Option<Element> {
        self.find_a11y(|node| node.role() == role && node.label().as_deref() == Some(label)).into_iter().next()
    }

    /// Where `element` is, in device pixels from the window's top-left corner. `None` when it
    /// produced no box.
    pub fn bounds(&self, element: Element) -> Option<Rect<DevicePx, Device>> {
        self.window().host_handle().window_box(element.0)
    }

    /// How many transitions and animations are running on `element`.
    pub fn running_animations(&self, element: Element) -> usize {
        self.window().host_handle().running_animations(element.0)
    }

    /// What a screen reader is told about `element`, or `None` when it is not in the
    /// accessibility tree.
    pub fn a11y(&mut self, element: Element) -> Option<A11yState> {
        let target = element.0.as_u64();
        let tree = self.a11y_tree()?;
        let state = tree.state();
        let mut pending = vec![state.root()];
        while let Some(node) = pending.pop() {
            if node.locate().0.0 == target {
                let data = node.data();
                return Some(A11yState {
                    role: node.role(),
                    label: node.label(),
                    value: node.value(),
                    selected: node.is_selected(),
                    checked: node.toggled(),
                    expanded: data.is_expanded(),
                    disabled: node.is_disabled(),
                    focused: node.is_focused(),
                });
            }
            pending.extend(node.children());
        }

        None
    }

    /// Moves the pointer over the middle of `element`.
    pub fn hover(&mut self, element: Element) -> Result<(), HeadlessError> {
        let at = self.centre(element)?;
        self.move_pointer(at);

        Ok(())
    }

    /// Moves the pointer to `at`, in device pixels from the window's top-left corner.
    pub fn hover_at(&mut self, at: Point<DevicePx, Device>) {
        let at = self.to_css(at);
        self.move_pointer(at);
    }

    /// Turns the wheel over the middle of `element`, asking to scroll `down` CSS pixels.
    pub fn wheel(&mut self, element: Element, down: f32) -> Result<(), HeadlessError> {
        let at = self.centre(element)?;
        self.move_pointer(at);

        let event = WheelEvent {
            id: PointerId::MOUSE,
            kind: PointerKind::Mouse,
            position: at,
            delta: ScrollDelta::Pixels(Size::new(CssPx(0.0), CssPx(down))),
            phase: ScrollPhase::Discrete,
        };
        self.deliver(SurfaceEvent::Wheel { event, modifiers: Modifiers::NONE, timestamp: Timestamp::ORIGIN });

        Ok(())
    }

    /// Presses and releases the primary button over the middle of `element`.
    pub fn click(&mut self, element: Element) -> Result<(), HeadlessError> {
        self.click_with(element, Modifiers::NONE)
    }

    /// Presses and releases the primary button over the middle of `element` with `modifiers` held.
    pub fn click_with(&mut self, element: Element, modifiers: Modifiers) -> Result<(), HeadlessError> {
        self.press_on(element, PointerButton::Primary, modifiers)
    }

    /// Presses and releases the primary button at `at`, in device pixels from the window's
    /// top-left corner, for a point other than an element's middle.
    pub fn click_at(&mut self, at: Point<DevicePx, Device>) {
        self.click_at_with(at, Modifiers::NONE);
    }

    /// Presses and releases the primary button at `at` with `modifiers` held.
    pub fn click_at_with(&mut self, at: Point<DevicePx, Device>, modifiers: Modifiers) {
        let at = self.to_css(at);
        self.move_pointer(at);
        self.pointer_event_with(PointerAction::Pressed, at, Some(PointerButton::Primary), modifiers);
        self.pointer_event_with(PointerAction::Released, at, Some(PointerButton::Primary), modifiers);
    }

    /// Clicks the primary button twice over the middle of `element`, close enough together to
    /// count as a double click.
    pub fn double_click(&mut self, element: Element) -> Result<(), HeadlessError> {
        self.click(element)?;
        self.click(element)
    }

    /// Presses and releases the secondary button over the middle of `element`.
    pub fn right_click(&mut self, element: Element) -> Result<(), HeadlessError> {
        self.press_on(element, PointerButton::Secondary, Modifiers::NONE)
    }

    /// Presses and releases the middle button over the middle of `element`.
    pub fn middle_click(&mut self, element: Element) -> Result<(), HeadlessError> {
        self.press_on(element, PointerButton::Middle, Modifiers::NONE)
    }

    /// Presses the primary button over the middle of `element`, moves the pointer to `to` in
    /// `steps` moves, one output frame apart, and releases it there.
    pub fn drag(&mut self, element: Element, to: Point<DevicePx, Device>, steps: u32) -> Result<(), HeadlessError> {
        let from = self.centre(element)?;
        let to = self.to_css(to);
        self.move_pointer(from);
        self.pointer_event(PointerAction::Pressed, from, Some(PointerButton::Primary));

        let steps = steps.max(1);
        for step in 1..=steps {
            let travel = step as f32 / steps as f32;
            let at = Point::new(
                CssPx(from.x.0 + (to.x.0 - from.x.0) * travel),
                CssPx(from.y.0 + (to.y.0 - from.y.0) * travel),
            );
            self.move_pointer(at);
            self.advance(FRAME);
        }

        self.pointer_event(PointerAction::Released, to, Some(PointerButton::Primary));

        Ok(())
    }

    /// Types `text` wherever the keyboard is, one character at a time.
    pub fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            self.key(KeyEvent::character(character.to_string()), Modifiers::NONE);
        }
    }

    /// Presses and releases a named key with `modifiers` held.
    pub fn press_key(&mut self, key: NamedKey, modifiers: Modifiers) {
        self.key(KeyEvent::named(key, physical_key(key)), modifiers);
    }

    /// Presses and releases the key that produces `character`, with `modifiers` held, as a
    /// shortcut is pressed.
    pub fn press_character(&mut self, character: char, modifiers: Modifiers) {
        self.key(KeyEvent::character(character.to_string()), modifiers);
    }

    /// The window's composed pixels, read back off the graphics device.
    pub fn pixels(&mut self) -> Result<Pixels, HeadlessError> {
        self.redraw_whole();

        let renderer = self.window().renderer().as_any().and_then(|any| any.downcast_ref::<WgpuRenderer>());
        let Some(renderer) = renderer else {
            return Err(HeadlessError::NotDrawnOnDevice);
        };

        Ok(renderer.read_composed())
    }

    /// Writes the window's composed pixels to `path` as a PNG.
    pub fn save_png(&mut self, path: &Path) -> Result<(), HeadlessError> {
        let pixels = self.pixels()?;
        crate::test_support::shot::write_png(&pixels, path)
    }

    /// Changes who the platform says draws the window's frame, as a compositor that answers again
    /// would.
    pub fn set_decoration_source(&mut self, source: DecorationSource) -> Result<(), HeadlessError> {
        let Some(surface) = self.surface() else {
            return Err(HeadlessError::NoWindow);
        };

        surface.set_decoration_source(source);
        self.deliver(SurfaceEvent::DecorationSourceChanged(source));

        Ok(())
    }

    /// Whether the window is still open.
    pub fn is_open(&self) -> bool {
        !self.harness.app().windows().is_empty()
    }

    fn window(&self) -> &zgui::runtime::Window {
        &self.harness.app().windows()[0]
    }

    fn surface(&self) -> Option<Arc<OffscreenSurface>> {
        self.harness.platform().offscreens().into_iter().next()
    }

    /// The first node in document order, overlays included, that `matches`.
    fn first_in_document(&self, matches: impl Fn(NodeId) -> bool) -> Option<Element> {
        let dom = self.window().dom();

        let mut pending = self.roots();
        while let Some(node) = pending.pop() {
            if matches(node) {
                return Some(Element(node));
            }
            pending.extend(dom.children(node).into_iter().rev());
        }

        None
    }

    /// The page root and each overlay band's root, since overlays are not below the page.
    fn roots(&self) -> Vec<NodeId> {
        let window = self.window();
        let page = window.dom().root_node();
        let handle = window.dom_handle();
        let mut roots = vec![page];
        for layer in OverlayLayer::ALL {
            let root = handle.overlay_root(page, *layer);
            if !roots.contains(&root) {
                roots.push(root);
            }
        }

        roots
    }

    /// Makes the window draw a whole frame, the redraw a surface owes when it is shown again.
    fn redraw_whole(&mut self) {
        self.harness.deliver_to_first(SurfaceEvent::Occluded(true));
        self.harness.settle(SETTLE_TURNS);
        self.harness.deliver_to_first(SurfaceEvent::Occluded(false));
        self.harness.settle(SETTLE_TURNS);
        self.advance(FRAME);
    }

    /// The accessibility tree as a screen reader holds it, with every update published so far
    /// applied.
    fn a11y_tree(&mut self) -> Option<&Tree> {
        let updates = self.surface().map(|surface| surface.take_a11y_log()).unwrap_or_default();
        for update in updates {
            match self.a11y.as_mut() {
                Some(tree) => tree.update_and_process_changes(update, &mut Silent),
                // A consumer starts from an update that carries the whole tree; anything earlier
                // cannot be applied to nothing.
                None if update.tree.is_some() => self.a11y = Some(Tree::new(update, true)),
                None => {}
            }
        }

        self.a11y.as_ref()
    }

    fn find_a11y(&mut self, matches: impl Fn(&Node<'_>) -> bool) -> Vec<Element> {
        let Some(tree) = self.a11y_tree() else {
            return Vec::new();
        };

        let mut found = Vec::new();
        let mut pending = vec![tree.state().root()];
        while let Some(node) = pending.pop() {
            if matches(&node)
                && let Some(id) = NodeId::from_u64(node.locate().0.0)
            {
                found.push(Element(id));
            }
            let children: Vec<Node<'_>> = node.children().collect();
            pending.extend(children.into_iter().rev());
        }

        found
    }

    fn centre(&self, element: Element) -> Result<Point<CssPx, Css>, HeadlessError> {
        let Some(bounds) = self.bounds(element) else {
            return Err(HeadlessError::NoBox { element: element.0 });
        };

        Ok(self.to_css(Point::new(
            DevicePx(bounds.origin.x.0 + bounds.size.width.0 / 2.0),
            DevicePx(bounds.origin.y.0 + bounds.size.height.0 / 2.0),
        )))
    }

    /// `at` in the CSS pixels the platform reports input in, which the window maps through its
    /// zoom itself.
    fn to_css(&self, at: Point<DevicePx, Device>) -> Point<CssPx, Css> {
        let scale = self.scale.0 as f32;

        Point::new(CssPx(at.x.0 / scale), CssPx(at.y.0 / scale))
    }

    fn press_on(&mut self, element: Element, button: PointerButton, modifiers: Modifiers) -> Result<(), HeadlessError> {
        let at = self.centre(element)?;
        self.move_pointer(at);
        self.pointer_event_with(PointerAction::Pressed, at, Some(button), modifiers);
        self.pointer_event_with(PointerAction::Released, at, Some(button), modifiers);

        Ok(())
    }

    fn move_pointer(&mut self, at: Point<CssPx, Css>) {
        if self.pointer.is_none() {
            self.pointer_event(PointerAction::Entered, at, None);
        }
        self.pointer = Some(at);
        self.pointer_event(PointerAction::Moved, at, None);
    }

    fn pointer_event(&mut self, action: PointerAction, at: Point<CssPx, Css>, button: Option<PointerButton>) {
        self.pointer_event_with(action, at, button, Modifiers::NONE);
    }

    fn pointer_event_with(
        &mut self,
        action: PointerAction,
        at: Point<CssPx, Css>,
        button: Option<PointerButton>,
        modifiers: Modifiers,
    ) {
        let mut event = PointerEvent::mouse(at);
        if let Some(button) = button {
            event = event.with_button(button);
        }

        self.deliver(SurfaceEvent::Pointer { action, event, modifiers, timestamp: Timestamp::ORIGIN });
    }

    fn key(&mut self, event: KeyEvent, modifiers: Modifiers) {
        for state in [KeyState::Pressed, KeyState::Released] {
            self.deliver(SurfaceEvent::Key { state, event: event.clone(), modifiers, timestamp: Timestamp::ORIGIN });
        }
    }

    fn deliver(&mut self, event: SurfaceEvent) {
        self.wake_if_unrouted();
        self.harness.deliver_to_first(event);
        self.harness.settle(SETTLE_TURNS);
    }

    /// Asks for a frame for reactive work, when this window's runtime does not receive the
    /// thread's reactive wakes: a signal its view depends on, written between frames, then wakes
    /// no loop or some other window's.
    fn wake_if_unrouted(&self) {
        if self.exclusive.receives_wakes() {
            return;
        }

        let platform = self.harness.platform();
        let surfaces = platform.offscreens().iter().map(|surface| Surface::id(surface.as_ref())).collect();
        platform.waker().wake(WakeReason::ReactiveWork { surfaces });
    }
}

/// A change handler that records nothing: what is read is the tree the updates leave behind.
struct Silent;

impl TreeChangeHandler for Silent {
    fn node_added(&mut self, _: &Node<'_>) {}
    fn node_updated(&mut self, _: &Node<'_>, _: &Node<'_>) {}
    fn focus_moved(&mut self, _: Option<&Node<'_>>, _: Option<&Node<'_>>) {}
    fn node_removed(&mut self, _: &Node<'_>) {}
}

/// A thread's claim on the process's windows, held while any [`HeadlessWindow`] is open on it.
///
/// Threads take turns, but one thread may hold any number of windows: a second window opened
/// beside the first must not wait on a claim its own thread holds.
struct Exclusive {
    window: WindowKey,
    /// Ties the claim to the thread that counted it.
    _thread: PhantomData<*const ()>,
}

impl Exclusive {
    fn take() -> Self {
        static PROCESS: Mutex<()> = Mutex::new(());

        let window = HELD.with_borrow_mut(|held| {
            let held = held.get_or_insert_with(|| Held {
                _guard: PROCESS.lock().unwrap_or_else(PoisonError::into_inner),
                open: 0,
                newest: WindowKey(0),
            });
            held.open += 1;
            held.newest = WindowKey(held.newest.0 + 1);

            held.newest
        });

        Self { window, _thread: PhantomData }
    }

    /// Whether this window's runtime receives the thread's reactive wakes. A runtime takes them
    /// when it starts, so the window opened last on the thread has them, and while it is open no
    /// other window does.
    fn receives_wakes(&self) -> bool {
        HELD.with_borrow(|held| held.as_ref().is_some_and(|held| held.newest == self.window))
    }
}

impl Drop for Exclusive {
    fn drop(&mut self) {
        // A window dropped while its thread is torn down has nothing left to release.
        let _ = HELD.try_with(|held| {
            let mut held = held.borrow_mut();
            let last = held.as_mut().is_some_and(|held| {
                held.open -= 1;
                held.open == 0
            });
            if last {
                *held = None;
            }
        });
    }
}

/// One window among those opened on a thread while its claim is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowKey(u64);

/// The process claim a thread holds, how many of its windows are open and which opened last.
struct Held {
    _guard: MutexGuard<'static, ()>,
    open: usize,
    newest: WindowKey,
}

thread_local! {
    static HELD: RefCell<Option<Held>> = const { RefCell::new(None) };
}

fn device_renderer(_surface: &Arc<dyn Surface>, target: RenderTarget) -> Result<Box<dyn Renderer>, AppError> {
    let mut renderer =
        Builder::new().offscreen(target, wgpu::TextureFormat::Bgra8Unorm, false).map_err(AppError::GpuUnavailable)?;
    zgui_render_vector_vello::attach(&mut renderer, target.size);

    Ok(Box::new(renderer))
}

/// The physical key a backend reports beside `key`, for the keys that have one fixed place.
fn physical_key(key: NamedKey) -> PhysicalKey {
    let code = match key {
        NamedKey::Tab => KeyCode::Tab,
        NamedKey::Enter => KeyCode::Enter,
        NamedKey::Space => KeyCode::Space,
        NamedKey::Escape => KeyCode::Escape,
        NamedKey::Backspace => KeyCode::Backspace,
        NamedKey::Delete => KeyCode::Delete,
        NamedKey::ArrowDown => KeyCode::ArrowDown,
        NamedKey::ArrowUp => KeyCode::ArrowUp,
        NamedKey::ArrowRight => KeyCode::ArrowRight,
        NamedKey::ArrowLeft => KeyCode::ArrowLeft,
        NamedKey::Home => KeyCode::Home,
        NamedKey::End => KeyCode::End,
        NamedKey::PageUp => KeyCode::PageUp,
        NamedKey::PageDown => KeyCode::PageDown,
        _ => return PhysicalKey::Unidentified(0),
    };

    PhysicalKey::Code(code)
}
