//! Warnings that stand in front of something, and the ones a reader may switch
//! off.
//!
//! The egui app puts the same two questions: hosting or joining a session
//! reveals this machine's address (`app.rs`'s `show_ip_warning_dialog`), and an
//! export with no GPU encoder behind it will encode in software
//! (`replay/renderer/mod.rs`'s `GpuEncoderWarning`). Both are suppressible, in
//! the rows the egui app writes, so a reader who switched one off there does not
//! meet it here.

use rust_i18n::t;

use crate::settings::keys;

/// Where the networking note lives, which the address warning links as the egui
/// dialog does.
pub const NETWORKING_URL: &str = "https://landaire.github.io/wows-toolkit/networking";

/// One of the two warnings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Hosting or joining a session tells the other peers where this machine is.
    Address,
    /// An export with no GPU encoder to use takes far longer.
    SoftwareEncode,
}

impl Notice {
    /// The settings row the reader's choice to switch this warning off is saved in.
    pub fn setting_key(self) -> &'static str {
        match self {
            Self::Address => keys::SUPPRESS_P2P_IP_WARNING,
            Self::SoftwareEncode => keys::SUPPRESS_GPU_ENCODER_WARNING,
        }
    }

    pub fn title(self) -> String {
        match self {
            Self::Address => t!("ui.windows.network_warning").into_owned(),
            Self::SoftwareEncode => t!("ui.windows.software_encode").into_owned(),
        }
    }

    pub fn message(self) -> String {
        match self {
            Self::Address => t!("ui.dialogs.p2p_warning").into_owned(),
            Self::SoftwareEncode => t!("ui.dialogs.software_encode_message").into_owned(),
        }
    }

    /// The page the warning links for more detail, if it links one.
    pub fn more_info_url(self) -> Option<&'static str> {
        match self {
            Self::Address => Some(NETWORKING_URL),
            Self::SoftwareEncode => None,
        }
    }
}

/// Which warnings the reader has switched off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Suppressed {
    pub address: bool,
    pub software_encode: bool,
}

impl Suppressed {
    pub fn is_suppressed(&self, notice: Notice) -> bool {
        match notice {
            Notice::Address => self.address,
            Notice::SoftwareEncode => self.software_encode,
        }
    }

    pub fn set(&mut self, notice: Notice, suppressed: bool) {
        match notice {
            Notice::Address => self.address = suppressed,
            Notice::SoftwareEncode => self.software_encode = suppressed,
        }
    }
}
