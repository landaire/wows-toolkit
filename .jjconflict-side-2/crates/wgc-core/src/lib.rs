//! Shared domain types for the launcher subsystem.

mod device_id;
mod product_id;

pub use device_id::DeviceIdParseError;
pub use device_id::InstallId;
pub use device_id::PcId;
pub use device_id::TrackingId;
pub use device_id::VendingId;
pub use device_id::WgcId;
pub use product_id::ProductId;
pub use product_id::ProductIdParseError;
pub use product_id::Realm;
