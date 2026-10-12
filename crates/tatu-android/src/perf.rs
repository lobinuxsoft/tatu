//! Performance of a game running in GameNative, sampled over adb — no root,
//! nothing installed on the phone. Every source is read best-effort: a
//! device that doesn't expose one simply leaves that metric empty.
//!
//! - FPS / frame times: SurfaceFlinger's per-layer present timestamps
//!   (`dumpsys SurfaceFlinger --latency <layer>`) for GameNative's game
//!   surface. That surface (`XServerView`) is `RENDERMODE_WHEN_DIRTY`, so it
//!   presents once per new game frame — the same draws GameNative's own FPS
//!   counter counts.
//! - CPU: `/proc/stat` deltas + cpufreq policies (current and max: the max
//!   is where performance modes / TDP limits show up).
//! - GPU: Adreno kgsl (`gpu_busy_percentage`, else `gpubusy` busy/total),
//!   Mali `utilisation`.
//! - RAM: `/proc/meminfo`. Temperatures: `dumpsys thermalservice` (AOSP
//!   policy doesn't let the shell read thermal zones directly).
//! - Power: battery `current_now` × `voltage_now`, only while nothing is
//!   plugged in (`dumpsys battery`) — over a USB cable it's charge current,
//!   not the device's draw.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::adb::Adb;
use crate::remote::quote;

/// FPS is read every half tick: SurfaceFlinger keeps only the last 127
/// frames, ~1 s at 120 fps — polling once a second would drop frames.
const HALF_TICK: Duration = Duration::from_millis(500);
/// A tick that takes longer than this is a dead link, not a slow phone.
const DEADLINE: Duration = Duration::from_secs(4);
/// This many failed reads in a row end the session (data so far is kept).
const MAX_FAILURES: u32 = 5;
/// `--latency` returning no frames this many times in a row means the layer
/// is gone (game closed, surface recreated) — look it up again.
const EMPTY_LATENCY_REDISCOVER: u32 = 3;

const GAMENATIVE: &str = "app.gamenative";

/// Everything one second of sampling produced. `None` = that source isn't
/// readable on this device (or wasn't at that moment).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sample {
    /// Seconds since recording started.
    pub t: f32,
    pub fps: Option<f32>,
    /// Worst frame time within this second — where stutter shows up.
    pub frametime_max_ms: Option<f32>,
    /// Standard deviation of this second's frame times: 0 = every frame took
    /// the same time (even pacing), a 30 fps judder of 16.7/50 ms ≈ 16.
    pub frametime_sd_ms: Option<f32>,
    pub cpu_pct: Option<f32>,
    /// The busiest core's load — one core pegged = CPU-bound game thread.
    pub cpu_core_max_pct: Option<f32>,
    pub cpu_mhz: Option<u32>,
    pub cpu_cap_mhz: Option<u32>,
    pub gpu_pct: Option<f32>,
    pub gpu_mhz: Option<u32>,
    pub gpu_cap_mhz: Option<u32>,
    pub ram_used_mb: Option<u32>,
    pub ram_total_mb: Option<u32>,
    pub power_w: Option<f32>,
    /// Plugged in (USB/AC/wireless): power can't be measured then.
    pub powered: bool,
    pub battery_pct: Option<u8>,
    pub temp_cpu: Option<f32>,
    pub temp_gpu: Option<f32>,
    pub temp_battery: Option<f32>,
    pub temp_skin: Option<f32>,
    pub gamenative_running: bool,
    pub layer_found: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub duration_s: f32,
    /// Frames counted for FPS / 1% low (frame times across a gap excluded).
    pub frames: u32,
    pub avg_fps: Option<f32>,
    /// 1000 / mean of the slowest 1% of frame times. `None` under 100 frames.
    pub low1_fps: Option<f32>,
    /// The cadence the game actually ran at (33.3 ms for a 30 fps cap).
    pub median_frametime_ms: Option<f32>,
    /// Pacing: % of frames within ±10% of the median frame time — how
    /// "console-like" a cap feels. See [`pacing`].
    pub pacing_pct: Option<f32>,
    pub frametime_sd_ms: Option<f32>,
    pub min_fps: Option<f32>,
    pub max_fps: Option<f32>,
    pub refresh_hz: Option<f32>,
    pub avg_cpu_pct: Option<f32>,
    pub avg_cpu_core_max_pct: Option<f32>,
    pub avg_cpu_mhz: Option<f32>,
    pub cpu_cap_mhz: Option<u32>,
    pub avg_gpu_pct: Option<f32>,
    pub avg_gpu_mhz: Option<f32>,
    pub gpu_cap_mhz: Option<u32>,
    pub max_ram_used_mb: Option<u32>,
    pub avg_power_w: Option<f32>,
    pub max_temp_cpu: Option<f32>,
    pub max_temp_gpu: Option<f32>,
    pub max_temp_battery: Option<f32>,
    pub max_temp_skin: Option<f32>,
}

