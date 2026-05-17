pub mod inject;
pub mod install;

pub use inject::{InjectError, ensure_auto_attach};
pub use install::{
    CE_VERSION, CE_ZIP_SHA256, CE_ZIP_SIZE, CE_ZIP_URL, CeError, CeInstall, CeStatus, binary_path,
    ensure_installed, install_dir, status,
};
