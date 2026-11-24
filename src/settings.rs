//! Per-device low-battery warning levels. Each edition keeps its own list under its settings key
//! (HKCU\Software\BatteryHub\<Edition>\Thresholds, one DWORD per device key), so the two editions
//! never share or overwrite each other's choices.
use std::collections::BTreeMap;

use windows::Win32::UI::WindowsAndMessaging::{AppendMenuW, CreatePopupMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED};

use crate::device::DeviceStatus;
use crate::i18n;
use crate::install::{self as inst, HKCU};

/// The levels offered in the menus.
pub const THRESHOLDS: [i32; 7] = [50, 40, 30, 20, 15, 10, 5];
pub const DEFAULT_THRESHOLD: i32 = 15;

fn key(settings_key: &str) -> String {
    format!(r"{settings_key}\Thresholds")
}

/// Warning level for one device (the percentage at or below which it turns red).
pub fn threshold(settings_key: &str, device_key: &str) -> i32 {
    inst::open(HKCU, &key(settings_key), false)
        .and_then(|k| k.get(device_key))
        .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .map(|v| v as i32)
        .filter(|v| THRESHOLDS.contains(v))
        .unwrap_or(DEFAULT_THRESHOLD)
}

pub fn set_threshold(settings_key: &str, device_key: &str, value: i32) -> bool {
    inst::create(HKCU, &key(settings_key)).is_some_and(|k| k.set_dword(device_key, value as u32))
}

// ---- per-device icons -------------------------------------------------------------------------