/// Running FPS figures over everything recorded so far — what the live view
/// shows next to each second's sample.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct LiveStats {
    pub avg_fps: Option<f32>,
    pub low1_fps: Option<f32>,
    pub pacing_pct: Option<f32>,
    pub median_frametime_ms: Option<f32>,
    pub frames: u32,
}

pub struct Recording {
    pub samples: Vec<Sample>,
    pub summary: Summary,
    /// Why recording ended early (device gone), if it did.
    pub error: Option<String>,
}

/// Records until `stop` is set or the device stops answering. `on_sample`
/// sees each second's sample as it's made, with the running FPS stats.
pub fn record(
    adb: &Adb,
    serial: &str,
    stop: &AtomicBool,
    mut on_sample: impl FnMut(&Sample, &LiveStats),
) -> Recording {
    let start = Instant::now();
    let mut probe = Probe::default();
    let mut samples = Vec::new();
    let mut error = None;
    let mut failures = 0;
    let mut full_tick = true;
    let mut next = start;

    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now < next {
            thread::sleep((next - now).min(Duration::from_millis(100)));
            continue;
        }
        // From now, not from the last deadline: a slow tick must not be
        // followed by a burst of catch-up ones (FPS uses device timestamps,
        // so tick drift doesn't matter).
        next = now + HALF_TICK;

        let result = if full_tick {
            probe.full_tick(adb, serial, start.elapsed().as_secs_f32())
        } else {
            probe.fps_tick(adb, serial).map(|()| None)
        };
        full_tick = !full_tick;
        match result {
            Ok(Some(sample)) => {
                failures = 0;
                let (avg_fps, low1_fps) = fps_stats(&probe.frames.all_ms);
                let pace = pacing(&probe.frames.all_ms);
                let live = LiveStats {
                    avg_fps,
                    low1_fps,
                    pacing_pct: pace.map(|p| p.within_pct),
                    median_frametime_ms: pace.map(|p| p.median_ms),
                    frames: probe.frames.all_ms.len() as u32,
                };
                on_sample(&sample, &live);
                samples.push(sample);
            }
            Ok(None) => failures = 0,
            Err(e) => {
                failures += 1;
                if failures >= MAX_FAILURES {
                    error = Some(e);
                    break;
                }
            }
        }
    }

    let summary = summarize(&samples, &probe.frames.all_ms, probe.refresh_hz);
    Recording {
        samples,
        summary,
        error,
    }
}

/// The raw output of one full probe plus the layer list — what the
/// diagnostics view shows, so a device where something reads empty can be
/// reported back as-is.
pub fn probe_raw(adb: &Adb, serial: &str) -> Result<String, crate::AndroidError> {
    let list = adb.shell_deadline(serial, "dumpsys SurfaceFlinger --list; exit 0", DEADLINE)?;
    let layer = find_layer(&list);
    let out = adb.shell_deadline(serial, &full_script(layer.as_deref()), DEADLINE)?;
    let matches: Vec<&str> = list.lines().filter(|l| l.contains(GAMENATIVE)).collect();
    Ok(format!(
        "== capa elegida: {}\n== capas de GameNative en --list:\n{}\n\n{out}",
        layer.as_deref().unwrap_or("(ninguna)"),
        matches.join("\n")
    ))
}

#[derive(Default)]
struct Probe {
    layer: Option<String>,
    empty_latency: u32,
    frames: FrameTracker,
    refresh_hz: Option<f32>,
    cpu: Option<CpuTimes>,
}

