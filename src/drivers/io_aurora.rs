//! IO by Red Square Aurora mouse over its 2.4 GHz receiver or its USB cable.
//!
//! Both transports use the same CompX report-8 power query (see `super::compx`): interface 1,
//! collection FF02:0002, 17-byte frame `08 04 ... <checksum>`, level in byte 6, power state in byte
//! 7. The receiver identifies itself as `IO Aurora Receiver` on 3554:FB08; the same mouse on its
//! charging cable appears as `IO Aurora` on 3554:FB09. 3554 is shared with Lamzu, ATK/VXE and other
//! CompX devices, so both the id and the exact product string are required.
//!
//! The cable is tried first: it answers while charging, when the receiver has gone quiet. Cable and
//! receiver keep one icon key, so plugging the cable in switches the same tray entry to charging.
//! A mouse that idles stops answering; the last reading stays greyed while its receiver is present.
use crate::device::{DeviceKind, DeviceStatus, PollContext};
use crate::hid;

use super::compx;

const VID: u16 = 0x3554;
const RECEIVER_PID: u16 = 0xFB08;
const CABLE_PID: u16 = 0xFB09;
const KEY: &str = "io_aurora:3554fb08";
const ASLEEP_SEC: u32 = 300;

/// The product string each captured identity must carry (case-insensitive, trimmed).
fn product_matches(pid: u16, product: &str) -> bool {
    let p = product.trim().to_ascii_lowercase();
    match pid {
        RECEIVER_PID => p == "io aurora receiver",
        CABLE_PID => p == "io aurora",
        _ => false,
    }
}

fn status(pid: u16, reading: compx::Reading) -> DeviceStatus {
    DeviceStatus {
        key: KEY.into(),
        name: "IO Aurora".into(),
        via: if pid == CABLE_PID { "USB cable".into() } else { "2.4 GHz receiver".into() },
        kind: DeviceKind::Mouse,
        percent: reading.level as i32,
        charging: reading.charging(),
        full: reading.power == 2,
        millivolts: reading.millivolts as i32,
        grace_sec: ASLEEP_SEC,
        hw: Some((VID, pid)),
        ..Default::default()
    }
}

pub fn poll(ctx: &mut PollContext) {
    // The cable wins: it answers while charging and names the same mouse.
    for pid in [CABLE_PID, RECEIVER_PID] {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        let colls: Vec<hid::HidColl> = ctx
            .find(VID, pid, |c| compx::is_control(c) && product_matches(pid, &hid::product_string(c)));
        if colls.is_empty() {
            continue;
        }
        if let Some(reading) = colls.iter().find_map(compx::read) {
            // One mouse, two transports: keep the generic fallback off both.
            ctx.claim(VID, CABLE_PID);
            ctx.claim(VID, RECEIVER_PID);
            ctx.add(status(pid, reading), VID, pid);
            return;
        }
        // The mouse is present but asleep: keep the previous reading greyed, and keep the generic
        // fallback from claiming a collection this driver owns.
        ctx.claim(VID, pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_product_strings_only() {
        assert!(product_matches(RECEIVER_PID, " IO Aurora Receiver "));
        assert!(product_matches(CABLE_PID, "io aurora"));
        assert!(!product_matches(CABLE_PID, "IO Aurora Receiver"));
        assert!(!product_matches(RECEIVER_PID, "IO Aurora"));
        assert!(!product_matches(RECEIVER_PID, "USB Receiver"));
        assert!(!product_matches(0xF50D, "LAMZU"));
    }

    #[test]
    fn one_key_and_charging_on_the_cable() {
        let cable = status(CABLE_PID, compx::Reading { level: 64, power: 1, millivolts: 4000 });
        let receiver = status(RECEIVER_PID, compx::Reading { level: 64, power: 0, millivolts: 4000 });
        assert_eq!(cable.key, receiver.key);
        assert_eq!(cable.via, "USB cable");
        assert_eq!(receiver.via, "2.4 GHz receiver");
        assert!(cable.charging);
        assert!(!receiver.charging);
    }
}
