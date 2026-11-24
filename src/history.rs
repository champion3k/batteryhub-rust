//! Battery history: a local TSV (30 days, at most one sample per 5 minutes per device) and a
//! self-contained HTML chart opened in the default browser.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::{w, HSTRING};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::device::DeviceStatus;
use crate::i18n;

const MAX_AGE_SEC: i64 = 30 * 24 * 3600;
const SAMPLE_EVERY_SEC: i64 = 5 * 60;
const CHANGE_SAMPLE_EVERY_SEC: i64 = 60;
const CHARGE_SAMPLE_EVERY_SEC: i64 = 60;

#[derive(Clone, Debug)]
pub struct Sample {
    pub ts: i64,
    pub key: String,
    pub name: String,
    pub percent: i32,
    pub charging: bool,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn data_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("BatteryHub").join("Taskbar").join("battery_history.tsv")
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ").trim().to_string()
}

pub struct History {
    pub samples: Vec<Sample>,
    last: BTreeMap<String, i64>,
    last_percent: BTreeMap<String, i32>,
    last_charging: BTreeMap<String, bool>,
}

impl Default for History {
    fn default() -> Self {
        Self::load()
    }
}

impl History {
    fn empty() -> Self {
        Self { samples: Vec::new(), last: BTreeMap::new(), last_percent: BTreeMap::new(), last_charging: BTreeMap::new() }
    }

    pub fn load() -> Self {
        let mut h = History::empty();
        let Ok(text) = std::fs::read_to_string(data_path()) else { return h };
        let cutoff = now() - MAX_AGE_SEC;
        for line in text.lines() {
            let mut p = line.splitn(5, '\t');
            let (Some(ts), Some(key), Some(pct), Some(chg), Some(name)) = (p.next(), p.next(), p.next(), p.next(), p.next()) else {
                continue;
            };
            let (Ok(ts), Ok(pct), Ok(chg)) = (ts.parse::<i64>(), pct.parse::<i32>(), chg.parse::<u8>()) else { continue };
            if ts < cutoff || pct < -1 || pct > 100 || key.is_empty() {
                continue;
            }
            h.samples.push(Sample {
                ts,
                key: key.to_string(),
                name: if name.is_empty() { key.to_string() } else { name.to_string() },
                percent: pct,
                charging: chg != 0,
            });
        }
        h.samples.sort_by_key(|s| s.ts);
        for s in &h.samples {
            h.last.insert(s.key.clone(), s.ts);
            h.last_percent.insert(s.key.clone(), s.percent);
            h.last_charging.insert(s.key.clone(), s.charging);
        }
        h
    }

    pub fn record(&mut self, devices: &[DeviceStatus], ts: i64) {
        let mut changed = false;
        for d in devices {
            if !d.connected || d.asleep || d.percent > 100 || d.key.is_empty() {
                continue;
            }
            // A wired device may only say "on power" without a percentage. Keep that as a
            // status-only sample, so the dashboard shows "charging" instead of an old level.
            let known = d.percent >= 0;
            if !known && !d.charging {
                continue;
            }
            let last = self.last.get(&d.key).copied();
            let charge_changed = last.is_some() && self.last_charging.get(&d.key).copied() != Some(d.charging);
            let percent_changed = last.is_some() && self.last_percent.get(&d.key).copied() != Some(d.percent);
            let due = match last {
                None => true,
                Some(t) => {
                    (!known && (percent_changed || charge_changed))
                        || ts - t >= SAMPLE_EVERY_SEC
                        || ((percent_changed || charge_changed)
                            && ts - t >= if charge_changed { CHARGE_SAMPLE_EVERY_SEC } else { CHANGE_SAMPLE_EVERY_SEC })
                }
            };
            if !due {
                continue;
            }
            self.samples.push(Sample {
                ts,
                key: d.key.clone(),
                name: clean(&d.name),
                percent: d.percent,
                charging: d.charging,
            });
            self.last.insert(d.key.clone(), ts);
            self.last_percent.insert(d.key.clone(), d.percent);
            self.last_charging.insert(d.key.clone(), d.charging);
            changed = true;
        }
        if changed {
            let cutoff = ts - MAX_AGE_SEC;
            self.samples.retain(|s| s.ts >= cutoff);
            self.save();
        }
    }