impl Probe {
    fn full_tick(&mut self, adb: &Adb, serial: &str, t: f32) -> Result<Option<Sample>, String> {
        if self.layer.is_none() {
            let list = adb
                .shell_deadline(serial, "dumpsys SurfaceFlinger --list; exit 0", DEADLINE)
                .map_err(|e| e.to_string())?;
            self.layer = find_layer(&list);
            self.frames.reset_baseline();
        }
        let out = adb
            .shell_deadline(serial, &full_script(self.layer.as_deref()), DEADLINE)
            .map_err(|e| e.to_string())?;
        let s = sections(&out);
        if let Some(lines) = s.get("fps") {
            self.feed_latency(lines);
        }

        let mut sample = Sample {
            t,
            gamenative_running: s
                .get("gn")
                .is_some_and(|l| l.iter().any(|x| !x.trim().is_empty())),
            layer_found: self.layer.is_some(),
            ..Sample::default()
        };
        let (fps, worst, sd) = self.frames.take_second(self.layer.is_some());
        sample.fps = fps;
        sample.frametime_max_ms = worst;
        sample.frametime_sd_ms = sd;

        if let Some(cpu) = s.get("stat").and_then(|l| parse_cpu_times(l)) {
            if let Some(prev) = &self.cpu {
                (sample.cpu_pct, sample.cpu_core_max_pct) = cpu_load(prev, &cpu);
            }
            self.cpu = Some(cpu);
        }
        if let Some((total, available)) = s.get("mem").and_then(|l| parse_meminfo(l)) {
            sample.ram_total_mb = Some((total / 1024) as u32);
            sample.ram_used_mb = Some((total.saturating_sub(available) / 1024) as u32);
        }
        let files = s.get("files").map(|l| parse_files(l)).unwrap_or_default();
        apply_files(&files, &mut sample);
        if let Some(lines) = s.get("battery") {
            apply_battery(lines, &files, &mut sample);
        }
        if let Some(lines) = s.get("thermal") {
            apply_thermal(lines, &mut sample);
        }
        Ok(Some(sample))
    }

    fn fps_tick(&mut self, adb: &Adb, serial: &str) -> Result<(), String> {
        let Some(layer) = &self.layer else {
            return Ok(());
        };
        let script = format!("dumpsys SurfaceFlinger --latency {}; exit 0", quote(layer));
        let out = adb
            .shell_deadline(serial, &script, DEADLINE)
            .map_err(|e| e.to_string())?;
        let lines: Vec<&str> = out.lines().collect();
        self.feed_latency(&lines);
        Ok(())
    }

    fn feed_latency(&mut self, lines: &[&str]) {
        let (refresh_ns, rows) = parse_latency(lines);
        if let Some(ns) = refresh_ns.filter(|&ns| ns > 0) {
            self.refresh_hz = Some(1e9 / ns as f32);
        }
        if rows.is_empty() {
            self.empty_latency += 1;
            if self.empty_latency >= EMPTY_LATENCY_REDISCOVER {
                self.layer = None;
                self.empty_latency = 0;
            }
            return;
        }
        self.empty_latency = 0;
        self.frames.feed(&rows);
    }
}

fn full_script(layer: Option<&str>) -> String {
    let mut script = String::from(
        "echo @stat; grep '^cpu' /proc/stat 2>/dev/null; \
         echo @mem; grep -E '^(MemTotal|MemAvailable):' /proc/meminfo 2>/dev/null; \
         echo @files; grep -H . \
           /sys/devices/system/cpu/cpufreq/policy*/scaling_cur_freq \
           /sys/devices/system/cpu/cpufreq/policy*/scaling_max_freq \
           /sys/class/kgsl/kgsl-3d0/gpu_busy_percentage \
           /sys/class/kgsl/kgsl-3d0/gpubusy \
           /sys/class/kgsl/kgsl-3d0/gpuclk \
           /sys/class/kgsl/kgsl-3d0/max_gpuclk \
           /sys/class/misc/mali0/device/utilisation \
           /sys/class/power_supply/battery/current_now \
           /sys/class/power_supply/battery/voltage_now 2>/dev/null; \
         echo @battery; dumpsys battery 2>/dev/null; \
         echo @thermal; dumpsys thermalservice 2>/dev/null; \
         echo @gn; pidof app.gamenative app.gamenative.gold 2>/dev/null; ",
    );
    if let Some(layer) = layer {
        script += &format!(
            "echo @fps; dumpsys SurfaceFlinger --latency {} 2>/dev/null; ",
            quote(layer)
        );
    }
    script + "exit 0"
}

/// `echo @name` markers → the lines under each.
fn sections(out: &str) -> HashMap<&str, Vec<&str>> {
    let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut current = None;
    for line in out.lines() {
        if let Some(name) = line.trim().strip_prefix('@') {
            current = Some(name);
            map.entry(name).or_default();
        } else if let Some(name) = current {
            map.entry(name).or_default().push(line);
        }
    }
    map
}

/// GameNative's game surface in `dumpsys SurfaceFlinger --list`. Android
/// 12–14 print bare names; 15+ wrap them as
/// `RequestedLayerState{<name> parentId=…}`. `--latency` needs the bare
/// name, and only the `(BLAST)` child of a SurfaceView carries buffers.
fn find_layer(list: &str) -> Option<String> {
    let names: Vec<&str> = list
        .lines()
        .map(|line| {
            let line = line.trim();
            match line.strip_prefix("RequestedLayerState{") {
                Some(inner) => inner
                    .split(" parentId=")
                    .next()
                    .unwrap_or(inner)
                    .trim_end_matches('}')
                    .trim(),
                None => line,
            }
        })
        .filter(|name| name.contains("SurfaceView") && name.contains(GAMENATIVE))
        .collect();
    names
        .iter()
        .find(|n| n.contains("(BLAST)"))
        .or(names.first())
        .map(|n| n.to_string())
}

