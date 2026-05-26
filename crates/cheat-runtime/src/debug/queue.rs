//! Event queue with Condvar wake. Mirrors CE's
//! `AddDebugEventToQueue` / `RemoveThreadDebugEventFromQueue` /
//! `FindThreadDebugEventInQueue` / `WakeDebuggerThread` (api.c around
//! the `debugEventQueue` field).
//!
//! # Why a queue and not just `waitpid` directly
//!
//! `WaitForDebugEventNative` (api.c:2014) wants to return an event
//! for a *specific* TID. If `waitpid` reports a different TID first,
//! CE doesn't drop it — it parks the event here and keeps waiting
//! for the wanted TID. The next `WaitForDebugEvent` call (possibly
//! from a different consumer) drains the parked event before
//! touching `waitpid` again, so nothing is lost across calls.
//!
//! # Wake semantics
//!
//! [`wake_one`] signals one waiter on the Condvar so the consumer
//! re-checks the queue + `waitpid` state. The SIGCHLD handler in
//! `ptrace_helpers::install_sigchld_handler` already wakes a global
//! Condvar; this queue's Condvar is *local* to the Debugger so each
//! debugged process has its own wake channel.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use nix::unistd::Pid;

use crate::debug::event::DebugEvent;

#[derive(Debug, Default)]
pub struct EventQueue {
    inner: Mutex<VecDeque<DebugEvent>>,
    cv: Condvar,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
        }
    }

    /// Append an event to the tail of the queue + wake one waiter.
    /// Mirror `AddDebugEventToQueue`.
    pub fn push(&self, event: DebugEvent) {
        self.inner.lock().expect("queue poisoned").push_back(event);
        self.cv.notify_one();
    }

    /// Take the first queued event for `tid`, if any. Mirror
    /// `RemoveThreadDebugEventFromQueue` + `FindThreadDebugEventInQueue`.
    pub fn pop_first_for_tid(&self, tid: Pid) -> Option<DebugEvent> {
        let mut q = self.inner.lock().expect("queue poisoned");
        let idx = q.iter().position(|e| e.tid == tid)?;
        q.remove(idx)
    }

    /// Take the oldest queued event regardless of TID.
    pub fn pop_any(&self) -> Option<DebugEvent> {
        self.inner.lock().expect("queue poisoned").pop_front()
    }

    /// Wake one waiter (e.g. from a SIGCHLD handler installed
    /// elsewhere, or after enqueuing an event via a back channel).
    /// Mirror `WakeDebuggerThread`.
    pub fn wake_one(&self) {
        self.cv.notify_one();
    }

    /// Block until an event is available (any TID), with optional
    /// timeout. Returns `None` on timeout, `Some(event)` otherwise.
    /// Spurious wakeups loop back into `wait`.
    pub fn wait_any(&self, timeout: Option<Duration>) -> Option<DebugEvent> {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut q = self.inner.lock().expect("queue poisoned");
        loop {
            if let Some(ev) = q.pop_front() {
                return Some(ev);
            }
            match deadline {
                None => {
                    q = self.cv.wait(q).expect("queue cv poisoned");
                }
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return None;
                    }
                    let remaining = d - now;
                    let (g, ts) = self
                        .cv
                        .wait_timeout(q, remaining)
                        .expect("queue cv poisoned");
                    q = g;
                    if ts.timed_out() && q.is_empty() {
                        return None;
                    }
                }
            }
        }
    }

    /// Block until an event for `tid` is available. Events for other
    /// TIDs that arrive in the meantime stay queued. Returns `None`
    /// on timeout.
    pub fn wait_for_tid(&self, tid: Pid, timeout: Option<Duration>) -> Option<DebugEvent> {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut q = self.inner.lock().expect("queue poisoned");
        loop {
            if let Some(idx) = q.iter().position(|e| e.tid == tid) {
                return q.remove(idx);
            }
            match deadline {
                None => {
                    q = self.cv.wait(q).expect("queue cv poisoned");
                }
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return None;
                    }
                    let remaining = d - now;
                    let (g, ts) = self
                        .cv
                        .wait_timeout(q, remaining)
                        .expect("queue cv poisoned");
                    q = g;
                    if ts.timed_out() && q.iter().all(|e| e.tid != tid) {
                        return None;
                    }
                }
            }
        }
    }

    /// Current queue length — test/debug aid.
    pub fn len(&self) -> usize {
        self.inner.lock().expect("queue poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::event::{DebugEvent, DebugEventKind};
    use std::sync::Arc;
    use std::thread;

    fn ev(tid: i32, signo: i32) -> DebugEvent {
        DebugEvent {
            tid: Pid::from_raw(tid),
            kind: DebugEventKind::Signal { signo },
        }
    }

    #[test]
    fn fifo_order() {
        let q = EventQueue::new();
        q.push(ev(10, 1));
        q.push(ev(11, 2));
        q.push(ev(12, 3));
        assert_eq!(q.pop_any().unwrap().tid, Pid::from_raw(10));
        assert_eq!(q.pop_any().unwrap().tid, Pid::from_raw(11));
        assert_eq!(q.pop_any().unwrap().tid, Pid::from_raw(12));
        assert!(q.pop_any().is_none());
    }

    #[test]
    fn pop_first_for_tid_skips_others() {
        let q = EventQueue::new();
        q.push(ev(10, 1));
        q.push(ev(20, 2));
        q.push(ev(10, 3));
        // First for tid=20.
        let e = q.pop_first_for_tid(Pid::from_raw(20)).unwrap();
        assert_eq!(e.tid, Pid::from_raw(20));
        // Two left, both tid=10.
        assert_eq!(q.len(), 2);
        let e1 = q.pop_first_for_tid(Pid::from_raw(10)).unwrap();
        assert!(matches!(e1.kind, DebugEventKind::Signal { signo: 1 }));
        let e2 = q.pop_first_for_tid(Pid::from_raw(10)).unwrap();
        assert!(matches!(e2.kind, DebugEventKind::Signal { signo: 3 }));
    }

    #[test]
    fn pop_first_for_tid_returns_none_when_absent() {
        let q = EventQueue::new();
        q.push(ev(99, 1));
        assert!(q.pop_first_for_tid(Pid::from_raw(7)).is_none());
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn wait_any_returns_immediately_if_queued() {
        let q = EventQueue::new();
        q.push(ev(1, 1));
        let e = q.wait_any(Some(Duration::from_millis(10))).unwrap();
        assert_eq!(e.tid, Pid::from_raw(1));
    }

    #[test]
    fn wait_any_times_out() {
        let q = EventQueue::new();
        let started = Instant::now();
        let result = q.wait_any(Some(Duration::from_millis(20)));
        let elapsed = started.elapsed();
        assert!(result.is_none());
        assert!(elapsed >= Duration::from_millis(15));
        assert!(elapsed < Duration::from_millis(300));
    }

    #[test]
    fn wait_any_wakes_on_push() {
        let q = Arc::new(EventQueue::new());
        let qc = Arc::clone(&q);
        let t = thread::spawn(move || qc.wait_any(Some(Duration::from_secs(1))));
        thread::sleep(Duration::from_millis(20));
        q.push(ev(42, 5));
        let got = t.join().unwrap().expect("event delivered");
        assert_eq!(got.tid, Pid::from_raw(42));
    }

    #[test]
    fn wait_for_tid_ignores_other_tids() {
        let q = Arc::new(EventQueue::new());
        let qc = Arc::clone(&q);
        let t =
            thread::spawn(move || qc.wait_for_tid(Pid::from_raw(7), Some(Duration::from_secs(1))));
        thread::sleep(Duration::from_millis(10));
        // Push for a different tid first — must not satisfy the wait.
        q.push(ev(99, 1));
        thread::sleep(Duration::from_millis(10));
        // Now the right one.
        q.push(ev(7, 2));
        let got = t.join().unwrap().expect("event for 7 delivered");
        assert_eq!(got.tid, Pid::from_raw(7));
        // The tid=99 event must still be queued.
        assert_eq!(q.len(), 1);
        assert_eq!(q.pop_any().unwrap().tid, Pid::from_raw(99));
    }
}
