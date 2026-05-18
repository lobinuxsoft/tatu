//! Direct smoke against running EM, bypassing Tauri. Prints exact error.
use cheat_runtime::{Engine, find_pid_by_exe, load_manifests_for, parse_script};
use nix::unistd::Pid;

fn main() {
    let manifests = load_manifests_for("2725260").expect("load manifests");
    let mut feature_script: Option<String> = None;
    let mut exe = String::new();
    for m in manifests {
        for f in m.features {
            if f.name == "Invincibility (smoke)" {
                feature_script = f.script.clone();
                exe = m.exe.clone();
                break;
            }
        }
        if feature_script.is_some() {
            break;
        }
    }
    let script_src = feature_script.expect("feature not found");
    println!("exe={}", exe);
    let pid = find_pid_by_exe(&exe).expect("EM process not running?");
    println!("EM pid={:?}", pid);
    let _ = Pid::from_raw(0); // silence unused import on some paths

    let script = parse_script(&script_src).expect("parse");
    println!("parsed: {} enable stmts", script.enable.len());

    let mut engine = Engine::new(pid);
    match engine.enable(&script) {
        Ok(active) => {
            println!(
                "✅ enable OK, writes={}, allocs={}",
                active.writes(),
                engine.symbols().len()
            );
            std::thread::sleep(std::time::Duration::from_secs(5));
            println!("disabling...");
            active.disable().expect("disable");
            println!("✅ disable OK");
        }
        Err(e) => {
            println!("❌ enable FAILED: {e}");
            println!("   debug: {e:?}");
        }
    }
}
