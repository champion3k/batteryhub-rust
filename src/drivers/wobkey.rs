//! WOBKEY Rainy 75 Pro on the Evision keyboard controller.
//!
//! The wired keyboard enumerates as 320F:5055, the 2.4 GHz dongle as 320F:5088. Both expose the
//! vendor collection FF1C:0092 with report ID 4. A 32-byte output report carries a little-endian
//! request id at bytes 1..2, command 0x1A at byte 3 and zeroes after it; the input report echoes the
//! id and command, then carries the level at byte 8 and the charge state at byte 9 (0 = on battery,
//! 1 = charging, 2 = charging variant). Nothing is written except this read-only query.
//!
//! Protocol from ATK HUB's Evision keyboard driver, as documented by HaloBattery (ATK V75X); the
//! same Evision firmware family is used by WOBKEY, VGN and other keyboards sharing these ids, so the
//! reply is validated strictly and the product string names the device.
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use crate::device::{DeviceKind, DeviceStatus, PollContext};
use crate::hid::{self, HidDev};

const VID: u16 = 0x320F;
const WIRED_PID: u16 = 0x5055;
const RECEIVER_PID: u16 = 0x5088;
const PAGE: u16 = 0xFF1C;
const USAGE: u16 = 0x0092;
const REPORT_ID: u8 = 0x04;
const CMD_BATTERY: u8 = 0x1A;
const REQUEST_LEN: usize = 32;
const READ_MS: u32 = 100;
const DEADLINE_MS: u64 = 600;

static SEQUENCE: AtomicU16 = AtomicU16::new(0);

fn is_collection(c: &hid::HidColl) -> bool {
    c.page() == PAGE && c.usage() == USAGE && c.out_len() >= REQUEST_LEN as u16 && c.in_len() >= 10
}

fn request(sequence: u16) -> [u8; REQUEST_LEN] {
    let mut r = [0u8; REQUEST_LEN];
    r[0] = REPORT_ID;
    r[1] = (sequence & 0xFF) as u8;
    r[2] = (sequence >> 8) as u8;
    r[3] = CMD_BATTERY;
    r
}

/// Reply to `sequence`: (level, charging). Rejects anything that is not the echo of this request.
fn parse(reply: &[u8], sequence: u16) -> Option<(u8, bool)> {
    if reply.len() < 10
        || reply[0] != REPORT_ID
        || reply[1] != (sequence & 0xFF) as u8
        || reply[2] != (sequence >> 8) as u8
        || reply[3] != CMD_BATTERY
        || reply[5] != 0
        || reply[6] != 0
    {
        return None;
    }
    let level = reply[8];
    let state = reply[9];
    if level > 100 || state > 2 {
        return None;
    }
    Some((level, state == 1 || state == 2))
}

fn read_battery(c: &hid::HidColl) -> Option<(u8, bool)> {
    let d = HidDev::open(c)?;
    d.flush();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if !d.write(&request(sequence)) {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(DEADLINE_MS);
    while Instant::now() < deadline {
        let Some(reply) = d.read(READ_MS) else { continue };
        if let Some(answer) = parse(&reply, sequence) {
            return Some(answer);
        }
    }
    None
}

fn display_name(product: &str) -> String {
    let p = product.trim();
    let lower = p.to_ascii_lowercase();
    if lower.contains("rainy") || lower.contains("wobkey") {
        "WOBKEY Rainy 75 Pro".into()
    } else if p.is_empty()
        || lower == "telink"
        || lower.contains("wireless gaming keyboard")
        || lower.contains("2.4g dongle")
        || lower.contains("2.4 ghz dongle")
    {
        "Wireless keyboard".into()
    } else {
        p.to_string()
    }
}

pub fn poll(ctx: &mut PollContext) {
    // The cable is read first: it answers while charging and names the keyboard.
    for (pid, via) in [(WIRED_PID, "USB cable"), (RECEIVER_PID, "2.4 GHz receiver")] {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        for c in ctx.find(VID, pid, is_collection) {
            let Some((level, charging)) = read_battery(&c) else { continue };
            let s = DeviceStatus {
                key: format!("wobkey:{}", crate::util::hex4(VID)),
                name: display_name(&hid::product_string(&c)),
                kind: DeviceKind::Keyboard,
                via: via.into(),
                percent: level as i32,
                charging,
                grace_sec: 600,
                hw: Some((VID, pid)),
                ..Default::default()
            };
            // The cable and the receiver are one keyboard: keep the generic fallback off both.
            ctx.claim(VID, WIRED_PID);
            ctx.claim(VID, RECEIVER_PID);
            ctx.add(s, VID, pid);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_echoes_the_sequence_and_command() {
        let r = request(0x7B01);
        assert_eq!(&r[..4], &[0x04, 0x01, 0x7B, 0x1A]);
        assert!(r[4..].iter().all(|&b| b == 0));
    }

    #[test]
    fn parses_a_captured_reply() {
        // ATK V75X 2.4 GHz capture: 60%, charging.
        let reply = [0x04, 0x01, 0x7B, 0x1A, 0x00, 0x00, 0x00, 0x00, 0x3C, 0x01, 0x00];
        assert_eq!(parse(&reply, 0x7B01), Some((60, true)));
    }

    #[test]
    fn rejects_a_reply_to_another_request_or_a_bad_state() {
        let reply = [0x04, 0x01, 0x7B, 0x1A, 0x00, 0x00, 0x00, 0x00, 0x3C, 0x00, 0x00];
        assert_eq!(parse(&reply, 0x7B02), None);
        assert_eq!(parse(&reply, 0x7B01), Some((60, false)));
        let mut bad = reply;
        bad[9] = 3;
        assert_eq!(parse(&bad, 0x7B01), None);
        let mut high = reply;
        high[8] = 101;
        assert_eq!(parse(&high, 0x7B01), None);
    }

    #[test]
    fn names_known_and_generic_products() {
        assert_eq!(display_name("Rainy 75 Pro"), "WOBKEY Rainy 75 Pro");
        assert_eq!(display_name("Rainy 75 2.4G Dongle"), "WOBKEY Rainy 75 Pro");
        assert_eq!(display_name("Wireless Gaming Keyboard"), "Wireless keyboard");
        assert_eq!(display_name("ATK V75X"), "ATK V75X");
    }
}
