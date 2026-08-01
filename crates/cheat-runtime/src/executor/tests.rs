//! Executor module tests.

use super::*;
use crate::parser::parse;
use nix::unistd::Pid;

fn engine_for_self() -> Engine {
    Engine::new(Pid::this())
}

#[test]
fn enable_with_only_noops_creates_empty_active() {
    // `{$begin_obfuscate}` is a no-op CE directive — used here as a
    // generic stand-in. We deliberately omit `{$lua}` because it now
    // forces the executor into the `LuaNotSupported` path (covered by
    // [`enable_with_lua_directive_returns_lua_not_supported`] below).
    let script =
        parse("[ENABLE]\nregistersymbol(foo)\nlabel(bar)\n{$begin_obfuscate}\n[DISABLE]\n")
            .unwrap();
    let mut eng = engine_for_self();
    let active = eng.enable(&script).expect("noop enable should succeed");
    assert_eq!(active.writes(), 0);
    active.disable().unwrap();
}

#[test]
fn enable_with_lua_directive_returns_lua_not_supported() {
    let script = parse("[ENABLE]\n{$lua}\npause()\n[DISABLE]\n").unwrap();
    let mut eng = engine_for_self();
    let err = eng.enable(&script).expect_err("lua_only must not succeed");
    assert!(
        matches!(err, ExecError::LuaNotSupported),
        "expected LuaNotSupported, got {err:?}"
    );
}

#[test]
fn enable_with_pure_lua_body_returns_lua_not_supported() {
    // No `[ENABLE]` block at all — pure CE `{$lua}` payload that CE
    // would normally hand to its embedded interpreter.
    let script = parse("{$lua}\nautoAssemble([[ ... ]])\n").unwrap();
    let mut eng = engine_for_self();
    let err = eng.enable(&script).expect_err("lua_only must not succeed");
    assert!(matches!(err, ExecError::LuaNotSupported));
}

#[test]
#[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
fn alloc_enable_then_disable_round_trips_codecave() {
    use nix::sys::signal::{Signal, kill};
    use nix::sys::wait::waitpid;
    use nix::unistd::{ForkResult, fork};
    use std::time::Duration;

    // Linux refuses ptrace-self. Fork a sleeping child and run the
    // executor against it.
    match unsafe { fork() }.expect("fork") {
        ForkResult::Child => {
            std::thread::sleep(Duration::from_secs(5));
            std::process::exit(0);
        }
        ForkResult::Parent { child } => {
            std::thread::sleep(Duration::from_millis(150));

            let script =
                parse("[ENABLE]\nalloc(codecave,4096)\n[DISABLE]\ndealloc(codecave)\n").unwrap();
            let mut eng = Engine::new(child);
            let active = eng.enable(&script).expect("alloc must succeed");
            let codecave_addr = *eng.symbols().get("codecave").expect("codecave bound");
            assert!(codecave_addr != 0);
            assert!(codecave_addr & 0xfff == 0);
            active.disable().unwrap();

            let _ = kill(child, Signal::SIGKILL);
            let _ = waitpid(child, None);
        }
    }
}

#[test]
fn dealloc_of_unknown_symbol_is_lenient_noop() {
    // CE's autoassembler silently ignores `dealloc(name)` when `name`
    // was not previously allocated. The wild (FearLess corpus, audit
    // 2026-05-26) routinely pairs cross-script cleanup blocks that
    // dealloc names a companion script already freed; erroring here
    // would block legitimate idempotent disables.
    let script = parse("[ENABLE]\ndealloc(missing)\n[DISABLE]\n").unwrap();
    let mut eng = engine_for_self();
    let active = eng.enable(&script).expect("dealloc(unknown) must no-op");
    active.disable().unwrap();
}

#[test]
fn orphan_raw_write_errors() {
    let script = parse("[ENABLE]\ndb DE AD BE EF\n[DISABLE]\n").unwrap();
    let mut eng = engine_for_self();
    let err = eng.enable(&script).unwrap_err();
    assert!(matches!(
        err,
        ExecError::Unsupported(_) | ExecError::OrphanWrite(_)
    ));
}

/// End-to-end: orchestration of LabelSite + `db` overwrite + DISABLE
/// rollback. Pre-binds the symbol address (the `aobscanmodule` path is
/// already covered by the scanner crate's own tests; here we exercise
/// the executor's enable/disable orchestration in isolation, which is
/// reliable even when the test process's heap holds other copies of
/// arbitrary byte patterns).
#[test]
fn enable_then_disable_roundtrips_a_byte_overwrite() {
    let mut victim = [0u8; 64];
    let original: [u8; 16] = [
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
        0x00,
    ];
    victim[16..32].copy_from_slice(&original);
    let target_addr = victim.as_ptr() as u64 + 16;
    let orig_hex = original
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");

    let script_src = format!(
        "[ENABLE]\n\
         registersymbol(victim)\n\
         victim:\n\
         db 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00\n\
         [DISABLE]\n\
         victim:\n\
         db {orig_hex}\n\
         unregistersymbol(victim)\n",
    );
    let script = parse(&script_src).unwrap();

    let mut eng = Engine::new(Pid::this());
    eng.bind_symbol("victim", target_addr);

    let active = eng.enable(&script).expect("enable must succeed");
    assert_eq!(active.writes(), 1);

    // After ENABLE the bytes are zeroed.
    assert_eq!(&victim[16..32], &[0u8; 16]);

    active.disable().unwrap();

    // After DISABLE the original bytes are restored.
    assert_eq!(&victim[16..32], &original);
}