pub fn load_icons(settings_key: &str) -> BTreeMap<String, String> {
    inst::open(HKCU, settings_key, false)
        .and_then(|k| k.get("DeviceIcons"))
        .map(|text| {
            text.lines()
                .filter_map(|l| {
                    let (k, v) = l.split_once('\t')?;
                    (!k.is_empty() && !v.is_empty()).then(|| (k.to_string(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_icons(settings_key: &str, icons: &BTreeMap<String, String>) -> bool {
    let text: String = icons.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
    inst::create(HKCU, settings_key).is_some_and(|k| k.set_string("DeviceIcons", text.trim_end()))
}

pub fn load_icon_sizes(settings_key: &str) -> BTreeMap<String, i32> {
    inst::open(HKCU, settings_key, false)
        .and_then(|k| k.get("DeviceIconSizes"))
        .map(|text| {
            text.lines()
                .filter_map(|l| {
                    let (k, v) = l.split_once('\t')?;
                    let size = v.parse::<i32>().ok()?;
                    (!k.is_empty() && (12..=32).contains(&size)).then(|| (k.to_string(), size))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_icon_sizes(settings_key: &str, sizes: &BTreeMap<String, i32>) -> bool {
    let text: String = sizes.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
    inst::create(HKCU, settings_key).is_some_and(|k| k.set_string("DeviceIconSizes", text.trim_end()))
}

/// Low = discharging, a known level, at or below the device's warning level.
pub fn is_low(s: &DeviceStatus, threshold: i32) -> bool {
    !s.charging && !s.is_full() && s.connected && s.percent >= 0 && s.percent <= threshold
}

// ---- menus -------------------------------------------------------------------------------------

/// Menu ids: BASE + device index * STRIDE + index into THRESHOLDS.
pub const MENU_BASE: usize = 1000;
const STRIDE: usize = 16;
pub const ICON_MENU_BASE: usize = 2000;
const ICON_STRIDE: usize = 16;
const ICON_SIZES: [i32; 6] = [12, 16, 20, 24, 28, 32];

/// Appends one submenu per device: "Name - 32%" > "Red warning at or below:" > 50% ... 5%.
pub fn append_device_menus(menu: HMENU, devices: &[DeviceStatus], settings_key: &str) {
    let icon_sizes = load_icon_sizes(settings_key);
    unsafe {
        for (i, d) in devices.iter().enumerate() {
            let Ok(sub) = CreatePopupMenu() else { continue };
            let current = threshold(settings_key, &d.key);
            let _ = AppendMenuW(sub, MF_STRING | MF_GRAYED, 0, &inst_str(&format!("{} - {}", d.name, d.charge_text())));
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, windows::core::PCWSTR::null());
            let _ = AppendMenuW(sub, MF_STRING | MF_GRAYED, 0, &inst_str(i18n::t().red_warning));
            for (j, t) in THRESHOLDS.iter().enumerate() {
                let check = if *t == current { MF_CHECKED } else { MF_UNCHECKED };
                let def = if *t == DEFAULT_THRESHOLD { i18n::t().default_suffix } else { "" };
                let _ = AppendMenuW(sub, MF_STRING | check, MENU_BASE + i * STRIDE + j, &inst_str(&format!("{t}%{def}")));
            }
            if let Ok(icon_menu) = CreatePopupMenu() {
                let _ = AppendMenuW(sub, MF_SEPARATOR, 0, windows::core::PCWSTR::null());
                let _ = AppendMenuW(icon_menu, MF_STRING, ICON_MENU_BASE + i * ICON_STRIDE, &inst_str(i18n::t().icon_file));
                let _ = AppendMenuW(icon_menu, MF_STRING, ICON_MENU_BASE + i * ICON_STRIDE + 1, &inst_str(i18n::t().icon_auto));
                let _ = AppendMenuW(icon_menu, MF_STRING, ICON_MENU_BASE + i * ICON_STRIDE + 2, &inst_str(i18n::t().icon_reset));
                if let Ok(size_menu) = CreatePopupMenu() {
                    let current_size = icon_sizes.get(&d.key).copied().unwrap_or(16);
                    for (j, size) in ICON_SIZES.iter().enumerate() {
                        let check = if *size == current_size { MF_CHECKED } else { MF_UNCHECKED };
                        let _ = AppendMenuW(
                            size_menu,
                            MF_STRING | check,
                            ICON_MENU_BASE + i * ICON_STRIDE + 3 + j,
                            &inst_str(&format!("{size} px")),
                        );
                    }
                    let _ = AppendMenuW(icon_menu, MF_STRING | MF_POPUP, size_menu.0 as usize, &inst_str(i18n::t().icon_size));
                }
                let _ = AppendMenuW(sub, MF_STRING | MF_POPUP, icon_menu.0 as usize, &inst_str(i18n::t().icon));
            }
            let label = format!("{}  ({})", d.name, d.charge_text());
            let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &inst_str(&label));
        }
    }
}

/// Decodes a menu id from `append_device_menus` into (device index, threshold).
pub fn decode_menu_id(id: usize) -> Option<(usize, i32)> {
    let rel = id.checked_sub(MENU_BASE)?;
    let (dev, j) = (rel / STRIDE, rel % STRIDE);
    THRESHOLDS.get(j).map(|t| (dev, *t))
}

/// Decodes an icon menu id into (device index, action): 0 file, 1 auto, 2 reset.
pub fn decode_icon_menu_id(id: usize) -> Option<(usize, usize)> {
    let rel = id.checked_sub(ICON_MENU_BASE)?;
    let (dev, action) = (rel / ICON_STRIDE, rel % ICON_STRIDE);
    (action < 9).then_some((dev, action))
}

pub fn icon_size_for_action(action: usize) -> Option<i32> {
    action.checked_sub(3).and_then(|i| ICON_SIZES.get(i).copied())
}

fn inst_str(s: &str) -> windows::core::HSTRING {
    windows::core::HSTRING::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_round_trip() {
        for dev in 0..5 {
            for (j, t) in THRESHOLDS.iter().enumerate() {
                assert_eq!(decode_menu_id(MENU_BASE + dev * STRIDE + j), Some((dev, *t)));
            }
        }
        assert_eq!(decode_menu_id(3), None);
    }
}
