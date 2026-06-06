//! Live smoke for the Mono collector (slice 5 of the Mono bridge).
//!
//! Run this AFTER:
//!   1. Dropping `cheat-mono-collector.dll` as `<game>/winhttp.dll`
//!      (`install_mono_collector`, or `cp` for a quick smoke).
//!   2. Setting the game's Steam launch options to
//!      `WINEDLLOVERRIDES="winhttp=n,b" %command%`.
//!   3. Launching the game so the collector loads and opens its TCP server.
//!
//! It connects to the collector over loopback and dumps Mono state: the Mono
//! module handle, the loaded assembly images, and — if a symbol is passed as an
//! argument — resolves it to a JIT address.
//!
//! Usage:
//!   cargo run -p cheat-runtime --example mono_smoke
//!   cargo run -p cheat-runtime --example mono_smoke -- "Player:Update"
//!   cargo run -p cheat-runtime --example mono_smoke -- "[Assembly-CSharp]Player:Update"

use cheat_runtime::MonoClient;

fn main() {
    let symbol = std::env::args().nth(1);

    let mut client = match MonoClient::connect() {
        Ok(c) => {
            println!("connected to collector on 127.0.0.1");
            c
        }
        Err(e) => {
            eprintln!("could not connect to collector: {e}");
            eprintln!(
                "is the game running with winhttp.dll dropped + \
                 WINEDLLOVERRIDES=\"winhttp=n,b\" %command%?"
            );
            std::process::exit(1);
        }
    };

    match client.init_mono() {
        Ok(0) => {
            eprintln!("InitMono returned 0 — Mono not found (IL2CPP? runtime not up yet?)");
            std::process::exit(2);
        }
        Ok(h) => println!("InitMono: mono module handle = {h:#x}"),
        Err(e) => {
            eprintln!("InitMono failed: {e}");
            std::process::exit(2);
        }
    }

    match client.enum_images() {
        Ok(images) => {
            println!("EnumImages: {} assembly image(s)", images.len());
            for (handle, name) in images.iter().take(20) {
                println!("  {handle:#018x}  {name}");
            }
            if images.len() > 20 {
                println!("  … {} more", images.len() - 20);
            }
        }
        Err(e) => eprintln!("EnumImages failed: {e}"),
    }

    if let Some(sym) = symbol {
        match client.resolve(&sym) {
            Ok(addr) => println!("resolve({sym:?}) -> JIT addr {addr:#x}"),
            Err(e) => eprintln!("resolve({sym:?}) failed: {e}"),
        }
    } else {
        println!("(pass a \"Class:Method\" arg to resolve a symbol)");
    }

    let _ = client.terminate();
}
