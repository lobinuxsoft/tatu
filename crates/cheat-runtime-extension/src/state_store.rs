//! In-process key/value store for sharing state between the host and
//! the extension's own internal hooks. Keys are `String`, values are
//! arbitrary `Vec<u8>`. A `Mutex<HashMap>` is fine — the IPC server is
//! single-connection so contention is none.
//!
//! Use cases: storing "current cheat is active" flags that the
//! speedhack / future hooks can read without re-asking the host; caching
//! intermediate scan results between calls; pinning offsets resolved at
//! the start of a session.

use std::collections::HashMap;
use std::sync::Mutex;

static STORE: Mutex<Option<HashMap<String, Vec<u8>>>> = Mutex::new(None);

fn map() -> std::sync::MutexGuard<'static, Option<HashMap<String, Vec<u8>>>> {
    let mut g = STORE.lock().expect("state store poisoned");
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

pub fn write(key: String, value: Vec<u8>) {
    if let Some(m) = map().as_mut() {
        m.insert(key, value);
    }
}

pub fn read(key: &str) -> Option<Vec<u8>> {
    map().as_ref().and_then(|m| m.get(key).cloned())
}

pub fn delete(key: &str) {
    if let Some(m) = map().as_mut() {
        m.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_round_trip() {
        write("hp".into(), vec![9, 9, 9, 9]);
        assert_eq!(read("hp"), Some(vec![9, 9, 9, 9]));
        delete("hp");
        assert_eq!(read("hp"), None);
    }

    #[test]
    fn missing_key_reads_none() {
        assert!(read("definitely-not-set-xyzzy").is_none());
    }

    #[test]
    fn overwrite_replaces() {
        write("k".into(), vec![1]);
        write("k".into(), vec![2, 2, 2]);
        assert_eq!(read("k"), Some(vec![2, 2, 2]));
        delete("k");
    }
}