/// AbsoluteSite parity with LabelSite: the migrator emits numeric label
/// sites (CE's `0xADDR:` form) and the executor must apply writes the
/// same way as it does for symbolic sites resolved via aobscanmodule.
#[test]
fn absolute_site_roundtrips_a_byte_overwrite() {
    let mut victim = [0u8; 16];
    let original = [0xCA, 0xFE, 0xBA, 0xBE];
    victim[4..8].copy_from_slice(&original);
    let target_addr = victim.as_ptr() as u64 + 4;

    let script_src = format!("[ENABLE]\n0x{target_addr:X}:\ndb 11 22 33 44\n[DISABLE]\n");
    let script = parse(&script_src).unwrap();

    let mut eng = Engine::new(Pid::this());
    let active = eng
        .enable(&script)
        .expect("absolute-site enable must succeed");
    assert_eq!(active.writes(), 1);
    assert_eq!(&victim[4..8], &[0x11, 0x22, 0x33, 0x44]);

    active.disable().unwrap();
    assert_eq!(&victim[4..8], &original);
}

/// Atomicity: a failing later statement must roll back the writes that
/// the earlier statements applied.
#[test]
fn failed_statement_rolls_back_prior_writes() {
    let mut victim = [0u8; 32];
    let original: [u8; 8] = [0xCA, 0xFE, 0xBA, 0xBE, 0xDE, 0xAD, 0xBE, 0xEF];
    victim[8..16].copy_from_slice(&original);
    let target_addr = victim.as_ptr() as u64 + 8;

    // First write zeros, then trigger an Unsupported asm line — must rollback.
    // `fizzbuzz` is a truly fake mnemonic that will never be added, so
    // this test stays robust as the supported set grows.
    let script_src = "[ENABLE]\n\
         registersymbol(victim)\n\
         victim:\n\
         db 00 00 00 00 00 00 00 00\n\
         fizzbuzz rax, rbx\n\
         [DISABLE]\n";
    let script = parse(script_src).unwrap();

    let mut eng = Engine::new(Pid::this());
    eng.bind_symbol("victim", target_addr);

    let err = eng.enable(&script).unwrap_err();
    assert!(matches!(err, ExecError::Unsupported(_)));
    // Crucially, the previously-applied write was reverted.
    assert_eq!(&victim[8..16], &original);
}

/// Pass 1 must bind a forward `LabelSite` to the cursor it computes from
/// previous Raw lengths. This is the lever that makes `jmp return` work
/// before `return:` is reached at write time.
#[test]
#[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
fn full_injection_round_trip_against_forked_child() {
    use nix::sys::signal::{Signal, kill};
    use nix::sys::wait::waitpid;
    use nix::unistd::{ForkResult, fork};
    use std::time::Duration;

    // Unique pattern + 8 bytes that will be overwritten by the hook.
    // The pattern is the *anchor* aobscan finds; the 8 bytes immediately
    // after it are what `jmp codecave + nop 3` (5+3 = 8 bytes) replaces.
    let pattern = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
    let original_hook = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
    // Box::leak so the buffer survives the fork's child lifetime without
    // touching the parent's stack frame.
    let mut buf = vec![0u8; 256];
    buf[0..8].copy_from_slice(&pattern);
    buf[8..16].copy_from_slice(&original_hook);
    let leaked: &'static [u8] = Box::leak(buf.into_boxed_slice());
    let pattern_addr = leaked.as_ptr() as u64;
    let hook_addr = pattern_addr + 8;

    match unsafe { fork() }.expect("fork") {
        ForkResult::Child => {
            // Hold the page alive long enough for the parent to ptrace
            // + run the inject + dealloc + verify.
            std::thread::sleep(Duration::from_secs(10));
            std::process::exit(0);
        }
        ForkResult::Parent { child } => {
            std::thread::sleep(Duration::from_millis(150));

            let pattern_str = pattern
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            let orig_hex = original_hook
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            let script_src = format!(
                "[ENABLE]\n\
                 aobscanmodule(hook_origin, $process, {pattern_str})\n\
                 alloc(codecave, 0x100, hook_origin)\n\
                 codecave:\n\
                 db {orig_hex}\n\
                 jmp hook_return\n\
                 hook:\n\
                 jmp codecave\n\
                 nop 3\n\
                 hook_return:\n\
                 [DISABLE]\n\
                 dealloc(codecave)\n",
            );
            let script = parse(&script_src).expect("script parses");

            let mut eng = Engine::new(child);
            // `hook` references the 8-byte hook region. AobScan finds
            // `hook_origin` = pattern_addr; we pre-bind `hook` =
            // pattern_addr + 8 so the second LabelSite can resolve to it.
            eng.bind_symbol("hook", hook_addr);

            let active = eng.enable(&script).expect("full inject must succeed");
            let _ = pattern_addr; // silence dead_code on the debug capture

            // After enable, the child's memory at `hook_addr` is the
            // jmp + nop pad we wrote. Read it back and verify.
            let hooked = crate::memory::read_bytes(child, hook_addr, 8).expect("read hook");
            assert_eq!(hooked[0], 0xE9, "first hook byte is jmp rel32 opcode");
            assert_eq!(&hooked[5..8], &[0x90, 0x90, 0x90], "nop 3 fill");

            // Disable rolls back the writes AND deallocs the codecave.
            active.disable().expect("disable must succeed");
            let restored = crate::memory::read_bytes(child, hook_addr, 8).expect("read restored");
            assert_eq!(restored, original_hook, "hook bytes byte-for-byte restored");

            let _ = kill(child, Signal::SIGKILL);
            let _ = waitpid(child, None);
        }
    }
}