/// First line: refresh period (ns). Then `desiredPresent actualPresent
/// frameReady` per frame, oldest first.
fn parse_latency(lines: &[&str]) -> (Option<i64>, Vec<[i64; 3]>) {
    let mut refresh = None;
    let mut rows = Vec::new();
    for line in lines {
        let nums: Vec<i64> = line
            .split_whitespace()
            .filter_map(|w| w.parse().ok())
            .collect();
        match nums.as_slice() {
            [period] if refresh.is_none() && rows.is_empty() => refresh = Some(*period),
            [a, b, c] => rows.push([*a, *b, *c]),
            _ => {}
        }
    }
    (refresh, rows)
}

/// Turns overlapping `--latency` dumps into a clean stream of frame times.
#[derive(Default)]
struct FrameTracker {
    /// 1 = actualPresent, 0 = desiredPresent (devices that report actual as
    /// 0). Decided once per session, never per frame.
    column: Option<usize>,
    /// Newest frame already counted; `None` = next dump is a baseline only.
    last: Option<i64>,
    second_ms: Vec<f32>,
    second_frames: u32,
    /// This second only set the baseline: "no data yet", not 0 fps.
    baseline_second: bool,
    all_ms: Vec<f32>,
}

fn valid_ts(t: i64) -> bool {
    t > 0 && t != i64::MAX
}

impl FrameTracker {
    fn reset_baseline(&mut self) {
        self.last = None;
    }

    fn feed(&mut self, rows: &[[i64; 3]]) {
        let column = *self
            .column
            .get_or_insert(if rows.iter().any(|r| valid_ts(r[1])) {
                1
            } else {
                0
            });
        // Pending frames (INT64_MAX) and invalid ones (0 / -1) are skipped;
        // they're the newest, so `last` never moves past them.
        let ts: Vec<i64> = rows
            .iter()
            .map(|r| r[column])
            .filter(|&t| valid_ts(t))
            .collect();
        let Some(&newest) = ts.last() else { return };
        let Some(last) = self.last else {
            // First dump of a layer: all history, not frames from this tick.
            self.last = Some(newest);
            self.baseline_second = true;
            return;
        };
        // Oldest frame in the dump already newer than the last one counted:
        // the ring buffer overflowed and frames were lost in between — never
        // compute a frame time across that gap.
        let mut prev = if ts[0] > last { None } else { Some(last) };
        for &t in ts.iter().filter(|&&t| t > last) {
            if let Some(p) = prev {
                let ms = (t - p) as f32 / 1e6;
                self.second_ms.push(ms);
                self.all_ms.push(ms);
            }
            self.second_frames += 1;
            prev = Some(t);
        }
        self.last = Some(newest);
    }

    /// FPS, worst frame time and frame-time deviation for the second just
    /// ended, from the device's own timestamps (PC-side tick timing jitters
    /// with adb). No new frames on a live layer is a real 0 fps (nothing new
    /// drawn); no layer is "unknown".
    fn take_second(&mut self, layer_present: bool) -> (Option<f32>, Option<f32>, Option<f32>) {
        let ms = std::mem::take(&mut self.second_ms);
        let frames = std::mem::take(&mut self.second_frames);
        let baseline = std::mem::take(&mut self.baseline_second);
        if !ms.is_empty() {
            let mean = ms.iter().sum::<f32>() / ms.len() as f32;
            let worst = ms.iter().copied().fold(0.0, f32::max);
            let sd = (ms.len() >= 2).then(|| std_dev(&ms, mean));
            (Some(1000.0 / mean), Some(worst), sd)
        } else if layer_present && self.last.is_some() && frames == 0 && !baseline {
            (Some(0.0), None, None)
        } else {
            (None, None, None)
        }
    }
}

#[derive(Clone)]
struct CpuTimes {
    /// (total, idle) jiffies: the `cpu` line, then each `cpuN`.
    all: (u64, u64),
    cores: HashMap<String, (u64, u64)>,
}

fn parse_cpu_times(lines: &[&str]) -> Option<CpuTimes> {
    let mut all = None;
    let mut cores = HashMap::new();
    for line in lines {
        let mut words = line.split_whitespace();
        let Some(name) = words.next() else { continue };
        let v: Vec<u64> = words.filter_map(|w| w.parse().ok()).collect();
        if v.len() < 5 {
            continue;
        }
        let total: u64 = v.iter().take(8).sum();
        let idle = v[3] + v[4]; // idle + iowait
        if name == "cpu" {
            all = Some((total, idle));
        } else {
            cores.insert(name.to_string(), (total, idle));
        }
    }
    Some(CpuTimes { all: all?, cores })
}

