//! Unity scripting-backend detection (filesystem-only, deps-free).
//!
//! The Mono symbol bridge only applies to Unity games using the **Mono**
//! backend — those ship a `mono-2.0-bdwgc.dll` exposing the `mono_*` C API the
//! [collector](cheat_mono_collector) calls. IL2CPP games AOT-compile to C++ and
//! expose a different (`il2cpp_*`) API, so they need a separate path; non-Unity
//! games need neither. This module classifies a game install directory so the
//! installer knows whether to drop the collector at all.
//!
//! Detection mirrors [`crate::prereqs::detect_reframework`]: cheap, layout-only
//! sniffing of the well-known Unity standalone directory structure. No PE
//! parsing, no process inspection.

use std::path::{Path, PathBuf};

/// Which scripting backend a Unity game was built with — or that it isn't a
/// Unity game at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnityBackend {
    /// Unity Mono backend — `mono-2.0-bdwgc.dll` present. The collector applies.
    Mono,
    /// Unity IL2CPP backend — `GameAssembly.dll` / `il2cpp_data`. Different API;
    /// the Mono collector does not apply.
    Il2Cpp,
    /// Not a Unity game (or an unrecognised layout).
    NotUnity,
}

/// Classify the scripting backend of the Unity game installed in `game_dir`.
///
/// The Unity standalone layout is a `<Name>_Data/` directory next to the exe
/// containing `globalgamemanagers` (always present). From there:
/// - **IL2CPP** ⇒ `GameAssembly.dll` beside the exe, or `<Data>/il2cpp_data/`.
/// - **Mono** ⇒ `<Data>/Managed/` with assemblies, plus a Mono runtime DLL
///   under `<Data>/MonoBleedingEdge/` (modern) or `<Data>/Mono/` (legacy).
///
/// IL2CPP is checked first: an IL2CPP build can still carry a vestigial
/// `Managed/` folder, but a Mono build never carries `GameAssembly.dll`.
pub fn detect_unity_backend(game_dir: &Path) -> UnityBackend {
    let Some(data) = find_unity_data_dir(game_dir) else {
        return UnityBackend::NotUnity;
    };

    if game_dir.join("GameAssembly.dll").is_file() || data.join("il2cpp_data").is_dir() {
        return UnityBackend::Il2Cpp;
    }

    if data.join("Managed").is_dir() && find_mono_runtime(&data).is_some() {
        return UnityBackend::Mono;
    }

    UnityBackend::NotUnity
}

