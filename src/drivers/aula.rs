//! AULA F75 / F87 Pro / F99 wireless keyboards on the "BY" 2.4 GHz receiver.
//!
//! The receiver exposes a 20-byte vendor collection (usage page FF02 or FF04, usage 0002). Battery
//! is read with a 20-byte output report 0x13: byte 1 = command 0x4A, byte 2 = packet flag, byte 19 =
//! sum of bytes 0..18. The reply is another 0x13 frame with the same command (top bit may be set)
//! and the level in byte 5; byte 6's high nibble is 1 while charging. A keyboard that is dozing
//! can miss the first request, so it is resent every 1.3 s within a 4 s deadline.
//!
//! Receivers: 3554:FA09 / 3554:FB40 (Compx "BY") and 41E4:2001 (MCHOSE). 05AC:024F is shared with
//! the SONiX dongles and is not probed here.
use std::time::Instant;

use crate::device::{DeviceKind, DeviceStatus, PollContext};
use crate::hid::{self, HidDev};
use crate::util::hex4;

const RECEIVERS: [(u16, u16); 3] = [(0x3554, 0xFA09), (0x3554, 0xFB40), (0x41E4, 0x2001)];
const CMD_BATTERY: u8 = 0x4A;
const REPORT_LEN: usize = 20;
const RESEND_MS: u64 = 1300;
const DEADLINE_MS: u64 = 4000;

fn is_by_collection(c: &hid::HidColl) -> bool {
    (c.page() == 0xFF02 || c.page() == 0xFF04)
        && c.usage() == 0x0002
        && c.out_len() >= REPORT_LEN as u16
        && c.in_len() >= REPORT_LEN as u16
}

fn sum(b: &[u8]) -> u8 {
    b.iter().take(19).fold(0u8, |a, &x| a.wrapping_add(x))
}

fn request(packets: u8) -> [u8; REPORT_LEN] {
    let mut r = [0u8; REPORT_LEN];
    r[0] = 0x13;
    r[1] = CMD_BATTERY;
    r[2] = packets;
    r[19] = sum(&r);
    r
}

fn exchange(d: &HidDev) -> Option<Vec<u8>> {
    d.flush();
    let start = Instant::now();
    let mut sent = 0u64;
    loop {
        let now = start.elapsed().as_millis() as u64;
        if now >= DEADLINE_MS {
            return None;
        }
        if now >= sent * RESEND_MS {
            if !d.write(&request((sent & 1) as u8)) {
                return None;
            }
            sent += 1;
        }
        let next = (sent * RESEND_MS).min(DEADLINE_MS);
        let wait = (next.saturating_sub(now)).max(1) as u32;
        let Some(r) = d.read(wait) else { continue };
        if r.len() < REPORT_LEN || r[0] != 0x13 || (r[1] & 0x7F) != CMD_BATTERY {
            continue;
        }
        if r[4..19].iter().all(|&b| b == 0) {
            continue; // echo of our own request
        }
        let checksum = r[19] == sum(&r);
        let mchose = r[19] == 0 && r[2] == 1 && r[3] == 0;
        if !checksum && !mchose {
            continue;
        }
        return Some(r);
    }
}

pub fn poll(ctx: &mut PollContext) {
    for (vid, pid) in RECEIVERS {
        if ctx.is_claimed(vid, pid) {
            continue;
        }
        for c in ctx.find(vid, pid, is_by_collection) {
            let Some(d) = HidDev::open(&c) else { continue };
            let Some(r) = exchange(&d) else { continue };
            let level = r[5];
            if !(1..=100).contains(&level) {
                continue;
            }
            let product = hid::product_string(&c);
            let name = if product.to_ascii_lowercase().contains("f75") {
                "AULA F75"
            } else if product.to_ascii_lowercase().contains("f87") {
                "AULA F87 Pro"
            } else if product.to_ascii_lowercase().contains("f99") {
                "AULA F99"
            } else if vid == 0x41E4 {
                "MCHOSE wireless keyboard"
            } else {
                "AULA F75"
            };
            let s = DeviceStatus {
                key: format!("aula:{}:{}", hex4(vid), hex4(pid)),
                name: name.into(),
                kind: DeviceKind::Keyboard,
                via: "2.4 GHz receiver".into(),
                percent: level as i32,
                charging: (r[6] >> 4) == 1,
                grace_sec: 1800,
                ..Default::default()
            };
            ctx.add(s, vid, pid);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_checksummed() {
        let r = request(0);
        assert_eq!(r[0], 0x13);
        assert_eq!(r[1], CMD_BATTERY);
        assert_eq!(r[19], 0x5D);
        assert_eq!(r[19], sum(&r));
    }
}
