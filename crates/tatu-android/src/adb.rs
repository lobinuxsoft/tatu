//! The user's own `adb` binary: finding it, listing devices, and running
//! commands without ever blocking on a full pipe or a cancelled transfer.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::AndroidError;

#[cfg(windows)]
const ADB_BIN: &str = "adb.exe";
#[cfg(not(windows))]
const ADB_BIN: &str = "adb";

/// How often [`wait_cancellable`] checks the cancel flag and ticks.
pub(crate) const POLL: Duration = Duration::from_millis(200);

/// One line of `adb devices -l`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    pub serial: String,
    /// adb's own state word: `device` (ready), `unauthorized` (the phone
    /// hasn't accepted this PC's key yet), `offline`, `no permissions`
    /// (Linux without the udev rule), ...
    pub state: String,
    /// `model:` from `-l`, underscores back to spaces; empty if not reported.
    pub model: String,
}

#[derive(Clone)]
pub struct Adb {
    exe: PathBuf,
}

/// Output of a child that ran to completion under [`wait_cancellable`].
pub(crate) struct Finished {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Adb {
    /// `configured` is the path the user typed (empty = not set). Otherwise
    /// the usual places: the SDK env vars, `PATH`, then Android Studio's
    /// default SDK folder — where adb lives for most Windows users, who
    /// rarely have it on `PATH`.
    pub fn locate(configured: &str) -> Result<Self, AndroidError> {
        if !configured.is_empty() {
            let exe = PathBuf::from(configured);
            return if exe.is_file() {
                Ok(Self { exe })
            } else {
                Err(AndroidError::AdbPathInvalid(configured.to_string()))
            };
        }
        let mut dirs: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|sdk| PathBuf::from(sdk).join("platform-tools"))
            .collect();
        if let Some(path) = std::env::var_os("PATH") {
            dirs.extend(std::env::split_paths(&path));
        }
        #[cfg(windows)]
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join(r"Android\Sdk\platform-tools"));
        }
        #[cfg(not(windows))]
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Android/Sdk/platform-tools"));
        }
        dirs.into_iter()
            .map(|dir| dir.join(ADB_BIN))
            .find(|exe| exe.is_file())
            .map(|exe| Self { exe })
            .ok_or(AndroidError::AdbNotFound)
    }

    pub fn devices(&self) -> Result<Vec<Device>, AndroidError> {
        Ok(parse_devices(&self.run(&["devices", "-l"])?))
    }

    /// Runs `script` through the phone's `/system/bin/sh` and returns its
    /// stdout. Every path inside `script` must already be `remote::quote`d.
    pub(crate) fn shell(&self, serial: &str, script: &str) -> Result<String, AndroidError> {
        self.run(&["-s", serial, "shell", script])
    }

    /// `shell` for the performance sampler, with a hard deadline — a dropped
    /// Wi-Fi link must not hang a 1-second tick forever. Probe scripts read
    /// best-effort and end in `exit 0`, so a non-zero exit is adb itself
    /// failing (device gone, offline) and comes back as an error.
    pub(crate) fn shell_deadline(
        &self,
        serial: &str,
        script: &str,
        deadline: Duration,
    ) -> Result<String, AndroidError> {
        let mut child = self.spawn(&["-s", serial, "shell", script])?;
        let stderr = drain(child.stderr.take());
        let stdout = child.stdout.take();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = stdout {
                let _ = pipe.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        match rx.recv_timeout(deadline) {
            Ok(buf) => {
                let status = child.wait().map_err(AndroidError::Spawn)?;
                if !status.success() {
                    return Err(AndroidError::Adb {
                        command: "shell".into(),
                        stderr: stderr.join().unwrap_or_default().trim().to_string(),
                    });
                }
                Ok(String::from_utf8_lossy(&buf).into_owned())
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(AndroidError::Adb {
                    command: "shell".into(),
                    stderr: "el dispositivo no respondió a tiempo".into(),
                })
            }
        }
    }

    /// Wireless debugging, first time only: `adb pair <ip:port> <code>`
    /// with the pairing port and 6-digit code the phone shows.
    pub fn pair(&self, address: &str, code: &str) -> Result<(), AndroidError> {
        validate_endpoint(address)?;
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(AndroidError::Wireless(
                "el código de emparejamiento son 6 dígitos".into(),
            ));
        }
        // Not `run()`: its error repeats the arguments, and the pairing code
        // must never end up in an error message.
        let out = self
            .command()
            .args(["pair", address, code])
            .output()
            .map_err(AndroidError::Spawn)?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("Successfully paired") {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(AndroidError::Wireless(
                format!("{} {}", stdout.trim(), stderr.trim())
                    .trim()
                    .to_string(),
            ))
        }
    }

    /// `adb connect <ip:port>` — the connect port, not the pairing one. adb
    /// exits 0 even when it fails, so success is read from what it prints.
    pub fn connect(&self, address: &str) -> Result<(), AndroidError> {
        validate_endpoint(address)?;
        let out = self.run(&["connect", address])?;
        if out.contains("connected to") && !out.contains("cannot") && !out.contains("failed") {
            Ok(())
        } else {
            Err(AndroidError::Wireless(out.trim().to_string()))
        }
    }

    /// Starts adb with both output pipes captured, for [`wait_cancellable`].
    pub(crate) fn spawn(&self, args: &[&str]) -> Result<Child, AndroidError> {
        self.command()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(AndroidError::Spawn)
    }

    fn run(&self, args: &[&str]) -> Result<String, AndroidError> {
        let out = self
            .command()
            .args(args)
            .output()
            .map_err(AndroidError::Spawn)?;
        if !out.status.success() {
            return Err(AndroidError::Adb {
                command: args.join(" "),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.exe);
        // `adb shell` forwards stdin to the phone; nothing here ever means
        // to send it anything.
        cmd.stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the release build is a GUI-subsystem app,
            // so every adb call would otherwise flash a console window.
            cmd.creation_flags(0x0800_0000);
        }
        cmd
    }
}