fn busy_pct(prev: (u64, u64), now: (u64, u64)) -> Option<f32> {
    let total = now.0.checked_sub(prev.0)?;
    let idle = now.1.checked_sub(prev.1)?;
    (total > 0).then(|| (total.saturating_sub(idle)) as f32 * 100.0 / total as f32)
}

fn cpu_load(prev: &CpuTimes, now: &CpuTimes) -> (Option<f32>, Option<f32>) {
    let total = busy_pct(prev.all, now.all);
    // Cores that went offline or came online between samples are skipped.
    let max_core = now
        .cores
        .iter()
        .filter_map(|(name, &t)| busy_pct(*prev.cores.get(name)?, t))
        .fold(None, |acc: Option<f32>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        });
    (total, max_core)
}

/// (MemTotal, MemAvailable) in kB.
fn parse_meminfo(lines: &[&str]) -> Option<(u64, u64)> {
    let value = |key: &str| {
        lines
            .iter()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
    };
    Some((value("MemTotal:")?, value("MemAvailable:")?))
}

/// `grep -H .` output: `path:value`.
fn parse_files<'a>(lines: &[&'a str]) -> Vec<(&'a str, &'a str)> {
    lines
        .iter()
        .filter_map(|l| l.split_once(':'))
        .map(|(path, value)| (path, value.trim()))
        .collect()
}

fn apply_files(files: &[(&str, &str)], s: &mut Sample) {
    let ints = |suffix: &str| -> Vec<i64> {
        files
            .iter()
            .filter(|(p, _)| p.ends_with(suffix))
            .filter_map(|(_, v)| v.split_whitespace().next()?.parse().ok())
            .collect()
    };
    let one = |suffix: &str| {
        files
            .iter()
            .find(|(p, _)| p.ends_with(suffix))
            .map(|(_, v)| *v)
    };

    // cpufreq is in kHz; the fastest cluster is the one that matters.
    s.cpu_mhz = ints("/scaling_cur_freq")
        .into_iter()
        .max()
        .map(|k| (k / 1000) as u32);
    s.cpu_cap_mhz = ints("/scaling_max_freq")
        .into_iter()
        .max()
        .map(|k| (k / 1000) as u32);

    s.gpu_pct = one("/gpu_busy_percentage")
        .and_then(|v| v.split_whitespace().next()?.parse::<f32>().ok())
        .or_else(|| {
            // `gpubusy` is "busy total" over a short window — a ratio.
            let v = one("/kgsl-3d0/gpubusy")?;
            let mut n = v.split_whitespace().filter_map(|w| w.parse::<f64>().ok());
            let (busy, total) = (n.next()?, n.next()?);
            (total > 0.0).then(|| (busy * 100.0 / total) as f32)
        })
        .or_else(|| {
            let digits: String = one("/utilisation")?
                .chars()
                .filter(char::is_ascii_digit)
                .collect();
            digits.parse::<f32>().ok()
        })
        .map(|p| p.clamp(0.0, 100.0));
    // kgsl clocks are in Hz.
    s.gpu_mhz = ints("/gpuclk")
        .first()
        .filter(|&&hz| hz > 0)
        .map(|hz| (hz / 1_000_000) as u32);
    s.gpu_cap_mhz = ints("/max_gpuclk")
        .first()
        .filter(|&&hz| hz > 0)
        .map(|hz| (hz / 1_000_000) as u32);
}

fn apply_battery(lines: &[&str], files: &[(&str, &str)], s: &mut Sample) {
    let field = |key: &str| {
        lines
            .iter()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(str::trim)
    };
    s.powered = lines.iter().any(|l| l.contains("powered: true"));
    s.battery_pct = field("level:").and_then(|v| v.parse().ok());
    s.temp_battery = field("temperature:")
        .and_then(|v| v.parse::<f32>().ok())
        .map(|t| t / 10.0);

    // BatteryManager.BATTERY_STATUS_DISCHARGING = 3. Anything plugged in
    // (USB included — the usual adb setup) turns the reading into charge
    // current, so power stays unknown then.
    let discharging = field("status:") == Some("3");
    if s.powered || !discharging {
        return;
    }
    let file = |suffix: &str| {
        files
            .iter()
            .find(|(p, _)| p.ends_with(suffix))
            .and_then(|(_, v)| v.parse::<i64>().ok())
    };
    let Some(current) = file("/current_now").map(i64::abs).filter(|&c| c > 0) else {
        return;
    };
    // µA on most devices, mA on some: a game never draws under 20 mA.
    let current_ua = if current < 20_000 {
        current * 1000
    } else {
        current
    };
    let voltage_uv = file("/voltage_now")
        .or_else(|| field("voltage:").and_then(|v| v.parse::<i64>().ok()))
        .map(|v| if v < 20_000 { v * 1000 } else { v });
    if let Some(v) = voltage_uv.filter(|&v| v > 0) {
        s.power_w = Some((current_ua as f64 / 1e6 * v as f64 / 1e6) as f32);
    }
}