    fn save(&self) {
        let path = data_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut out = String::with_capacity(self.samples.len() * 48);
        for s in &self.samples {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                s.ts,
                clean(&s.key),
                s.percent,
                s.charging as u8,
                clean(&s.name)
            ));
        }
        let _ = std::fs::write(path, out);
    }

    /// Last numeric level for a device (status-only samples carry -1).
    pub fn last_percent(&self, key: &str) -> Option<i32> {
        self.samples.iter().rev().find(|s| s.key == key && s.percent >= 0).map(|s| s.percent)
    }

    /// Estimated hours until empty from the last 7 days of discharging samples. None when there is
    /// not enough data or the level is rising (charging / just unplugged).
    pub fn eta_hours(&self, key: &str) -> Option<f64> {
        let cutoff = now() - 7 * 24 * 3600;
        let pts: Vec<&Sample> = self
            .samples
            .iter()
            .filter(|s| s.key == key && s.ts >= cutoff && !s.charging && s.percent >= 0)
            .collect();
        let (first, last) = (pts.first()?, pts.last()?);
        let hours = (last.ts - first.ts) as f64 / 3600.0;
        let drop = (first.percent - last.percent) as f64;
        if hours < 1.0 || drop < 2.0 {
            return None;
        }
        let rate = drop / hours;
        if rate <= 0.05 {
            return None;
        }
        Some((last.percent as f64 / rate).clamp(0.1, 999.0))
    }

    fn devices_json(&self) -> String {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for s in &self.samples {
            if seen.insert(s.key.clone()) {
                out.push(format!(r#"{{"k":"{}","n":"{}"}}"#, json(&s.key), json(&s.name)));
            }
        }
        format!("[{}]", out.join(","))
    }

    fn samples_json(&self) -> String {
        let out: Vec<String> = self
            .samples
            .iter()
            .map(|s| {
                format!(
                    r#"{{"t":{},"k":"{}","p":{},"c":{}}}"#,
                    s.ts,
                    json(&s.key),
                    s.percent,
                    s.charging
                )
            })
            .collect();
        format!("[{}]", out.join(","))
    }

    pub fn report_html(&self) -> String {
        let t = i18n::history();
        let labels = format!(
            r#"{{"title":"{}","device":"{}","range":"{}","r24":"{}","r7":"{}","r30":"{}","latest":"{}","min":"{}","max":"{}","change":"{}","samples":"{}","rate":"{}","eta":"{}","no_data":"{}","charging":"{}","on_battery":"{}","chart_title":"{}","chart_sub":"{}","insufficient":"{}","generated":"{}","theme":"{}","light":"{}","dark":"{}"}}"#,
            json(t.title),
            json(t.device),
            json(t.range),
            json(t.h24),
            json(t.d7),
            json(t.d30),
            json(t.latest),
            json(t.minimum),
            json(t.maximum),
            json(t.change),
            json(t.samples),
            json(t.rate),
            json(t.eta),
            json(t.no_data),
            json(t.charging),
            json(t.on_battery),
            json(t.chart_title),
            json(t.chart_sub),
            json(t.insufficient),
            json(t.generated),
            json(t.theme),
            json(t.light),
            json(t.dark),
        );
        TEMPLATE
            .replace("__DATA__", &self.samples_json())
            .replace("__DEVICES__", &self.devices_json())
            .replace("__LABELS__", &labels)
            .replace("__LANG__", i18n::current().code())
            .replace("__PAGE_TITLE__", &html(t.title))
    }
}

pub fn open_report() -> bool {
    let h = History::load();
    let path = data_path().with_file_name("battery_history.html");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&path, h.report_html()).is_err() {
        return false;
    }
    let target = HSTRING::from(path.to_string_lossy().as_ref());
    unsafe {
        let r = ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL);
        r.0 as isize > 32
    }
}

/// Live dashboard: a loopback-only HTTP server keeps the already open page updated every 2 s.
pub fn open_live() -> bool {
    let Some(port) = live_server() else { return open_report() };
    let url = format!("http://127.0.0.1:{port}/");
    unsafe {
        let r = ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
        r.0 as isize > 32
    }
}

static LIVE_PORT: OnceLock<u16> = OnceLock::new();

