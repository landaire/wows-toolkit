use std::time::Duration;

use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;

/// The attribute on the window's root element that zeroes every motion token.
///
/// The style engine has no `prefers-reduced-motion` media feature, so the preference is written
/// into the document instead and an application's own sheets can match `[reduced-motion]` too.
pub const REDUCED_MOTION_ATTRIBUTE: &str = "reduced-motion";

/// Whether the interface animates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MotionPreference {
    #[default]
    Full,
    Reduced,
}

/// One kind of movement, each with its own duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Movement {
    /// A disclosure caret turning.
    Turn,
    /// A block of content opening or closing.
    Reveal,
    /// A side panel sliding open or shut.
    Slide,
    /// A surface fading in.
    Fade,
    /// An item moving to its new place within a row of items.
    Rearrange,
}

impl Movement {
    pub const ALL: [Self; 5] = [Self::Turn, Self::Reveal, Self::Slide, Self::Fade, Self::Rearrange];

    /// How long the movement takes with motion allowed.
    pub const fn full_duration(self) -> Duration {
        match self {
            Self::Turn => Duration::from_millis(240),
            Self::Reveal => Duration::from_millis(170),
            Self::Slide => Duration::from_millis(170),
            Self::Fade => Duration::from_millis(220),
            Self::Rearrange => Duration::from_millis(200),
        }
    }

    pub const fn duration(self, preference: MotionPreference) -> Duration {
        match preference {
            MotionPreference::Full => self.full_duration(),
            MotionPreference::Reduced => Duration::ZERO,
        }
    }

    /// The custom property a style sheet reads the duration from.
    pub const fn property(self) -> &'static str {
        match self {
            Self::Turn => "--lui-motion-turn",
            Self::Reveal => "--lui-motion-reveal",
            Self::Slide => "--lui-motion-slide",
            Self::Fade => "--lui-motion-fade",
            Self::Rearrange => "--lui-motion-rearrange",
        }
    }
}

/// A CSS `cubic-bezier()` timing function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl CubicBezier {
    /// The standard ease-in-out: a gentle start and end around a steady middle, so most of the
    /// travel is visible.
    pub const STANDARD: Self = Self { x1: 0.4, y1: 0.0, x2: 0.2, y2: 1.0 };

    pub fn css(self) -> String {
        format!("cubic-bezier({}, {}, {}, {})", self.x1, self.y1, self.x2, self.y2)
    }
}

/// The custom property every movement's easing is read from.
pub const EASE_PROPERTY: &str = "--lui-motion-ease";

/// The preference a [`ThemeRoot`](crate::ThemeRoot) applies, published to everything inside it.
#[derive(Clone, Copy)]
struct MotionContext(Signal<MotionPreference, LocalStorage>);

/// Publishes `preference` to the scopes below the calling one.
pub(crate) fn provide_motion_preference(preference: Signal<MotionPreference, LocalStorage>) {
    provide_local_context(MotionContext(preference));
}

/// The motion preference of the enclosing [`ThemeRoot`](crate::ThemeRoot), for movement a style
/// sheet cannot express. Outside one, motion is allowed, which is what a window with no
/// preference applied shows.
///
/// Read it in a component's body: a listener runs outside the scope the context is found through.
pub fn motion_preference() -> Signal<MotionPreference, LocalStorage> {
    use_local_context::<MotionContext>()
        .map_or_else(|| Signal::stored_local(MotionPreference::Full), |context| context.0)
}
