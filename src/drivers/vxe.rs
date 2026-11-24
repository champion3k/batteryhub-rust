//! VXE / ATK mice on the CompX report-8 protocol: VXE R1 SE+, VXE R1 Pro Max and the VGN
//! Dragonfly 4K. The 2.4 GHz receiver and the USB cable share the same power query (see
//! `super::compx`): interface 1, collection FF02:0002, 17-byte frame `08 04 ... <checksum>`, level
//! in byte 6, power state in byte 7 and millivolts in bytes 8..9.
//!
//! The R1 SE+ receiver is 373B:1085 and its cable is 3554:F58F. The R1 Pro Max uses 3554:F58A on
//! its 1 kHz receiver and 3554:F58C on its cable; the VGN Dragonfly 4K uses 3554:F505 / 3554:F506.
//! The cable is read first because it answers while charging and reports the charging flag. Both
//! transports keep one icon key per model.
//!
//! A VXE receiver stops answering a few seconds after the mouse idles. That is normal, not a
//! disconnect: the last value stays greyed while the receiver is still plugged in.
use std::collections::BTreeSet;

use crate::device::{DeviceKind, DeviceStatus, PollContext};

use super::compx;

const ASLEEP_SEC: u32 = 86400;

struct Model {
    vid: u16,
    pid: u16,
    name: &'static str,
    key: &'static str,
    cable: bool,
}

const MODELS: [Model; 6] = [
    Model { vid: 0x3554, pid: 0xF58F, name: "VXE R1 SE+", key: "vxe:r1seplus", cable: true },
    Model { vid: 0x3554, pid: 0xF58C, name: "VXE R1 Pro Max", key: "vxe:r1promax", cable: true },
    Model { vid: 0x3554, pid: 0xF506, name: "VGN Dragonfly 4K", key: "vxe:dragonfly4k", cable: true },
    Model { vid: 0x373B, pid: 0x1085, name: "VXE R1 SE+", key: "vxe:r1seplus", cable: false },
    Model { vid: 0x3554, pid: 0xF58A, name: "VXE R1 Pro Max", key: "vxe:r1promax", cable: false },
    Model { vid: 0x3554, pid: 0xF505, name: "VGN Dragonfly 4K", key: "vxe:dragonfly4k", cable: false },
];

fn status(m: &Model, reading: compx::Reading) -> DeviceStatus {
    DeviceStatus {
        key: m.key.into(),
        name: m.name.into(),
        via: if m.cable { "USB cable".into() } else { "2.4 GHz receiver".into() },
        kind: DeviceKind::Mouse,
        percent: reading.level as i32,
        charging: reading.charging(),
        full: reading.power == 2,
        millivolts: reading.millivolts as i32,
        grace_sec: ASLEEP_SEC,
        hw: Some((m.vid, m.pid)),
        ..Default::default()
    }
}

pub fn poll(ctx: &mut PollContext) {
    let mut seen = BTreeSet::new();
    // Cables first: a cable answers while charging, when its receiver has gone quiet.
    for cable in [true, false] {
        for m in MODELS.iter().filter(|m| m.cable == cable) {
            if seen.contains(m.key) || ctx.is_claimed(m.vid, m.pid) {
                continue;
            }
            let colls = ctx.find(m.vid, m.pid, compx::is_control);
            if colls.is_empty() {
                continue;
            }
            if let Some(reading) = colls.iter().find_map(compx::read) {
                seen.insert(m.key);
                // Claim both transports of this model so the generic fallback cannot add a twin.
                for other in MODELS.iter().filter(|o| o.key == m.key) {
                    ctx.claim(other.vid, other.pid);
                }
                ctx.add(status(m, reading), m.vid, m.pid);
            } else {
                // Present but asleep: keep the previous reading greyed and the generic fallback out.
                ctx.claim(m.vid, m.pid);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cable_and_receiver_share_one_key() {
        let cable = MODELS.iter().find(|m| m.pid == 0xF58F).unwrap();
        let receiver = MODELS.iter().find(|m| m.pid == 0x1085).unwrap();
        assert_eq!(cable.key, receiver.key);
        assert_eq!(cable.name, "VXE R1 SE+");
        assert_eq!(receiver.name, "VXE R1 SE+");
        assert_ne!(cable.key, MODELS.iter().find(|m| m.pid == 0xF58A).unwrap().key);
    }

    #[test]
    fn charging_state_comes_from_the_power_byte() {
        let m = &MODELS[0];
        let reading = |power| compx::Reading { level: 64, power, millivolts: 4000 };
        assert!(status(m, reading(1)).charging);
        assert!(status(m, reading(2)).full);
        assert!(!status(m, reading(3)).charging);
    }
}
