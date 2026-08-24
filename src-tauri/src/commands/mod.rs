// The cheat panel is a Linux-only surface until the Win32 backend lands
// (#181): every module below the gate reaches `cheat-runtime`, which is
// built on ptrace and /proc.
#[cfg(unix)]
pub mod ce_cmd;
#[cfg(unix)]
pub mod cheat_runtime_cmd;
#[cfg(unix)]
pub mod cheat_search_cmd;

pub mod cartridge_cmd;
pub mod collection_cmd;
pub mod detail_cmd;
pub mod disk_cmd;
pub mod drm_cmd;
pub mod misc_cmd;
pub mod state_cmd;
pub mod sync_cmd;
pub mod window_cmd;
