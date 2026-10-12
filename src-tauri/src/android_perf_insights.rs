// "Ideas para mejorar": what a recorded session says about how to configure
// the game's GameNative container better. Rule-based on purpose — each idea
// carries the numbers that triggered it, so the user can judge it, and every
// tip names a setting that really exists in GameNative (checked against its
// source): the in-game FPS limiter, the DX wrapper's `framerate` / `async`
// options, the graphics driver's Present Mode, Resolución (+ FSR), the Box64 /
// FEXCore presets, CPUs. Never applied automatically — the loop is: record,
// change one thing, record again, compare.
//
// Frame caps go through GameNative's own settings, not env vars: it deletes
// DXVK_FRAME_RATE / VKD3D_FRAME_RATE from the container's variables at launch
// and sets MESA_VK_WSI_PRESENT_MODE from the driver config. Its limiter paces
// Present notifications with a timer against a simulated 60 Hz clock, not the
// real display vsync — so Present Mode advice is "measure it", never a promise.

use serde::Serialize;
use tatu_android::perf::{Sample, Summary};

use crate::android::GameNativeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Good,
    Info,
    Warn,
}

#[derive(Debug, Clone, Serialize)]
pub struct Insight {
    pub level: Level,
    pub title: String,
    /// The numbers behind it.
    pub evidence: String,
    pub tips: Vec<String>,
}

/// Seconds to ignore at the start of a long session: loading screens and the
/// first shader compiles aren't the game's steady state.
const WARMUP_S: f32 = 10.0;
/// Frame rates people actually cap at.
const CAPS: &[f32] = &[30.0, 40.0, 45.0, 60.0, 90.0, 120.0];