fn live_server() -> Option<u16> {
    if let Some(port) = LIVE_PORT.get() {
        return Some(*port);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(s) => s,
                Err(_) => continue,
            });
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let path = first.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/");
            let h = History::load();
            let (ctype, body) = if path == "/data" {
                (
                    "application/json; charset=utf-8",
                    format!(
                        r#"{{"now":{},"devices":{},"samples":{}}}"#,
                        now(),
                        h.devices_json(),
                        h.samples_json()
                    ),
                )
            } else if path == "/favicon.ico" {
                ("image/x-icon", String::new())
            } else {
                ("text/html; charset=utf-8", h.report_html())
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store, max-age=0\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
    });
    let _ = LIVE_PORT.set(port);
    Some(port)
}

fn html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\n' | '\r' | '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eta_from_discharge_curve() {
        let t = now();
        let mut h = History::empty();
        for (dt, p) in [(7200, 80), (3600, 70), (0, 60)] {
            h.samples.push(Sample {
                ts: t - dt,
                key: "k".into(),
                name: "Mouse".into(),
                percent: p,
                charging: false,
            });
        }
        let eta = h.eta_hours("k").expect("eta");
        assert!((eta - 6.0).abs() < 0.2, "eta={eta}");
    }

    #[test]
    fn html_has_data_and_no_placeholders() {
        let mut h = History::empty();
        h.samples.push(Sample { ts: 1_700_000_000, key: "k".into(), name: "Mouse".into(), percent: 55, charging: false });
        let html = h.report_html();
        assert!(!html.contains("__"));
        assert!(html.contains(r#""p":55"#));
        assert!(html.contains("Mouse"));
        assert!(html.contains("background-repeat:no-repeat"));
        assert!(html.contains(r#"id="theme_toggle""#));
        assert!(html.contains(r#"id="pct_icon""#));
    }

    #[test]
    fn status_only_charging_sample_never_becomes_a_level() {
        let mut h = History::empty();
        h.samples.push(Sample { ts: now(), key: "k".into(), name: "Mouse".into(), percent: -1, charging: true });
        assert_eq!(h.last_percent("k"), None);
        assert_eq!(h.eta_hours("k"), None);
        assert!(h.samples_json().contains(r#""p":-1"#));
        assert!(h.samples_json().contains(r#""c":true"#));
    }
}

const TEMPLATE: &str = r#"<!doctype html>
<html lang="__LANG__"><head><meta charset="utf-8">
<script>try{const m=document.cookie.match(/(?:^|; )bh-theme=(light|dark)/);const t=m?m[1]:(localStorage.getItem('bh-theme')||'');if(t==='light'||t==='dark')document.documentElement.dataset.theme=t}catch(e){}</script>
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>__PAGE_TITLE__</title>
<style>
:root{color-scheme:dark;--bg:#0d1016;--panel:#151a22;--line:#272d38;--text:#e9edf5;--muted:#8d98aa;--blue:#4da3ff;--green:#37d383;--red:#ff6262;
--glow:#1c2b44;--panel-hi:#1a2029;--panel-lo:#12161d;--panel2-hi:#171c24;--panel2-lo:#13171e;--line-strong:#2a3a52;--input:#11161e;--input-border:#2a3342;--active:#223a5c;--active-text:#e2f0ff;
--logo-hi:#22334d;--logo-lo:#11151c;--shadow:0 10px 30px #0008;--panel-shadow:0 24px 70px #0006;--gauge-hole:#10141b;--gauge-track:#232a35;--gauge-glow:#4da3ff22;
--chip-bg:#1d2c42;--chip-border:#2e4564;--chip-text:#dbeaff;--chip-green-bg:#163326;--chip-green-border:#25553c;--chip-green-text:#d9ffe9;
--live:#9fe7c0;--toggle-bg:#11161e;--toggle-active:#283654;--toggle-icon:#e9edf5;--empty-overlay:#0d1016cc}
:root[data-theme=light]{color-scheme:light;--bg:#f7f8f8;--panel:#ffffff;--line:#e2e4e6;--text:#2f302c;--muted:#6f7379;--blue:#4f86c6;--green:#6b9d11;--red:#d0021b;
--glow:#e7eef8;--panel-hi:#ffffff;--panel-lo:#f7f8f8;--panel2-hi:#ffffff;--panel2-lo:#f9fafb;--line-strong:#d2d2d2;--input:#f2f3f4;--input-border:#dcdedf;--active:#dbe6f5;--active-text:#2c4a6b;
--logo-hi:#f2f6fb;--logo-lo:#e1e8f1;--shadow:0 10px 30px #2f3b4a14;--panel-shadow:0 24px 70px #2f3b4a0f;--gauge-hole:#ffffff;--gauge-track:#e5e5e5;--gauge-glow:#4f86c622;
--chip-bg:#eef4fc;--chip-border:#cfdcf0;--chip-text:#33506f;--chip-green-bg:#edf4e0;--chip-green-border:#c9dfa3;--chip-green-text:#4f7410;
--live:#4f7410;--toggle-bg:#eef1f4;--toggle-active:#dbe6f5;--toggle-icon:#2f4f73;--empty-overlay:#f7f8f8cc}
*{box-sizing:border-box}
body{margin:0;background-color:var(--bg);background-image:radial-gradient(1100px 600px at 15% -15%,var(--glow) 0,var(--bg) 58%);background-repeat:no-repeat;background-attachment:fixed;color:var(--text);font:15px/1.45 "Segoe UI",system-ui,-apple-system,sans-serif}
.wrap{max-width:1180px;margin:0 auto;padding:28px 24px 42px}
header{display:flex;align-items:center;justify-content:space-between;gap:16px;margin-bottom:18px}
.brand{display:flex;align-items:center;gap:14px}
.logo{width:54px;height:54px;border-radius:15px;display:grid;place-items:center;font-size:27px;background:linear-gradient(145deg,var(--logo-hi),var(--logo-lo));box-shadow:var(--shadow);border:1px solid var(--line-strong)}
h1{font-size:25px;margin:0;letter-spacing:.2px}
.sub{color:var(--muted);font-size:13px;margin-top:2px}
.meta{color:var(--muted);font-size:12px;text-align:right;display:flex;align-items:center;gap:10px;justify-content:flex-end}
.theme{display:inline-flex;align-items:center;gap:2px;padding:3px;border-radius:999px;background:var(--toggle-bg);border:1px solid var(--line)}
.theme button{width:30px;height:30px;padding:0;border:0;border-radius:50%;display:grid;place-items:center;background:transparent;color:var(--muted);cursor:pointer;transition:background-color .18s,color .18s}
.theme button:hover{color:var(--text)}
.theme button.active{background:var(--toggle-active);color:var(--toggle-icon);box-shadow:0 1px 4px #0002}
.theme svg{width:17px;height:17px;display:block}
.live{display:inline-flex;align-items:center;gap:6px;color:var(--live);font-weight:600;font-size:11px;letter-spacing:.08em;text-transform:uppercase}
.live::before{content:"";width:7px;height:7px;border-radius:50%;background:var(--green);box-shadow:0 0 10px var(--green)}
.hero{display:grid;grid-template-columns:230px 1fr;gap:14px;margin-bottom:14px}
.current{background:linear-gradient(180deg,var(--panel-hi),var(--panel-lo));border:1px solid var(--line);border-radius:18px;padding:16px 18px;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px}
.eyebrow{font-size:11px;letter-spacing:.12em;text-transform:uppercase;color:var(--muted);margin-bottom:-2px}
.gauge{--pct:0;width:150px;height:150px;border-radius:50%;background:conic-gradient(var(--blue) calc(var(--pct)*1%),var(--gauge-track) 0);display:grid;place-items:center;position:relative;box-shadow:0 0 34px var(--gauge-glow)}
.gauge::before{content:"";position:absolute;inset:10px;border-radius:50%;background:var(--gauge-hole);border:1px solid var(--line)}
.gauge .val{position:relative;display:flex;align-items:baseline;gap:2px;font-variant-numeric:tabular-nums}
.gauge b{font-size:42px;line-height:1;letter-spacing:-1px}
.gauge span{font-size:17px;color:var(--muted)}
.pct-icon{display:grid;place-items:center;color:var(--green);align-self:center}
.pct-icon[hidden]{display:none}
.pct-icon svg{width:70px;height:70px;filter:drop-shadow(0 0 10px var(--gauge-glow))}
.chips{display:flex;gap:6px;flex-wrap:wrap;justify-content:center}
.chip{font-size:12px;color:var(--chip-text);background:var(--chip-bg);border:1px solid var(--chip-border);padding:4px 9px;border-radius:999px}
.chip.ghost{color:var(--muted);background:var(--panel2-hi);border-color:var(--line)}
.chip.green{color:var(--chip-green-text);background:var(--chip-green-bg);border-color:var(--chip-green-border)}
.metrics{display:grid;grid-template-columns:repeat(3,1fr);gap:10px}
.metric{background:linear-gradient(180deg,var(--panel2-hi),var(--panel2-lo));border:1px solid var(--line);border-radius:14px;padding:13px 14px}
.metric b{display:block;font-size:22px;line-height:1.15;margin-bottom:3px;font-variant-numeric:tabular-nums}
.metric span{color:var(--muted);font-size:12px}
.panel{background:linear-gradient(180deg,var(--panel2-hi),var(--panel2-lo));border:1px solid var(--line);border-radius:18px;padding:16px 18px 12px;box-shadow:var(--panel-shadow)}
.panel-head{display:flex;align-items:flex-start;justify-content:space-between;gap:16px;margin-bottom:6px}
.panel-title{font-size:15px;font-weight:600}
.panel-sub{color:var(--muted);font-size:12px;margin-top:2px}
.toolbar{display:flex;align-items:center;gap:10px;flex-wrap:wrap;justify-content:flex-end}
select{background:var(--input);border:1px solid var(--input-border);color:var(--text);padding:9px 11px;border-radius:10px;font:inherit;min-width:190px;max-width:260px}
.range{display:flex;gap:4px;background:var(--input);border:1px solid var(--input-border);padding:4px;border-radius:11px}
.range button{background:transparent;border:0;color:var(--muted);padding:7px 13px;border-radius:8px;font:inherit;cursor:pointer;white-space:nowrap}
.range button:hover{color:var(--text)}
.range button.active{background:var(--active);color:var(--active-text)}
.chart-wrap{position:relative;margin-top:6px}
canvas{width:100%;height:auto;display:block;border-radius:12px}
.empty{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:8px;color:var(--muted);text-align:center;pointer-events:none;background:radial-gradient(closest-side,var(--empty-overlay),transparent)}
.empty[hidden]{display:none}
.empty .ico{font-size:30px;opacity:.65}
.legend{display:flex;gap:18px;align-items:center;padding:10px 4px 2px;color:var(--muted);font-size:12px}
.legend .dot{display:inline-block;width:9px;height:9px;border-radius:50%;background:var(--blue);margin-right:6px;vertical-align:1px}
.legend .dot.green{background:var(--green)}
footer{display:flex;justify-content:space-between;gap:12px;color:var(--muted);font-size:12px;margin-top:14px;padding:0 4px}
@media(max-width:900px){.hero{grid-template-columns:1fr}.gauge{width:128px;height:128px}.gauge b{font-size:36px}.metrics{grid-template-columns:repeat(3,1fr)}.panel-head{align-items:stretch;flex-direction:column}.toolbar{justify-content:flex-start}}
@media(max-width:520px){.wrap{padding:18px 14px 30px}.metrics{grid-template-columns:repeat(2,1fr)}header{align-items:flex-start}.meta #generated{display:none}h1{font-size:21px}.range button{padding:7px 10px}}
</style></head>
<body><div class="wrap">
<header>
  <div class="brand"><div class="logo">🔋</div><div><h1>BatteryHub</h1><div class="sub" id="sub"></div></div></div>
  <div class="meta">
    <div class="theme" id="theme_toggle" role="group" aria-label="Theme">
      <button type="button" data-mode="light" aria-pressed="false"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M19.1 4.9l-1.4 1.4M6.3 17.7l-1.4 1.4"/></svg></button>
      <button type="button" data-mode="dark" aria-pressed="false"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z"/></svg></button>
    </div>
    <span class="live" id="live">—</span><span id="generated"></span>
  </div>
</header>
<section class="hero">
  <div class="current">
    <div class="eyebrow" id="current_label"></div>
    <div class="gauge" id="gauge"><div class="val"><span class="pct-icon" id="pct_icon" role="img" hidden><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round" aria-hidden="true"><rect x="2.5" y="7" width="19" height="10" rx="5"/><rect x="7.5" y="11" width="9" height="2" rx="1" fill="currentColor" stroke="none"/></svg></span><b id="latest">—</b><span id="pct_sign">%</span></div></div>
    <div class="chips"><span class="chip" id="status"></span><span class="chip ghost" id="points"></span></div>
  </div>
  <div class="metrics">
    <div class="metric"><b id="min">—</b><span id="l_min"></span></div>
    <div class="metric"><b id="max">—</b><span id="l_max"></span></div>
    <div class="metric"><b id="change">—</b><span id="l_change"></span></div>
    <div class="metric"><b id="rate">—</b><span id="l_rate"></span></div>
    <div class="metric"><b id="eta">—</b><span id="l_eta"></span></div>
    <div class="metric"><b id="count">—</b><span id="samples_label"></span></div>
  </div>
</section>
<section class="panel">
  <div class="panel-head">
    <div><div class="panel-title" id="chart_title"></div><div class="panel-sub" id="chart_sub"></div></div>
    <div class="toolbar"><select id="device"></select>
      <div class="range" id="ranges"><button data-r="86400" class="active" id="r24"></button><button data-r="604800" id="r7"></button><button data-r="2592000" id="r30"></button></div>
    </div>
  </div>
  <div class="chart-wrap"><canvas id="chart" width="1200" height="390"></canvas><div class="empty" id="empty" hidden><div class="ico">🔋</div><div id="empty_text"></div></div></div>
  <div class="legend"><span><i class="dot"></i><span id="legend_level"></span></span><span><i class="dot green"></i><span id="legend_charge"></span></span></div>
</section>
<footer><span id="foot_left"></span><span id="foot_right"></span></footer>
</div>
<script>
let DATA=__DATA__, DEVICES=__DEVICES__; const L=__LABELS__;
const $=id=>document.getElementById(id), c=$('chart'), ctx=c.getContext('2d');
let device=DEVICES.length?DEVICES[0].k:'', range=86400, hover=null, geom=null, pts=[], cur=null;
const THEMES={
  dark:{bg:'#12161d',grid:'#252c38',grid2:'#1c222c',text:'#8d98aa',line:'#4da3ff',green:'#37d383',tip:'#0b0f15',tipText:'#eaf3ff',tipBorder:'#33475f',cross:'#55708f',fillTop:'rgba(77,163,255,.34)',fillBottom:'rgba(77,163,255,0)'},
  light:{bg:'#ffffff',grid:'#e2e4e6',grid2:'#eef0f2',text:'#6f7379',line:'#4f86c6',green:'#6b9d11',tip:'#ffffff',tipText:'#2f302c',tipBorder:'#c9ced4',cross:'#9aa3ad',fillTop:'rgba(79,134,198,.20)',fillBottom:'rgba(79,134,198,0)'}
};
let C=THEMES.dark;
const pad=n=>String(n).padStart(2,'0');
function fmt(ts,full){const d=new Date(ts*1000);return full?pad(d.getDate())+'.'+pad(d.getMonth()+1)+' '+pad(d.getHours())+':'+pad(d.getMinutes()):(range<=86400?pad(d.getHours())+':'+pad(d.getMinutes()):pad(d.getDate())+'.'+pad(d.getMonth()+1));}
function applyTheme(mode,persist){
  mode=mode==='light'?'light':'dark';
  document.documentElement.dataset.theme=mode;
  C=THEMES[mode];
  document.querySelectorAll('#theme_toggle button').forEach(b=>{
    const on=b.dataset.mode===mode;
    b.classList.toggle('active',on);
    b.setAttribute('aria-pressed',on?'true':'false');
  });
  if(persist){try{localStorage.setItem('bh-theme',mode)}catch(e){}try{document.cookie='bh-theme='+mode+'; path=/; max-age=31536000; SameSite=Lax'}catch(e){}}
  draw();
}
function setup(){
  $('sub').textContent=L.title; $('current_label').textContent=L.latest; $('r24').textContent=L.r24; $('r7').textContent=L.r7; $('r30').textContent=L.r30;
  $('l_min').textContent=L.min; $('l_max').textContent=L.max; $('l_change').textContent=L.change;
  $('l_rate').textContent=L.rate; $('l_eta').textContent=L.eta; $('samples_label').textContent=L.samples;
  $('chart_title').textContent=L.chart_title; $('chart_sub').textContent=L.chart_sub; $('legend_level').textContent=L.chart_title; $('legend_charge').textContent=L.charging;
  $('generated').textContent=L.generated+' '+new Date().toLocaleString();
  $('theme_toggle').setAttribute('aria-label',L.theme);
  document.querySelectorAll('#theme_toggle button').forEach(b=>{
    const label=b.dataset.mode==='light'?L.light:L.dark;
    b.title=label; b.setAttribute('aria-label',label);
    b.onclick=()=>applyTheme(b.dataset.mode,true);
  });
  const sel=$('device'); for(const d of DEVICES){const o=document.createElement('option');o.value=d.k;o.textContent=d.n;sel.appendChild(o);}
  sel.onchange=()=>{device=sel.value;hover=null;draw();};
  document.querySelectorAll('#ranges button').forEach(b=>b.onclick=()=>{range=+b.dataset.r;document.querySelectorAll('#ranges button').forEach(x=>x.classList.toggle('active',x===b));hover=null;draw();});
  c.addEventListener('mousemove',e=>{if(!geom){return}const r=c.getBoundingClientRect(),x=(e.clientX-r.left)*c.width/r.width;let best=-1,bd=28;pts.forEach((p,i)=>{const d=Math.abs(geom.X(p.t)-x);if(d<bd){bd=d;best=i}});if(best!==hover){hover=best>=0?best:null;draw();}});
  c.addEventListener('mouseleave',()=>{if(hover!==null){hover=null;draw();}});
  applyTheme(document.documentElement.dataset.theme==='light'?'light':'dark',false);
  poll();
  setInterval(poll,2000);
}

function updateDevices(list){
  if(JSON.stringify(list)===JSON.stringify(DEVICES)) return;
  DEVICES=list;
  const sel=$('device'), old=device;
  sel.innerHTML='';
  for(const d of DEVICES){const o=document.createElement('option');o.value=d.k;o.textContent=d.n;sel.appendChild(o);}
  if(!DEVICES.some(d=>d.k===old)) device=DEVICES.length?DEVICES[0].k:'';
  sel.value=device;
}

async function poll(){
  try{
    const r=await fetch('/data',{cache:'no-store'});
    if(!r.ok) throw new Error('http');
    const j=await r.json();
    DATA=j.samples||[];
    updateDevices(j.devices||[]);
    draw();
    $('live').textContent='LIVE · '+new Date().toLocaleTimeString();
  }catch(e){
    $('live').textContent='offline';
  }
}
function stats(){
  const last=cur||(pts.length?pts[pts.length-1]:null);
  if(!last)return null;
  const dis=pts.filter(p=>!p.c);
  let rate=null;
  if(dis.length>1){const a=dis[0],b=dis[dis.length-1],h=(b.t-a.t)/3600;if(h>=.5&&a.p>b.p)rate=(a.p-b.p)/h;}
  return {latest:last.p>=0?last.p:null,numeric:pts.length>0,min:pts.length?Math.min(...pts.map(p=>p.p)):null,max:pts.length?Math.max(...pts.map(p=>p.p)):null,change:pts.length>1?pts[pts.length-1].p-pts[0].p:null,rate,eta:rate&&!last.c&&last.p>0?last.p/rate:null,n:pts.length,charging:last.c};
}
function draw(){
  const all=DATA.filter(s=>s.k===device&&s.t>=Date.now()/1000-range).sort((a,b)=>a.t-b.t);
  cur=all.length?all[all.length-1]:null;
  pts=all.filter(s=>s.p>=0);
  const W=c.width,H=c.height,l=58,r=24,t=24,b=44,pw=W-l-r,ph=H-t-b,now=Math.floor(Date.now()/1000),start=now-range;
  ctx.clearRect(0,0,W,H); ctx.fillStyle=C.bg; ctx.fillRect(0,0,W,H);
  ctx.font='13px "Segoe UI",sans-serif'; ctx.lineWidth=1;
  for(let i=0;i<=4;i++){const y=t+ph*i/4;ctx.strokeStyle=C.grid;ctx.beginPath();ctx.moveTo(l,y);ctx.lineTo(W-r,y);ctx.stroke();ctx.fillStyle=C.text;ctx.textAlign='right';ctx.textBaseline='middle';ctx.fillText(String(100-i*25),l-10,y);}
  ctx.textAlign='center';ctx.textBaseline='top';
  for(let i=0;i<=4;i++){const ts=start+range*i/4,x=l+pw*i/4;ctx.strokeStyle=C.grid2;ctx.beginPath();ctx.moveTo(x,t);ctx.lineTo(x,t+ph);ctx.stroke();ctx.fillStyle=C.text;ctx.fillText(fmt(ts),x,H-b+13);}
  const X=ts=>l+Math.max(0,Math.min(1,(ts-start)/range))*pw, Y=p=>t+ph*(1-p/100); geom={X,Y};
  if(pts.length){
    if(pts.length>1){
      const g=ctx.createLinearGradient(0,t,0,t+ph);g.addColorStop(0,C.fillTop);g.addColorStop(1,C.fillBottom);
      ctx.beginPath();ctx.moveTo(X(pts[0].t),Y(pts[0].p));for(const p of pts)ctx.lineTo(X(p.t),Y(p.p));ctx.lineTo(X(pts[pts.length-1].t),t+ph);ctx.lineTo(X(pts[0].t),t+ph);ctx.closePath();ctx.fillStyle=g;ctx.fill();
      ctx.beginPath();ctx.moveTo(X(pts[0].t),Y(pts[0].p));for(const p of pts)ctx.lineTo(X(p.t),Y(p.p));ctx.strokeStyle=C.line;ctx.lineWidth=2.6;ctx.lineJoin='round';ctx.stroke();
    }
    for(const p of pts){if(p.c){ctx.beginPath();ctx.arc(X(p.t),Y(p.p),3.2,0,Math.PI*2);ctx.fillStyle=C.green;ctx.fill();}}
    const last=pts[pts.length-1];ctx.beginPath();ctx.arc(X(last.t),Y(last.p),5,0,Math.PI*2);ctx.fillStyle=last.c?C.green:C.line;ctx.fill();ctx.strokeStyle=C.bg;ctx.lineWidth=2;ctx.stroke();
    if(hover!==null&&pts[hover]){
      const p=pts[hover],px=X(p.t),py=Y(p.p);
      ctx.save();ctx.setLineDash([4,5]);ctx.strokeStyle=C.cross;ctx.beginPath();ctx.moveTo(px,t);ctx.lineTo(px,t+ph);ctx.stroke();ctx.restore();
      ctx.beginPath();ctx.arc(px,py,5.5,0,Math.PI*2);ctx.fillStyle=p.c?C.green:C.line;ctx.fill();ctx.strokeStyle=C.tipText;ctx.lineWidth=2;ctx.stroke();
      const label=fmt(p.t,true)+'  ·  '+p.p+'%'+(p.c?' ⚡':'');
      ctx.font='600 13px "Segoe UI",sans-serif';const tw=ctx.measureText(label).width+22;let bx=px-tw/2;bx=Math.max(l,Math.min(W-r-tw,bx));const by=Math.max(t+4,py-42);
      ctx.fillStyle=C.tip;ctx.strokeStyle=C.tipBorder;ctx.lineWidth=1;ctx.beginPath();ctx.roundRect(bx,by,tw,28,8);ctx.fill();ctx.stroke();
      ctx.fillStyle=C.tipText;ctx.textAlign='left';ctx.textBaseline='middle';ctx.fillText(label,bx+11,by+14.5);
    }
  }
  const s=stats();
  $('empty').hidden=!!all.length; $('empty_text').textContent=all.length?'':L.no_data;
  $('chart_sub').textContent=pts.length<2?L.insufficient:L.chart_sub;
  const hasLevel=s&&s.latest!==null, cable=!!(s&&s.latest===null&&s.charging);
  $('gauge').style.setProperty('--pct',hasLevel?s.latest:0);
  $('latest').textContent=hasLevel?s.latest:'—';
  $('latest').hidden=cable;
  $('pct_icon').hidden=!cable;
  $('pct_icon').title=cable?L.charging:'';
  $('pct_icon').setAttribute('aria-label',cable?L.charging:'');
  $('pct_sign').hidden=!hasLevel;
  $('status').textContent=s?(s.charging?L.charging:L.on_battery):'';
  $('status').className='chip'+(s&&s.charging?' green':'');
  $('points').textContent=s?s.n+' '+L.samples:'';
  $('min').textContent=s&&s.min!==null?s.min+'%':'—'; $('max').textContent=s&&s.max!==null?s.max+'%':'—';
  $('change').textContent=s&&s.change!==null?(s.change>0?'+':'')+s.change+'%':'—';
  $('rate').textContent=s&&s.rate?s.rate.toFixed(1)+' %/h':'—';
  $('eta').textContent=s&&s.eta!==null?Math.round(s.eta)+' h':'—';
  $('count').textContent=s?s.n:'—';
  $('foot_left').textContent=s?s.n+' '+L.samples:'—';
  $('foot_right').textContent=L.generated+' '+new Date().toLocaleString();
}
setup();
</script></body></html>"#;
