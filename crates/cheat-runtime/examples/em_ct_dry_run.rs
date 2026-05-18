//! Convert the user's actual EM .CT and print a manifest summary. Read-only;
//! no files are written. Used as a sanity check before wiring the auto-import
//! into the Tauri startup path.
use std::path::PathBuf;

fn main() {
    let path = PathBuf::from(format!(
        "{}/.config/backlog-tracker/cheat-tables/2725260/Ender Magnolia v11.ct",
        std::env::var("HOME").expect("HOME unset"),
    ));
    let manifest = cheat_runtime::convert_ct_file(&path).expect("convert");
    println!("title: {}", manifest.title);
    println!("exe:   {}", manifest.exe);
    println!("features ({}):", manifest.features.len());
    for f in &manifest.features {
        let kind = match f.kind {
            cheat_runtime::FeatureKind::Toggle => "toggle",
            cheat_runtime::FeatureKind::Header => "header",
        };
        println!("  - [{kind}] {}", f.name);
    }
}
