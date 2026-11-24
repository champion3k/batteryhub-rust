//! AJAZZ AJ-series mice on their SONiX 2.4 GHz receiver (VID 3151, e.g. AJ159 APEX 3151:5007).
//!
//! The receiver exposes a 65-byte feature collection on usage page FFFF / usage 0002. A
//! SET_FEATURE status poll (report 0x00, opcode 0xF7, zero payload) brings the 2.4 GHz telemetry
//! link up; about 30 ms later GET_FEATURE report 0x05 carries the charge in byte 3 (0..100). A
//! frame with a non-zero byte 1 or 2 is transient reconnect garbage and is rejected.
//! Protocol same as the original BatteryHub's AJ-series driver.
use std::thread;
use std::time::Duration;

use crate::device::{DeviceKind, DeviceStatus, PollContext};
use crate::hid::{self, HidDev};
use crate::util::hex4;

const VID: u16 = 0x3151;

fn read_percent(c: &hid::HidColl) -> Option<u8> {
    let d = HidDev::open_feature_only(c)?;
    let mut req = vec![0u8; c.feat_len() as usize];
    req[0] = 0x00; // unnumbered report
    req[1] = 0xF7; // status poll: brings the 2.4 GHz telemetry link up
    if !d.set_feature(&req) {
        return None;
    }
    thread::sleep(Duration::from_millis(30));
    let r = d.get_feature(0x05)?;
    if r.len() < 4 || r[0] != 0x05 || r[1] != 0 || r[2] != 0 || r[3] > 100 {
        return None;
    }
    Some(r[3])
}

pub fn poll(ctx: &mut PollContext) {
    for (vid, pid) in ctx.products(VID) {
        // Wired AJ159 APEX: the receiver disappears and the mouse enumerates as 3151:4026.
        // Its vendor collection does not expose a battery value, so report an unknown level
        // while on the cable instead of inventing one from history.
        if pid == 0x4026 {
            if !ctx.find(vid, pid, |c| c.page() == 0xFFFF && c.usage() == 0x0002 && c.feat_len() >= 64).is_empty() {
                let s = DeviceStatus {
                    key: "ajazz:3151:5007".into(),
                    name: "AJAZZ AJ159 APEX".into(),
                    kind: DeviceKind::Mouse,
                    via: "USB cable".into(),
                    percent: -1,
                    charging: true,
                    hw: Some((vid, pid)),
                    ..Default::default()
                };
                ctx.add(s, vid, pid);
                break;
            }
        }
        for c in ctx.find(vid, pid, |c| c.page() == 0xFFFF && c.usage() == 0x0002 && c.feat_len() >= 65) {
            let Some(pct) = read_percent(&c) else { continue };
            let name = if pid == 0x5007 {
                "AJAZZ AJ159 APEX".to_string()
            } else {
                let n = hid::product_string(&c);
                if n.is_empty() { "AJAZZ mouse".to_string() } else { n }
            };
            let s = DeviceStatus {
                key: format!("ajazz:{}:{}", hex4(vid), hex4(pid)),
                name,
                kind: DeviceKind::Mouse,
                via: "2.4 GHz receiver".into(),
                // 0 means the receiver is up but the mouse is off / not linked yet.
                connected: pct > 0,
                percent: if pct > 0 { pct as i32 } else { -1 },
                hw: Some((vid, pid)),
                ..Default::default()
            };
            ctx.add(s, vid, pid);
            break;
        }
    }
}