pub fn analyze(
    samples: &[Sample],
    summary: &Summary,
    config: Option<&GameNativeConfig>,
) -> Vec<Insight> {
    let get = |key: &str| {
        config
            .and_then(|c| c.container.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let steady: Vec<&Sample> = if summary.duration_s > 3.0 * WARMUP_S {
        samples.iter().filter(|s| s.t >= WARMUP_S).collect()
    } else {
        samples.iter().collect()
    };
    let mut out = Vec::new();

    let with_fps: Vec<&Sample> = steady.iter().copied().filter(|s| s.fps.is_some()).collect();
    if with_fps.is_empty() {
        out.push(Insight {
            level: Level::Info,
            title: "No se midieron FPS".into(),
            evidence: "No apareció la pantalla del juego de GameNative mientras se medía.".into(),
            tips: vec![
                "Abrí el juego en GameNative antes (o durante) la medición; el resto de las métricas igual se guardan.".into(),
                "Si el juego estaba abierto, mandá la salida de \"Diagnóstico\" para revisar qué capa ve Android.".into(),
            ],
        });
    }

    let share = |pred: &dyn Fn(&Sample) -> bool| {
        let known: Vec<&&Sample> = steady
            .iter()
            .filter(|s| s.cpu_core_max_pct.is_some() || s.gpu_pct.is_some())
            .collect();
        if known.is_empty() {
            return 0.0;
        }
        known.iter().filter(|s| pred(s)).count() as f32 / known.len() as f32
    };
    let gpu_bound = share(&|s| s.gpu_pct.is_some_and(|g| g >= 90.0));
    let cpu_bound = share(&|s| {
        s.cpu_core_max_pct.is_some_and(|c| c >= 90.0) && s.gpu_pct.is_none_or(|g| g < 75.0)
    });
    let avg = |f: fn(&Sample) -> Option<f32>| {
        let v: Vec<f32> = steady.iter().filter_map(|s| f(s)).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
    };

    if gpu_bound >= 0.5 {
        let mut tips = vec![
            "Bajá \"Resolución\" del contenedor (ej. 1280x720 → 960x540) y compensá con FSR en los efectos de pantalla de GameNative.".into(),
            "Bajá la calidad gráfica dentro del juego (sombras, post-procesado, resolución de render).".into(),
        ];
        if get("dxwrapper") == "wined3d" {
            tips.push("DX wrapper en WineD3D traduce a OpenGL: probá DXVK (DirectX 9–11) o VKD3D (DirectX 12).".into());
        }
        if get("graphicsdriver") != "turnip" && steady.iter().any(|s| s.gpu_mhz.is_some()) {
            tips.push("En GPU Adreno probá el driver Turnip (y distintas versiones): suele rendir distinto que Vortek/VirGL.".into());
        }
        out.push(Insight {
            level: Level::Warn,
            title: "Limitado por la GPU".into(),
            evidence: format!(
                "La GPU estuvo al 90% o más el {:.0}% del tiempo (promedio {}).",
                gpu_bound * 100.0,
                pct(avg(|s| s.gpu_pct))
            ),
            tips,
        });
    } else if cpu_bound >= 0.5 {
        let mut tips = Vec::new();
        match get("emulator").as_str() {
            "box64" => tips.push("Emulador Box64: probá \"Preset Box64\" en PERFORMANCE (si algo falla, volvé a INTERMEDIATE).".into()),
            "fexcore" => tips.push("Emulador FEXCore: probá \"Preset FEXCore\" en PERFORMANCE (EXTREME rinde más pero es más inestable).".into()),
            _ => tips.push("Probá el preset del emulador (Box64 o FEXCore) en PERFORMANCE; si algo falla, volvé un paso atrás.".into()),
        }
        tips.push("Revisá \"CPUs\": que incluya los núcleos rápidos del dispositivo.".into());
        if get("dxwrapper") == "dxvk" {
            tips.push("Con DXVK, una versión gplasync con async activado (config del DX wrapper) saca la compilación de shaders del hilo principal.".into());
        }
        tips.push("Dentro del juego, bajá lo que carga la CPU: distancia de dibujado, densidad de NPCs/objetos, física.".into());
        out.push(Insight {
            level: Level::Warn,
            title: "Limitado por la CPU".into(),
            evidence: format!(
                "Un núcleo estuvo al 90% o más con la GPU por debajo del 75% el {:.0}% del tiempo (núcleo más cargado {}, GPU {}).",
                cpu_bound * 100.0,
                pct(avg(|s| s.cpu_core_max_pct)),
                pct(avg(|s| s.gpu_pct))
            ),
            tips,
        });
    }

    // Frame pacing: a 1% low far below the average, or seconds whose worst
    // frame is way over the average frame time.
    let mut paced_badly = false;
    if let (Some(avg_fps), Some(low)) = (summary.avg_fps, summary.low1_fps)
        && avg_fps >= 20.0
    {
        let avg_ms = 1000.0 / avg_fps;
        let spikes = with_fps
            .iter()
            .filter(|s| {
                s.frametime_max_ms
                    .is_some_and(|m| m > (2.5 * avg_ms).max(50.0))
            })
            .count();
        if low / avg_fps < 0.6 || spikes >= 3 {
            paced_badly = true;
            let mut tips = vec![format!(
                "Limitá los FPS a lo que el juego sostiene ({} fps): limitador de FPS del menú rápido de GameNative, o \"framerate\" en la config del DX wrapper si usa DXVK.",
                steady_cap(&with_fps)
            )];
            tips.push("Si jugás con tope bajo (ej. 30), usá un tope que divida justo el refresco de la pantalla y, si podés, bajá la pantalla a 60 Hz: cada cuadro tiene más margen para caer siempre en 2 refrescos.".into());
            tips.push("Probá Present Mode \"fifo\" contra \"mailbox\" y compará las dos sesiones: el vsync que ven los juegos en GameNative es simulado, así que el efecto hay que medirlo.".into());
            if get("dxwrapper") == "dxvk" || get("dxwrapper").is_empty() {
                tips.push("Si los tirones son al ver cosas nuevas, es compilación de shaders: DXVK gplasync con async activado, y repetí la escena (la caché se calienta).".into());
            }
            tips.push("Las variables DXVK_FRAME_RATE / VKD3D_FRAME_RATE no sirven en el contenedor: GameNative las borra al lanzar.".into());
            out.push(Insight {
                level: Level::Warn,
                title: "Ritmo de cuadros irregular (tirones)".into(),
                evidence: format!(
                    "1% low {low:.1} fps contra {avg_fps:.1} fps de promedio; {spikes} segundo(s) con un cuadro de más de {:.0} ms.",
                    (2.5 * avg_ms).max(50.0)
                ),
                tips,
            });
        }
    }

    // Throttling: frequency falling while temperature rises.
    if summary.duration_s >= 60.0 && steady.len() >= 30 {
        let third = steady.len() / 3;
        let (first, last) = (&steady[..third], &steady[steady.len() - third..]);
        let mean = |part: &[&Sample], f: fn(&Sample) -> Option<f32>| {
            let v: Vec<f32> = part.iter().filter_map(|s| f(s)).collect();
            (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
        };
        type Metric = fn(&Sample) -> Option<f32>;
        let parts: [(&str, Metric, Metric); 2] = [
            ("CPU", |s| s.cpu_mhz.map(|v| v as f32), |s| s.temp_cpu),
            ("GPU", |s| s.gpu_mhz.map(|v| v as f32), |s| s.temp_gpu),
        ];
        for (name, freq, temp) in parts {
            if let (Some(f0), Some(f1)) = (mean(first, freq), mean(last, freq))
                && f1 < 0.85 * f0
            {
                let heat = match (mean(first, temp), mean(last, temp)) {
                    (Some(t0), Some(t1)) if t1 - t0 >= 5.0 => {
                        format!(" mientras la temperatura subió de {t0:.0} a {t1:.0} °C")
                    }
                    _ => String::new(),
                };
                out.push(Insight {
                    level: Level::Warn,
                    title: format!("La {name} bajó su frecuencia (throttling)"),
                    evidence: format!("De {f0:.0} MHz al principio a {f1:.0} MHz al final{heat}."),
                    tips: vec![
                        "Limitá los FPS un poco por debajo del promedio: menos carga, menos calor, rendimiento más parejo.".into(),
                        "Probá un modo de rendimiento/ventilador más alto del dispositivo, o uno más bajo con FPS limitados para sesiones largas.".into(),
                    ],
                });
            }
        }
    }

    if let Some(t) = summary.max_temp_battery
        && t >= 43.0
    {
        out.push(Insight {
            level: Level::Warn,
            title: "Batería caliente".into(),
            evidence: format!(
                "La batería llegó a {t:.1} °C (por encima de ~43 °C se degrada más rápido)."
            ),
            tips: vec![
                "Limitá los FPS o bajá el modo de rendimiento.".into(),
                "Evitá jugar cargando: la carga suma calor a la batería.".into(),
            ],
        });
    }

    let ram_peak = steady
        .iter()
        .filter_map(|s| Some(s.ram_used_mb? as f32 / s.ram_total_mb? as f32))
        .reduce(f32::max);
    if let Some(r) = ram_peak
        && r >= 0.92
    {
        out.push(Insight {
            level: Level::Warn,
            title: "Memoria casi llena".into(),
            evidence: format!(
                "El dispositivo llegó al {:.0}% de la RAM en uso.",
                r * 100.0
            ),
            tips: vec![
                "Cerrá las apps en segundo plano antes de jugar.".into(),
                "Bajá la calidad de texturas dentro del juego.".into(),
            ],
        });
    }

    // Judder at a cap: the counter says 30 but frames don't arrive evenly —
    // the thing a console-style limiter (RTSS) fixes. Checked on its own,
    // because a steady cap with a fine 1% low can still feel choppy.
    let at_cap = summary.avg_fps.and_then(|avg_fps| {
        summary
            .refresh_hz
            .into_iter()
            .chain(CAPS.iter().copied())
            .find(|c| (avg_fps - c).abs() <= c * 0.05)
    });
    let uneven = matches!(summary.pacing_pct, Some(p) if p < 85.0);
    if let (Some(cap), true) = (at_cap, uneven) {
        let median = summary.median_frametime_ms.unwrap_or(1000.0 / cap);
        let mut tips = Vec::new();
        match summary.refresh_hz.filter(|hz| *hz > cap) {
            Some(hz) if ((hz / cap) - (hz / cap).round()).abs() > 0.05 => tips.push(format!(
                "La pantalla va a {hz:.0} Hz y {cap:.0} no la divide justo: usá un tope divisor ({}).",
                divisors(hz)
            )),
            Some(hz) if hz > 70.0 => tips.push(format!(
                "Bajá la pantalla a 60 Hz para jugar a {cap:.0}: a {hz:.0} Hz cada cuadro tiene la mitad de margen para caer siempre en la misma cantidad de refrescos."
            )),
            _ => {}
        }
        tips.push("Usá un solo limitador y probá los dos por separado: el del menú rápido de GameNative y \"framerate\" en la config del DX wrapper (DXVK).".into());
        tips.push("Probá Present Mode \"fifo\" contra \"mailbox\": el vsync que ven los juegos en GameNative es simulado, así que el efecto hay que medirlo.".into());
        out.push(Insight {
            level: Level::Warn,
            title: format!("Cuadros desparejos al tope de {cap:.0} fps"),
            evidence: format!(
                "Solo el {:.0}% de los cuadros llegó a la pantalla a ±10% de {median:.1} ms (desvío {}): se nota como tirones aunque el contador marque {cap:.0}.",
                summary.pacing_pct.unwrap_or(0.0),
                ms(summary.frametime_sd_ms)
            ),
            tips,
        });
    }

    // At a cap with room to spare: the budget can go to quality or battery.
    let fps_values: Vec<f32> = with_fps.iter().filter_map(|s| s.fps).collect();
    if let Some(avg_fps) = summary.avg_fps
        && !paced_badly
        && gpu_bound < 0.5
        && cpu_bound < 0.5
        && fps_values.len() >= 10
    {
        let target = summary
            .refresh_hz
            .into_iter()
            .chain(CAPS.iter().copied())
            .find(|c| (avg_fps - c).abs() <= c * 0.03);
        let steady_share = fps_values
            .iter()
            .filter(|f| (*f - avg_fps).abs() <= avg_fps * 0.05)
            .count() as f32
            / fps_values.len() as f32;
        if let Some(cap) = target
            && steady_share >= 0.9
            && !uneven
        {
            out.push(Insight {
                level: Level::Good,
                title: format!("Estable en el tope de {cap:.0} fps, con margen"),
                evidence: format!(
                    "El {:.0}% de los segundos quedó a ±5% de {cap:.0} fps; ni la CPU ni la GPU están al límite.",
                    steady_share * 100.0
                ),
                tips: vec![
                    "Hay margen: podés subir \"Resolución\" o la calidad dentro del juego.".into(),
                    "O al revés: bajá el modo de rendimiento del dispositivo para gastar menos batería y calentar menos.".into(),
                ],
            });
        }
    }

    if out.is_empty() && summary.avg_fps.is_some() {
        out.push(Insight {
            level: Level::Good,
            title: "Sin problemas claros".into(),
            evidence: format!(
                "{} fps de promedio, 1% low {} fps, sin cuello de botella marcado.",
                num(summary.avg_fps),
                num(summary.low1_fps)
            ),
            tips: vec![
                "Para mejorar más, cambiá una sola cosa, medí de nuevo y compará las dos sesiones."
                    .into(),
            ],
        });
    }
    out
}

/// The highest usual cap the game holds 90% of the time.
fn steady_cap(with_fps: &[&Sample]) -> f32 {
    let mut fps: Vec<f32> = with_fps.iter().filter_map(|s| s.fps).collect();
    fps.sort_by(f32::total_cmp);
    let p10 = fps.get(fps.len() / 10).copied().unwrap_or(30.0);
    CAPS.iter()
        .rev()
        .copied()
        .find(|c| *c <= p10)
        .unwrap_or(30.0)
}

/// Frame rates that divide a refresh rate exactly (e.g. 120 Hz → 30, 40, 60).
fn divisors(hz: f32) -> String {
    let list: Vec<String> = (2..=4)
        .map(|n| hz / n as f32)
        .filter(|fps| *fps >= 24.0)
        .map(|fps| format!("{fps:.0}"))
        .collect();
    list.join(", ")
}

fn ms(v: Option<f32>) -> String {
    v.map_or("—".into(), |v| format!("{v:.1} ms"))
}

fn pct(v: Option<f32>) -> String {
    v.map_or("—".into(), |v| format!("{v:.0}%"))
}

fn num(v: Option<f32>) -> String {
    v.map_or("—".into(), |v| format!("{v:.1}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(n: usize, f: impl Fn(usize) -> Sample) -> (Vec<Sample>, Summary) {
        let samples: Vec<Sample> = (0..n)
            .map(|i| Sample {
                t: i as f32,
                ..f(i)
            })
            .collect();
        let summary = Summary {
            duration_s: (n - 1) as f32,
            avg_fps: Some(samples.iter().filter_map(|s| s.fps).sum::<f32>() / n as f32),
            ..Summary::default()
        };
        (samples, summary)
    }

    fn titles(v: &[Insight]) -> Vec<&str> {
        v.iter().map(|i| i.title.as_str()).collect()
    }

    fn config(pairs: &[(&str, &str)]) -> GameNativeConfig {
        let mut c = GameNativeConfig::default();
        for (k, v) in pairs {
            c.container
                .insert(k.to_string(), serde_json::Value::String(v.to_string()));
        }
        c
    }

    #[test]
    fn gpu_bound_suggests_resolution_and_driver() {
        let (s, mut sum) = run(60, |_| Sample {
            fps: Some(28.0),
            gpu_pct: Some(98.0),
            gpu_mhz: Some(680),
            cpu_core_max_pct: Some(60.0),
            ..Sample::default()
        });
        sum.low1_fps = Some(25.0);
        let out = analyze(
            &s,
            &sum,
            Some(&config(&[
                ("graphicsDriver", "vortek"),
                ("dxwrapper", "wined3d"),
            ])),
        );
        assert_eq!(titles(&out), vec!["Limitado por la GPU"]);
        assert!(out[0].tips.iter().any(|t| t.contains("Turnip")));
        assert!(out[0].tips.iter().any(|t| t.contains("WineD3D")));
    }

    #[test]
    fn cpu_bound_names_the_emulators_preset() {
        let (s, mut sum) = run(60, |_| Sample {
            fps: Some(35.0),
            gpu_pct: Some(55.0),
            cpu_core_max_pct: Some(97.0),
            ..Sample::default()
        });
        sum.low1_fps = Some(30.0);
        let out = analyze(&s, &sum, Some(&config(&[("emulator", "FEXCore")])));
        assert_eq!(titles(&out), vec!["Limitado por la CPU"]);
        assert!(out[0].tips[0].contains("Preset FEXCore"));
    }

    #[test]
    fn stutter_suggests_a_cap_the_game_holds() {
        let (s, mut sum) = run(60, |i| Sample {
            fps: Some(if i % 6 == 0 { 38.0 } else { 55.0 }),
            frametime_max_ms: Some(if i % 5 == 0 { 120.0 } else { 20.0 }),
            gpu_pct: Some(70.0),
            ..Sample::default()
        });
        sum.low1_fps = Some(18.0);
        let out = analyze(&s, &sum, None);
        let pacing = out
            .iter()
            .find(|i| i.title.starts_with("Ritmo"))
            .expect("pacing insight");
        assert!(pacing.tips[0].contains("(30 fps)"), "{:?}", pacing.tips[0]);
    }

    #[test]
    fn throttling_and_a_steady_cap() {
        let (s, mut sum) = run(120, |i| Sample {
            fps: Some(60.0),
            frametime_max_ms: Some(18.0),
            cpu_mhz: Some(if i < 60 { 3000 } else { 2000 }),
            temp_cpu: Some(if i < 60 { 60.0 } else { 80.0 }),
            gpu_pct: Some(60.0),
            ..Sample::default()
        });
        sum.low1_fps = Some(57.0);
        sum.refresh_hz = Some(60.0);
        let out = analyze(&s, &sum, None);
        let t = titles(&out);
        assert!(
            t.contains(&"La CPU bajó su frecuencia (throttling)"),
            "{t:?}"
        );
        assert!(
            t.iter().any(|x| x.starts_with("Estable en el tope de 60")),
            "{t:?}"
        );
    }

    #[test]
    fn judder_at_a_30_cap_is_not_called_stable() {
        let (s, mut sum) = run(60, |_| Sample {
            fps: Some(30.0),
            frametime_max_ms: Some(50.0),
            gpu_pct: Some(50.0),
            ..Sample::default()
        });
        sum.low1_fps = Some(20.0);
        sum.refresh_hz = Some(120.0);
        sum.pacing_pct = Some(52.0);
        sum.median_frametime_ms = Some(33.3);
        sum.frametime_sd_ms = Some(14.0);
        let out = analyze(&s, &sum, None);
        let t = titles(&out);
        assert!(t.contains(&"Cuadros desparejos al tope de 30 fps"), "{t:?}");
        assert!(!t.iter().any(|x| x.starts_with("Estable")), "{t:?}");
        let judder = out.iter().find(|i| i.title.starts_with("Cuadros")).unwrap();
        assert!(judder.tips[0].contains("60 Hz"), "{:?}", judder.tips[0]);
    }

    #[test]
    fn no_fps_says_so() {
        let (s, sum) = run(20, |_| Sample {
            cpu_pct: Some(30.0),
            ..Sample::default()
        });
        let sum = Summary {
            avg_fps: None,
            ..sum
        };
        assert_eq!(titles(&analyze(&s, &sum, None)), vec!["No se midieron FPS"]);
    }
}
