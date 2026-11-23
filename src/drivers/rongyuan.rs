//! Rongyuan 2.4 GHz receivers used by Attack Shark, AJAZZ and some other mice / keyboards.
//!
//! One feature report returns both slots: request report 0 [00 F7 00 ...] on interface 2,
//! vendor collection FFFF:0002; reply has keyboard % in byte 2, mouse % in byte 3 and the
//! "offline" flags in bytes 4 / 5. Protocol from qu1ncky/BatteryTray and the ATTACK SHARK web
//! driver (see the C++ BatteryHub's rongyuan.cpp).
use std::thread;
use std::time::Duration;

use crate::device::{DeviceKind, DeviceStatus, PollContext};
use crate::hid::{self, HidDev};
use crate::util::hex4;

const RECEIVERS: [u32; 81] = [
    0x0DB0197E, 0x0DB08E9A, 0x145F0376, 0x145F038D, 0x14A5C045, 0x25AA2008, 0x25AA4008, 0x25AA4018, 0x31514011,
    0x31514014, 0x31514017, 0x31514021, 0x31514027, 0x3151402D, 0x31514038, 0x31515006, 0x31515007, 0x31515008,
    0x31515020, 0x31515026, 0x3151502B, 0x31515031, 0x31515037, 0x31515038, 0x3151503D, 0x3151503F, 0x31515040,
    0x31515042, 0x31515045, 0x3151504B, 0x3151504E, 0x31515050, 0x31515052, 0x31515055, 0x3151505A, 0x342DE3E0,
    0x342DE3E1, 0x342DE3E2, 0x342DE3E6, 0x342DE3E7, 0x342DE3E8, 0x342DE3F5, 0x342DE3F6, 0x342DE3F7, 0x342DE3FB,
    0x342DE3FC, 0x342DE3FD, 0x347AA302, 0x347AA305, 0x374AA205, 0x374AA217, 0x374AA220, 0x374AA223, 0x374AA226,
    0x374AA229, 0x374AA234, 0x374AA237, 0x374AA251, 0x374AA263, 0x374AA268, 0x374AA311, 0x374AA314, 0x374AA319,
    0x374AA322, 0x374AA329, 0x374AA355, 0x379A1804, 0x379A1807, 0x38A90002, 0x38EE0000, 0x38EE0002, 0x38EE0039,
    0x391D1A04, 0x391D1A08, 0x391D1A0E, 0x391D1A13, 0x391D1A25, 0x391D1A2B, 0x391D1A34, 0x391D2017, 0x391D2A05,
];

fn is_receiver(vid: u16, pid: u16) -> bool {
    let key = ((vid as u32) << 16) | pid as u32;
    RECEIVERS.contains(&key)
}

/// "AJAZZ 2.4G 8K" -> "AJAZZ"; "2.4G Wireless Receiver" -> "".
fn brand(product: &str) -> String {
    let mut out = String::new();
    for word in product.split_whitespace() {
        let upper = word.to_ascii_uppercase();
        if matches!(
            upper.as_str(),
            "2.4G" | "2.4GHZ" | "8K" | "4K" | "2K" | "1K" | "RECEIVER" | "WIRELESS" | "DONGLE" | "USB" | "KEYBOARD"
                | "MOUSE"
        ) {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

fn query_status(d: &HidDev) -> Option<Vec<u8>> {
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(100));
        }
        let mut cmd = vec![0u8; d.coll().feat_len() as usize];
        if cmd.len() < 7 {
            return None;
        }
        cmd[1] = 0xF7;
        if !d.set_feature(&cmd) {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
        if let Some(r) = d.get_feature(0) {
            if r.len() >= 7 && r[2..7].iter().any(|&b| b != 0) {
                return Some(r);
            }
        }
    }
    None
}

pub fn poll(ctx: &mut PollContext) {
    let mut products: Vec<(u16, u16)> = Vec::new();
    for c in &ctx.colls {
        // 3151:5007 is the AJAZZ AJ159 APEX receiver, handled by the AJAZZ driver.
        if is_receiver(c.vid, c.pid) && !ctx.is_claimed(c.vid, c.pid) && (c.vid, c.pid) != (0x3151, 0x5007) {
            let id = (c.vid, c.pid);
            if !products.contains(&id) {
                products.push(id);
            }
        }
    }

    for (vid, pid) in products {
        for c in ctx.find(vid, pid, |c| {
            c.iface == 2 && c.page() == 0xFFFF && c.usage() == 0x0002 && c.feat_len() >= 7
        }) {
            let Some(d) = HidDev::open_feature_only(&c) else { continue };
            let Some(r) = query_status(&d) else { continue };
            let product = hid::product_string(&c);
            let brand = brand(&product);
            let generic = product.is_empty()
                || product.to_ascii_lowercase().contains("receiver")
                || product.to_ascii_lowercase().contains("2.4g");

            for keyboard in [true, false] {
                let offline = r[if keyboard { 4 } else { 5 }];
                if offline != 0 {
                    continue; // nothing connected in this slot
                }
                let level = r[if keyboard { 2 } else { 3 }];
                let name = if vid == 0x3151
                    && pid == 0x402D
                    && !keyboard
                    && (brand.is_empty() || brand.to_ascii_uppercase().contains("ATTACK SHARK"))
                {
                    "Attack Shark R3".to_string()
                } else if generic && !brand.is_empty() {
                    format!("{brand} wireless {}", if keyboard { "keyboard" } else { "mouse" })
                } else if !product.is_empty() && !generic {
                    product.clone()
                } else if keyboard {
                    "Wireless keyboard".to_string()
                } else {
                    "Wireless mouse".to_string()
                };
                let s = DeviceStatus {
                    key: format!("rongyuan:{}:{}:{}", hex4(vid), hex4(pid), if keyboard { "kb" } else { "ms" }),
                    name,
                    kind: if keyboard { DeviceKind::Keyboard } else { DeviceKind::Mouse },
                    via: "2.4 GHz receiver".into(),
                    percent: if (1..=100).contains(&level) { level as i32 } else { -1 },
                    grace_sec: 1800,
                    hw: Some((vid, pid)),
                    ..Default::default()
                };
                ctx.add(s, vid, pid);
            }
            break;
        }
    }
}
