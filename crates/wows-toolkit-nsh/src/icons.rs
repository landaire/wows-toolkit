//! The icons the app draws, for Rust views ([`icon`]) and the TSX interface
//! ([`IconView`], wrapped by `ui/icons.tsx`): Lucide's (ISC,
//! `assets/icons/LICENSE-LUCIDE`), and brand logos from Simple Icons (CC0,
//! `assets/icons/LICENSE-SIMPLE-ICONS`), which Lucide does not draw. A frame
//! producer rasterizes [`Icon::svg`] with `nsh_core::svg::rasterize`.

use std::borrow::Cow;

use nsh_core::Color;
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

use crate::js_newtype::js_newtype;

/// Declares [`Icon`], one variant per vendored `assets/icons/<name>.svg`.
macro_rules! icons {
    ($($variant:ident => $name:tt,)*) => {
        /// An icon. Its JS name is its file's (`"chevron-down"`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PropEnum)]
        pub enum Icon {
            $(
                #[prop(rename = $name)]
                $variant,
            )*
        }

        impl Icon {
            /// Every icon.
            pub const ALL: &[Icon] = &[$(Self::$variant),*];

            /// The icon's SVG document on a 24 point grid, painted in
            /// `currentColor`: stroked 2 points wide (Lucide), filled (a
            /// logo), or both (a filled Lucide icon).
            pub fn svg(self) -> &'static str {
                match self {
                    $(Self::$variant => include_str!(concat!("../assets/icons/", $name, ".svg")),)*
                }
            }
        }
    };
}

icons! {
    AppWindow => "app-window",
    Archive => "archive",
    Bell => "bell",
    Bomb => "bomb",
    Box => "box",
    Castle => "castle",
    ChartColumn => "chart-column",
    ChartLine => "chart-line",
    Check => "check",
    ChevronDown => "chevron-down",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    Circle => "circle",
    CircleAlert => "circle-alert",
    CircleCheck => "circle-check",
    CircleX => "circle-x",
    Clipboard => "clipboard",
    ClipboardList => "clipboard-list",
    Clock => "clock",
    Copy => "copy",
    Crosshair => "crosshair",
    Database => "database",
    Download => "download",
    Ellipsis => "ellipsis",
    Equal => "equal",
    Eraser => "eraser",
    ExternalLink => "external-link",
    Eye => "eye",
    EyeOff => "eye-off",
    FaceSlightlyFrowning => "face-slightly-frowning",
    FastForward => "fast-forward",
    File => "file",
    FileText => "file-text",
    Flame => "flame",
    Folder => "folder",
    FolderOpen => "folder-open",
    Funnel => "funnel",
    HardDrive => "hard-drive",
    Image => "image",
    Info => "info",
    LayoutDashboard => "layout-dashboard",
    List => "list",
    ListSortAscending => "list-sort-ascending",
    ListSortDescending => "list-sort-descending",
    Loader => "loader",
    Lock => "lock",
    Maximize => "maximize",
    Minimize => "minimize",
    Minus => "minus",
    Monitor => "monitor",
    MousePointer => "mouse-pointer",
    MoveUpRight => "move-up-right",
    Music => "music",
    NotebookPen => "notebook-pen",
    Paintbrush => "paintbrush",
    PanelBottom => "panel-bottom",
    PanelBottomOpen => "panel-bottom-open",
    PanelLeftClose => "panel-left-close",
    PanelLeftOpen => "panel-left-open",
    PanelRightClose => "panel-right-close",
    PanelRightOpen => "panel-right-open",
    Pause => "pause",
    Pencil => "pencil",
    Pin => "pin",
    Play => "play",
    Plug => "plug",
    Plus => "plus",
    Radio => "radio",
    RefreshCw => "refresh-cw",
    Rewind => "rewind",
    Rocket => "rocket",
    RotateCcw => "rotate-ccw",
    RotateCw => "rotate-cw",
    Ruler => "ruler",
    Search => "search",
    Settings => "settings",
    Settings2 => "settings-2",
    Shield => "shield",
    SkipBack => "skip-back",
    SkipForward => "skip-forward",
    Slash => "slash",
    Square => "square",
    SquareArrowOutUpRight => "square-arrow-out-up-right",
    Star => "star",
    StarFilled => "star-filled",
    Sword => "sword",
    Table => "table",
    Trash => "trash",
    Triangle => "triangle",
    TriangleAlert => "triangle-alert",
    Trophy => "trophy",
    Twitch => "twitch",
    Upload => "upload",
    Users => "users",
    VenetianMask => "venetian-mask",
    X => "x",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
}

impl Icon {
    /// The SVG document in `color` (else `currentColor`, the text color
    /// around it), its strokes `stroke` wide (else Lucide's 2 points; a
    /// logo has none).
    fn markup(self, color: Option<Color>, stroke: Option<StrokeWidth>) -> Cow<'static, str> {
        let svg = self.svg();
        if color.is_none() && stroke.is_none() {
            return Cow::Borrowed(svg);
        }

