// filled in a later task

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceIdParseError {
    #[error("parse error")]
    ParseError,
}

pub struct InstallId;
pub struct PcId;
pub struct TrackingId;
pub struct VendingId;
pub struct WgcId;