/// Locate the Mono runtime DLL inside a Unity `_Data` directory, if any. The
/// installer uses this both to confirm the Mono backend and to know the runtime
/// is the modern bdwgc build the collector targets. Returns the DLL path.
pub fn find_mono_runtime(data_dir: &Path) -> Option<PathBuf> {
    // Modern Unity: MonoBleedingEdge; legacy: Mono. The runtime may sit at the
    // root of that dir or under an arch subdir (`EmbedRuntime`, `x86_64`).
    const RUNTIME_ROOTS: &[&str] = &["MonoBleedingEdge", "Mono"];
    const RUNTIME_NAMES: &[&str] = &["mono-2.0-bdwgc.dll", "mono.dll", "monosgen-2.0.dll"];

    for root in RUNTIME_ROOTS {
        let base = data_dir.join(root);
        if !base.is_dir() {
            continue;
        }
        // Check the root and one level of subdirectories (EmbedRuntime/x86_64).
        let mut search = vec![base.clone()];
        if let Ok(entries) = std::fs::read_dir(&base) {
            search.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
        for dir in search {
            for name in RUNTIME_NAMES {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Find the Unity `<Name>_Data` directory: one ending in `_Data` that holds the
/// `globalgamemanagers` blob every Unity standalone build emits. Matching on
/// that blob (rather than the exe-derived name) keeps detection independent of
/// what the exe is called.
fn find_unity_data_dir(game_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(game_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let is_data_dir = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("_Data"));
        if is_data_dir && path.join("globalgamemanagers").is_file() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Build a Unity `_Data` dir with the `globalgamemanagers` marker.
    fn make_data_dir(root: &Path, name: &str) -> PathBuf {
        let data = root.join(name);
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("globalgamemanagers"), b"\0").unwrap();
        data
    }

    #[test]
    fn detects_modern_mono_backend() {
        let tmp = TempDir::new().unwrap();
        let data = make_data_dir(tmp.path(), "Game_Data");
        fs::create_dir_all(data.join("Managed")).unwrap();
        fs::write(data.join("Managed/Assembly-CSharp.dll"), b"MZ").unwrap();
        let runtime = data.join("MonoBleedingEdge/EmbedRuntime");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("mono-2.0-bdwgc.dll"), b"MZ").unwrap();

        assert_eq!(detect_unity_backend(tmp.path()), UnityBackend::Mono);
        assert!(find_mono_runtime(&data).is_some());
    }

    #[test]
    fn detects_legacy_mono_backend_at_root() {
        let tmp = TempDir::new().unwrap();
        let data = make_data_dir(tmp.path(), "Old_Data");
        fs::create_dir_all(data.join("Managed")).unwrap();
        fs::create_dir_all(data.join("Mono")).unwrap();
        fs::write(data.join("Mono/mono.dll"), b"MZ").unwrap();

        assert_eq!(detect_unity_backend(tmp.path()), UnityBackend::Mono);
    }

    #[test]
    fn detects_il2cpp_by_gameassembly() {
        let tmp = TempDir::new().unwrap();
        make_data_dir(tmp.path(), "Game_Data");
        fs::write(tmp.path().join("GameAssembly.dll"), b"MZ").unwrap();

        assert_eq!(detect_unity_backend(tmp.path()), UnityBackend::Il2Cpp);
    }

    #[test]
    fn detects_il2cpp_by_il2cpp_data() {
        let tmp = TempDir::new().unwrap();
        let data = make_data_dir(tmp.path(), "Game_Data");
        fs::create_dir_all(data.join("il2cpp_data")).unwrap();

        assert_eq!(detect_unity_backend(tmp.path()), UnityBackend::Il2Cpp);
    }

    #[test]
    fn il2cpp_wins_over_vestigial_managed() {
        // An IL2CPP build can carry a Managed/ folder; GameAssembly.dll must
        // still classify it as IL2CPP, not Mono.
        let tmp = TempDir::new().unwrap();
        let data = make_data_dir(tmp.path(), "Game_Data");
        fs::create_dir_all(data.join("Managed")).unwrap();
        fs::write(tmp.path().join("GameAssembly.dll"), b"MZ").unwrap();

        assert_eq!(detect_unity_backend(tmp.path()), UnityBackend::Il2Cpp);
    }

    #[test]
    fn non_unity_dir_is_not_unity() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("game.exe"), b"MZ").unwrap();
        assert_eq!(detect_unity_backend(tmp.path()), UnityBackend::NotUnity);
    }

    #[test]
    fn data_dir_without_marker_is_ignored() {
        // A `_Data`-suffixed dir without `globalgamemanagers` is not Unity.
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("Something_Data");
        fs::create_dir_all(data.join("Managed")).unwrap();
        assert_eq!(detect_unity_backend(tmp.path()), UnityBackend::NotUnity);
    }

    #[test]
    fn mono_without_runtime_is_not_unity_mono() {
        // Managed/ present but no mono runtime DLL ⇒ not a usable Mono target.
        let tmp = TempDir::new().unwrap();
        let data = make_data_dir(tmp.path(), "Game_Data");
        fs::create_dir_all(data.join("Managed")).unwrap();
        assert_eq!(detect_unity_backend(tmp.path()), UnityBackend::NotUnity);
    }
}
