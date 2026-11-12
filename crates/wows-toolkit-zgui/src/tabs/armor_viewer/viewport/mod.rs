//! The armor viewport: the 3D scene drawn by the shared wgpu engine into a `surface` element, with
//! pointer and keyboard camera control, CPU picking with hover tooltips, and an overlay drawing the
//! navigation gizmo, labels, cast arcs and hit rings over it.
//!
//! A pane holds an [`ArmorViewportState`] per viewport and shows it with [`ArmorViewport`]. The
//! state is the whole viewport: the scene uploaded to the device, the camera, what the reader has
//! hidden, hovered and cast, and the loads in flight. The panels around the viewport read it and
//! change it through the same handle.
//!
//! Drawing is on demand. A change that moves the scene marks the engine's viewport dirty, and the
//! handle asks the surface to draw again; only a camera animation in flight or a movement key held
//! down asks for a frame per refresh. The pipeline is compiled off the UI thread once per device
//! and shared by every viewport; a new device generation drops it with every mesh made on it, and
//! each viewport uploads its scene again.

mod model;
mod state;
mod types;

pub use model::ViewportModel;
pub use state::ArmorViewportState;
pub use state::PaneMenu;
pub use types::Agreement;
pub use types::AllVisible;
pub use types::ArcSummary;
pub use types::CameraRingSettings;
pub use types::CastShell;
pub use types::GapState;
pub use types::Isolate;
pub use types::LoadGeneration;
pub use types::MeshSeed;
pub use types::OpeningLayer;
pub use types::OpeningLoad;
pub use types::PerspectiveSettings;
pub use types::SceneFeatures;
pub use types::SceneSource;
pub use types::SimAgreement;
pub use types::SplashState;
pub use types::SyncedSettings;
pub use types::TrajectoryState;
pub use types::ViewportChange;
pub use types::agreement;