/// `Temperature{mValue=45.5, mType=0, mName=cpu0, mStatus=0}` — types per
/// `android.os.Temperature`: 0 CPU, 1 GPU, 2 battery, 3 skin. The HAL's
/// current readings when the dump has them, otherwise whatever it lists.
fn apply_thermal(lines: &[&str], s: &mut Sample) {
    let hal_start = lines
        .iter()
        .position(|l| l.contains("Current temperatures from HAL"));
    let scope: &[&str] = match hal_start {
        Some(i) => {
            let rest = &lines[i + 1..];
            let end = rest
                .iter()
                .position(|l| !l.contains("Temperature{"))
                .unwrap_or(rest.len());
            &rest[..end]
        }
        None => lines,
    };
    let mut max: [Option<f32>; 4] = [None; 4];
    for line in scope {
        let get = |key: &str| {
            let rest = &line[line.find(key)? + key.len()..];
            rest.split([',', '}']).next().map(str::trim)
        };
        let (Some(value), Some(kind)) = (
            get("mValue=").and_then(|v| v.parse::<f32>().ok()),
            get("mType=").and_then(|v| v.parse::<usize>().ok()),
        ) else {
            continue;
        };
        if kind < 4 && value.is_finite() && value > -40.0 && value < 150.0 {
            max[kind] = Some(max[kind].map_or(value, |m| m.max(value)));
        }
    }
    s.temp_cpu = max[0];
    s.temp_gpu = max[1];
    s.temp_battery = max[2].or(s.temp_battery);
    s.temp_skin = max[3];
}

/// Average FPS (total frames / total time) and 1% low: 1000 / mean of the
/// slowest 1% of frame times, only from 100 frames up. Linear-time partial
/// selection, not a full sort — the live view runs this every second over
/// the whole session.
fn fps_stats(frame_ms: &[f32]) -> (Option<f32>, Option<f32>) {
    if frame_ms.is_empty() {
        return (None, None);
    }
    let mean = frame_ms.iter().sum::<f32>() / frame_ms.len() as f32;
    let low = (frame_ms.len() >= 100).then(|| {
        let worst_n = frame_ms.len().div_ceil(100);
        let mut v = frame_ms.to_vec();
        v.select_nth_unstable_by(worst_n - 1, |a, b| b.total_cmp(a));
        1000.0 / (v[..worst_n].iter().sum::<f32>() / worst_n as f32)
    });
    (Some(1000.0 / mean), low)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pacing {
    pub median_ms: f32,
    pub within_pct: f32,
    pub sd_ms: f32,
}

/// How even the frames are: the share within ±10% of the median frame time
/// (the cadence the game really runs at — 33.3 ms for a 30 fps cap), plus
/// the standard deviation. The timestamps are when each frame reached the
/// real display, so a "30 fps" that alternates 16.7/50 ms (the judder a
/// console-style limiter avoids) scores ~50%, not 100%. From 30 frames up.
pub fn pacing(frame_ms: &[f32]) -> Option<Pacing> {
    if frame_ms.len() < 30 {
        return None;
    }
    let mut v = frame_ms.to_vec();
    let mid = v.len() / 2;
    v.select_nth_unstable_by(mid, f32::total_cmp);
    let median = v[mid];
    let tolerance = (median * 0.1).max(1.0);
    let within = frame_ms
        .iter()
        .filter(|&&ms| (ms - median).abs() <= tolerance)
        .count();
    let mean = frame_ms.iter().sum::<f32>() / frame_ms.len() as f32;
    Some(Pacing {
        median_ms: median,
        within_pct: within as f32 * 100.0 / frame_ms.len() as f32,
        sd_ms: std_dev(frame_ms, mean),
    })
}

fn std_dev(values: &[f32], mean: f32) -> f32 {
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32).sqrt()
}

