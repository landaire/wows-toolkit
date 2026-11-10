use rootcause::compat::ReportAsError;

/// Reasons the Armor Viewer's shared ship assets failed to load.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ArmorAssetsError {
    #[error("failed to load ship assets: {source}")]
    ShipAssets { source: ReportAsError },
}

/// Reasons a ship's armor model failed to load.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ShipLoadError {
    #[error("no vehicle found for param index {param_index}")]
    NoVehicle { param_index: String },
    #[error("failed to export ship model: {source}")]
    ShipExport { source: ReportAsError },
    #[error("failed to build armor meshes: {source}")]
    ArmorMeshes { source: ReportAsError },
    #[error("failed to build hull meshes: {source}")]
    HullMeshes { source: ReportAsError },
    #[error("failed to resolve camouflage textures: {source}")]
    CamoSource { source: ReportAsError },
}

/// Reasons a GLB model export failed.
#[derive(Debug, thiserror::Error)]
pub enum ExportGlbError {
    #[error("no vehicle found for param index {param_index}")]
    NoVehicle { param_index: String },
    #[error("failed to export ship model: {source}")]
    ShipExport { source: ReportAsError },
    #[error("failed to create output file {path}: {source}")]
    CreateFile { path: std::path::PathBuf, source: std::io::Error },
    #[error("failed to write glb: {source}")]
    WriteGlb { source: ReportAsError },
}

/// Reasons the viewport's own wgpu device could not be created.
#[derive(Debug, thiserror::Error)]
pub enum GpuInitError {
    #[error("no suitable wgpu adapter: {source}")]
    NoAdapter { source: wgpu::RequestAdapterError },
    #[error("failed to create wgpu device: {source}")]
    RequestDevice { source: wgpu::RequestDeviceError },
}