/// Waits for `child`, killing it the moment `cancel` is set, and calls
/// `tick` every [`POLL`] — `adb push` prints no progress when its output
/// isn't a terminal, so callers measure progress themselves from `tick`.
/// Both pipes drain on their own threads so a chatty child can never block
/// on a full pipe while this loop waits for it to exit.
pub(crate) fn wait_cancellable(
    mut child: Child,
    cancel: &AtomicBool,
    mut tick: impl FnMut(),
) -> Result<Finished, AndroidError> {
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AndroidError::Cancelled);
        }
        if let Some(status) = child.try_wait().map_err(AndroidError::Spawn)? {
            break status;
        }
        tick();
        thread::sleep(POLL);
    };
    Ok(Finished {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// `host:port` with a real port — adb gets it as its own argv entry, so this
/// is about a readable error, not injection.
fn validate_endpoint(address: &str) -> Result<(), AndroidError> {
    let ok = address.rsplit_once(':').is_some_and(|(host, port)| {
        !host.is_empty()
            && !host.contains(char::is_whitespace)
            && port.parse::<u16>().is_ok_and(|p| p > 0)
    });
    if ok {
        Ok(())
    } else {
        Err(AndroidError::Wireless(format!(
            "\"{address}\" no es una dirección válida (ej. 192.168.1.20:37123)"
        )))
    }
}

fn parse_devices(out: &str) -> Vec<Device> {
    out.lines()
        .filter(|line| !line.starts_with("List of devices") && !line.starts_with('*'))
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let serial = words.next()?.to_string();
            let state = match words.next()? {
                // `no permissions (missing udev rules? ...)` — keep it readable.
                "no" => "no permissions".to_string(),
                word => word.to_string(),
            };
            let model = words
                .find_map(|word| word.strip_prefix("model:"))
                .unwrap_or_default()
                .replace('_', " ");
            Some(Device {
                serial,
                state,
                model,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_list() {
        let out = "* daemon not running; starting now at tcp:5037\n\
                   List of devices attached\n\
                   R58M12345ABC           device usb:1-1 product:beyond1 model:SM_G973F device:beyond1 transport_id:1\n\
                   192.168.1.20:5555      unauthorized transport_id:2\n\
                   ????????????           no permissions (missing udev rules? user is in the plugdev group); see [http://developer.android.com/tools/device.html]\n\
                   \n";
        let devices = parse_devices(out);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].state, "device");
        assert_eq!(devices[0].model, "SM G973F");
        assert_eq!(devices[1].serial, "192.168.1.20:5555");
        assert_eq!(devices[1].state, "unauthorized");
        assert_eq!(devices[1].model, "");
        assert_eq!(devices[2].state, "no permissions");
    }

    #[test]
    fn endpoints() {
        assert!(validate_endpoint("192.168.1.20:37123").is_ok());
        assert!(validate_endpoint("phone.local:5555").is_ok());
        for bad in [
            "192.168.1.20",
            ":5555",
            "1.2.3.4:0",
            "1.2.3.4:99999",
            "a b:5555",
        ] {
            assert!(validate_endpoint(bad).is_err(), "{bad}");
        }
    }
}