fn summarize(samples: &[Sample], frame_ms: &[f32], refresh_hz: Option<f32>) -> Summary {
    let avg = |f: fn(&Sample) -> Option<f32>| {
        let v: Vec<f32> = samples.iter().filter_map(f).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
    };
    let max = |f: fn(&Sample) -> Option<f32>| samples.iter().filter_map(f).reduce(f32::max);
    let per_second_fps: Vec<f32> = samples.iter().filter_map(|s| s.fps).collect();

    let (avg_fps, low1_fps) = fps_stats(frame_ms);
    let pace = pacing(frame_ms);

    Summary {
        duration_s: samples.last().map_or(0.0, |s| s.t),
        frames: frame_ms.len() as u32,
        avg_fps,
        low1_fps,
        median_frametime_ms: pace.map(|p| p.median_ms),
        pacing_pct: pace.map(|p| p.within_pct),
        frametime_sd_ms: pace.map(|p| p.sd_ms),
        min_fps: per_second_fps.iter().copied().reduce(f32::min),
        max_fps: per_second_fps.iter().copied().reduce(f32::max),
        refresh_hz,
        avg_cpu_pct: avg(|s| s.cpu_pct),
        avg_cpu_core_max_pct: avg(|s| s.cpu_core_max_pct),
        avg_cpu_mhz: avg(|s| s.cpu_mhz.map(|v| v as f32)),
        cpu_cap_mhz: samples.iter().filter_map(|s| s.cpu_cap_mhz).max(),
        avg_gpu_pct: avg(|s| s.gpu_pct),
        avg_gpu_mhz: avg(|s| s.gpu_mhz.map(|v| v as f32)),
        gpu_cap_mhz: samples.iter().filter_map(|s| s.gpu_cap_mhz).max(),
        max_ram_used_mb: samples.iter().filter_map(|s| s.ram_used_mb).max(),
        avg_power_w: avg(|s| s.power_w),
        max_temp_cpu: max(|s| s.temp_cpu),
        max_temp_gpu: max(|s| s.temp_gpu),
        max_temp_battery: max(|s| s.temp_battery),
        max_temp_skin: max(|s| s.temp_skin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_names_across_android_versions() {
        let a14 = "Display 0\nSurfaceView[app.gamenative/app.gamenative.MainActivity]#88\n\
                   SurfaceView[app.gamenative/app.gamenative.MainActivity](BLAST)#89\n\
                   app.gamenative/app.gamenative.MainActivity#80";
        assert_eq!(
            find_layer(a14).as_deref(),
            Some("SurfaceView[app.gamenative/app.gamenative.MainActivity](BLAST)#89")
        );
        let a15 = "RequestedLayerState{SurfaceView[app.gamenative/app.gamenative.MainActivity]#122 parentId=120}\n\
                   RequestedLayerState{SurfaceView[app.gamenative/app.gamenative.MainActivity](BLAST)#123 parentId=122}";
        assert_eq!(
            find_layer(a15).as_deref(),
            Some("SurfaceView[app.gamenative/app.gamenative.MainActivity](BLAST)#123")
        );
        assert_eq!(find_layer("StatusBar#1\nNavigationBar0#2"), None);
    }

    #[test]
    fn frames_baseline_gaps_and_pending() {
        let ms = 1_000_000i64;
        let rows = |ts: &[i64]| -> Vec<[i64; 3]> { ts.iter().map(|&t| [t, t, t]).collect() };
        let mut f = FrameTracker::default();

        // First dump is history only — and that second is "no data", not 0 fps.
        f.feed(&rows(&[100 * ms, 116 * ms, 133 * ms]));
        assert_eq!(f.take_second(true), (None, None, None));
        // A later second with no new frame on a live layer is a real 0 fps.
        f.feed(&rows(&[100 * ms, 116 * ms, 133 * ms]));
        assert_eq!(f.take_second(true), (Some(0.0), None, None));

        // Overlapping dump + a pending frame at the end.
        let mut next = rows(&[116 * ms, 133 * ms, 150 * ms, 166 * ms]);
        next.push([i64::MAX, i64::MAX, i64::MAX]);
        f.feed(&next);
        let (fps, worst, sd) = f.take_second(true);
        assert_eq!(sd, Some(0.5));
        assert!((fps.unwrap() - 1000.0 / 16.5).abs() < 0.1, "{fps:?}");
        assert_eq!(worst, Some(17.0));

        // Ring buffer overflowed: nothing overlaps the last frame counted, so
        // the jump 166 → 500 is not a frame time.
        f.feed(&rows(&[500 * ms, 533 * ms]));
        assert_eq!(f.all_ms, vec![17.0, 16.0, 33.0]);
    }

    #[test]
    fn cpu_memory_gpu_power_thermal() {
        let before = [
            "cpu  100 0 100 800 0 0 0 0",
            "cpu0 50 0 50 400 0 0 0 0",
            "cpu1 50 0 50 400 0 0 0 0",
        ];
        let after = [
            "cpu  200 0 200 1400 0 0 0 0",
            "cpu0 150 0 150 400 0 0 0 0",
            "cpu1 50 0 50 1000 0 0 0 0",
        ];
        let (total, core) = cpu_load(
            &parse_cpu_times(&before).unwrap(),
            &parse_cpu_times(&after).unwrap(),
        );
        assert_eq!(total, Some(25.0));
        assert_eq!(core, Some(100.0));

        assert_eq!(
            parse_meminfo(&["MemTotal:  11700000 kB", "MemAvailable: 5850000 kB"]),
            Some((11_700_000, 5_850_000))
        );

        let files_raw = [
            "/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq:1804800",
            "/sys/devices/system/cpu/cpufreq/policy7/scaling_cur_freq:3187200",
            "/sys/devices/system/cpu/cpufreq/policy7/scaling_max_freq:3187200",
            "/sys/class/kgsl/kgsl-3d0/gpubusy:  3200  8000",
            "/sys/class/kgsl/kgsl-3d0/gpuclk:680000000",
            "/sys/class/power_supply/battery/current_now:-1500000",
            "/sys/class/power_supply/battery/voltage_now:4000000",
        ];
        let files = parse_files(&files_raw);
        let mut s = Sample::default();
        apply_files(&files, &mut s);
        assert_eq!(
            (s.cpu_mhz, s.cpu_cap_mhz, s.gpu_mhz),
            (Some(3187), Some(3187), Some(680))
        );
        assert_eq!(s.gpu_pct, Some(40.0));

        let unplugged = [
            "  AC powered: false",
            "  USB powered: false",
            "  status: 3",
            "  level: 81",
            "  temperature: 352",
        ];
        apply_battery(&unplugged, &files, &mut s);
        assert_eq!(s.power_w, Some(6.0));
        assert_eq!((s.battery_pct, s.temp_battery), (Some(81), Some(35.2)));

        let mut plugged = Sample::default();
        apply_battery(
            &["  USB powered: true", "  status: 2"],
            &files,
            &mut plugged,
        );
        assert!(plugged.powered && plugged.power_w.is_none());

        let thermal = [
            "Cached temperatures:",
            "\tTemperature{mValue=99.0, mType=0, mName=stale, mStatus=0}",
            "Current temperatures from HAL:",
            "\tTemperature{mValue=61.2, mType=0, mName=cpu-1-0, mStatus=0}",
            "\tTemperature{mValue=58.0, mType=0, mName=cpu-0-0, mStatus=0}",
            "\tTemperature{mValue=55.4, mType=1, mName=gpu, mStatus=0}",
            "\tTemperature{mValue=40.1, mType=3, mName=skin, mStatus=0}",
            "Current cooling devices from HAL:",
        ];
        apply_thermal(&thermal, &mut s);
        assert_eq!(
            (s.temp_cpu, s.temp_gpu, s.temp_skin),
            (Some(61.2), Some(55.4), Some(40.1))
        );
    }

    #[test]
    fn one_percent_low_is_the_slowest_frames() {
        // 99 frames at 10 ms + 1 at 100 ms: avg ~91.7 fps, 1% low = 10 fps.
        let mut ms = vec![10.0f32; 99];
        ms.push(100.0);
        let s = summarize(&[], &ms, Some(60.0));
        assert!((s.avg_fps.unwrap() - 1000.0 / 10.9).abs() < 0.01);
        assert_eq!(s.low1_fps, Some(10.0));
        assert_eq!(summarize(&[], &ms[..50], None).low1_fps, None);
    }

    #[test]
    fn pacing_tells_even_frames_from_judder() {
        // A clean 30 fps cap: every frame one 33.3 ms step.
        let even = pacing(&[33.3; 60]).unwrap();
        assert_eq!(even.within_pct, 100.0);
        assert!(even.sd_ms < 1e-3, "{even:?}");

        // Same "30 fps" on average, delivered as 16.7 / 50 ms on a 60 Hz
        // screen — what stutters even though the counter says 30.
        let judder: Vec<f32> = (0..60)
            .map(|i| if i % 2 == 0 { 16.7 } else { 50.0 })
            .collect();
        let p = pacing(&judder).unwrap();
        assert_eq!(p.within_pct, 50.0);
        assert!((p.sd_ms - 16.65).abs() < 0.01, "{p:?}");

        assert_eq!(pacing(&[33.3; 10]), None);
    }

    #[test]
    fn sections_split_on_markers() {
        let out = "@stat\ncpu 1 2 3 4 5\n@mem\n@fps\n16666666\n";
        let s = sections(out);
        assert_eq!(s["stat"], vec!["cpu 1 2 3 4 5"]);
        assert!(s["mem"].is_empty());
        assert_eq!(parse_latency(&s["fps"]), (Some(16_666_666), vec![]));
    }
}