        let mut markup = svg.to_owned();
        if let Some(color) = color {
            let [red, green, blue, alpha] = color.0.to_be_bytes();
            let hex = format!("#{red:02x}{green:02x}{blue:02x}");
            let opacity = f32::from(alpha) / 255.0;
            markup = markup
                .replacen(r#"stroke="currentColor""#, &format!(r#"stroke="{hex}" stroke-opacity="{opacity}""#), 1)
                .replacen(r#"fill="currentColor""#, &format!(r#"fill="{hex}" fill-opacity="{opacity}""#), 1);
        }
        if let Some(StrokeWidth(width)) = stroke {
            markup = markup.replacen(r#"stroke-width="2""#, &format!(r#"stroke-width="{width}""#), 1);
        }

        Cow::Owned(markup)
    }
}

/// A stroke width in an icon's 24 point grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeWidth(pub f32);

js_newtype!(StrokeWidth, f32);

/// The props of [`IconView`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct IconViewProps {
    /// The icon drawn. None draws nothing.
    pub name: Prop<Option<Icon>>,
    /// Its width and height.
    #[prop(default = px(16.0))]
    pub size: Prop<Points>,
    /// Its paint color; the text color around it when unset.
    pub color: Prop<Option<Color>>,
    /// Its stroke width in the icon's 24 point grid; 2 when unset. A logo
    /// has no strokes.
    pub stroke_width: Prop<Option<StrokeWidth>>,
    /// What a screen reader says. Unset leaves the icon to the element
    /// around it.
    pub accessibility_label: Prop<Option<SharedString>>,
}

/// An icon, for the TSX interface.
#[component]
pub fn IconView(props: IconViewProps) -> impl IntoElement {
    let source = move || {
        let markup = match props.name.get() {
            Some(icon) => icon.markup(props.color.get(), props.stroke_width.get()),
            // An empty document draws nothing.
            None => Cow::Borrowed(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"/>"#),
        };
        ImageSrc::svg_markup(&markup)
    };

    svg(source)
        .w(move || props.size.get())
        .h(move || props.size.get())
        .accessible(move || props.accessibility_label.with(Option::is_some))
        // Read only while accessible, so the empty label is never announced.
        .label(move || props.accessibility_label.get().unwrap_or_default())
}

/// `icon` at `size`, painted in the text color around it.
pub fn icon(icon: Icon, size: Points) -> Svg {
    svg(ImageSrc::svg_markup(icon.svg())).w(size).h(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every vendored document parses as the drawing `svg` shows, and paints
    /// only through the root attributes `Icon::markup` rewrites.
    #[test]
    fn every_icon_parses_and_is_restylable() {
        for &icon in Icon::ALL {
            let svg = icon.svg();
            let strokes = svg.matches(r#"stroke="currentColor""#).count();
            let fills = svg.matches(r#"fill="currentColor""#).count();
            assert!(
                strokes <= 1 && fills <= 1 && strokes + fills >= 1,
                "{icon:?} paints in the text color at its root"
            );
            assert_eq!(svg.matches("currentColor").count(), strokes + fills, "{icon:?} paints nothing else in it");
            assert_eq!(svg.matches(r#"stroke-width="2""#).count(), strokes, "{icon:?} sets a stroke's width once");

            let drawing = nsh_core::svg::drawing(svg);
            assert!(drawing.is_ok(), "{icon:?} parses: {drawing:?}");
        }
    }

    #[test]
    fn markup_takes_the_color_and_stroke_width_it_is_given() {
        let markup = Icon::X.markup(Some(Color::rgba(0x12, 0x34, 0x56, 0xff)), Some(StrokeWidth(1.5)));

        assert!(markup.contains(r##"stroke="#123456" stroke-opacity="1""##), "{markup}");
        assert!(markup.contains(r#"stroke-width="1.5""#), "{markup}");
        assert!(!markup.contains("currentColor"), "{markup}");
        assert!(nsh_core::svg::drawing(&markup).is_ok());
    }

    #[test]
    fn a_filled_icon_takes_the_color_it_is_given() {
        let markup = Icon::Twitch.markup(Some(Color::rgba(0x91, 0x46, 0xff, 0x80)), None);

        assert!(markup.contains(r##"fill="#9146ff" fill-opacity="0.50"##), "{markup}");
        assert!(!markup.contains("currentColor"), "{markup}");
        assert!(nsh_core::svg::drawing(&markup).is_ok());
    }

    #[test]
    fn markup_without_overrides_is_the_vendored_document() {
        assert!(matches!(Icon::X.markup(None, None), Cow::Borrowed(svg) if svg == Icon::X.svg()));
    }
}
