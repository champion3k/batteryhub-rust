//! Shared CompX report-8 battery protocol (IO Aurora, VXE / ATK mice).
//!
//! A 17-byte report with id 0x08 asks for the power details: `08 04 00 ... <checksum>`, where the
//! checksum is `0x55 - sum(bytes[0..16])`. The reply echoes the header and the command and carries
//! the level in byte 6, the power state in byte 7 (0 = battery, 1 = charging, 2 = full, 3 = unknown)
//! and big-endian millivolts in bytes 8..9. The collection is interface 1, usage page FF02, usage
//! 0002, and its output report must be able to hold the 17-byte frame.
//!
//! Only the read-only power query is sent. Idle mice stop answering within seconds; callers keep
//! their last value greyed while the receiver stays plugged in.
use crate::hid::{self, HidDev};

pub const PAYLOAD_LEN: usize = 17;
pub const REPORT_ID: u8 = 0x08;
pub const CMD_POWER: u8 = 0x04;
const CHECKSUM_BASE: u8 = 0x55;
const READ_ATTEMPTS: usize = 4;
const READ_MS: u32 = 250;

/// One valid power reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub level: u8,
    /// 0 = on battery, 1 = charging, 2 = full, 3 = unknown.
    pub power: u8,
    pub millivolts: u16,
}

impl Reading {
    pub fn charging(self) -> bool {
        self.power == 1 || self.power == 2
    }
}

pub fn checksum(frame: &[u8]) -> u8 {
    CHECKSUM_BASE.wrapping_sub(frame.iter().fold(0u8, |sum, &b| sum.wrapping_add(b)))
}

/// The read-only power query, report id included.
pub fn request() -> [u8; PAYLOAD_LEN] {
    let mut frame = [0u8; PAYLOAD_LEN];
    frame[0] = REPORT_ID;
    frame[1] = CMD_POWER;
    frame[PAYLOAD_LEN - 1] = checksum(&frame[..PAYLOAD_LEN - 1]);
    frame
}

/// The CompX control collection that can carry the 17-byte frame.
pub fn is_control(c: &hid::HidColl) -> bool {
    c.iface == 1
        && c.page() == 0xFF02
        && c.usage() == 0x0002
        && c.out_len() as usize >= PAYLOAD_LEN
        && c.in_len() as usize >= PAYLOAD_LEN
}

/// Decodes a reply. Rejects event frames, short frames, a wrong header and a bad checksum.
pub fn parse(reply: &[u8]) -> Option<Reading> {
    if reply.len() < PAYLOAD_LEN || reply[0] != REPORT_ID || reply[1] != CMD_POWER {
        return None;
    }
    let frame = &reply[..PAYLOAD_LEN];
    if frame[PAYLOAD_LEN - 1] != checksum(&frame[..PAYLOAD_LEN - 1]) {
        return None;
    }
    let level = frame[6];
    let power = frame[7];
    if level > 100 || power > 3 {
        return None;
    }
    Some(Reading { level, power, millivolts: u16::from_be_bytes([frame[8], frame[9]]) })
}

/// Sends the power query and waits for its reply. The input queue is drained first so a frame
/// queued before this request cannot be mistaken for a fresh reading.
pub fn read(c: &hid::HidColl) -> Option<Reading> {
    let d = HidDev::open(c)?;
    d.flush();
    if !d.write(&request()) {
        return None;
    }
    for _ in 0..READ_ATTEMPTS {
        let Some(reply) = d.read(READ_MS) else { break };
        if let Some(reading) = parse(&reply) {
            return Some(reading);
        }
    }
    // Some CompX receivers answer the same request as a feature report.
    if c.feat_len() as usize >= PAYLOAD_LEN {
        if let Some(reply) = d.get_feature(REPORT_ID) {
            return parse(&reply);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(level: u8, power: u8) -> Vec<u8> {
        let mut frame = vec![REPORT_ID, CMD_POWER, 0, 0, 0, 0, level, power, 0x0D, 0xED, 0, 0, 0, 0, 0, 0];
        frame.push(checksum(&frame));
        frame
    }

    #[test]
    fn request_matches_the_captured_frame() {
        // The captured IO Aurora / CompX query: checksum 0x49.
        assert_eq!(request(), [0x08, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x49]);
    }

    #[test]
    fn parses_a_captured_vxe_reply() {
        // VXE R1 SE+ 2.4 GHz: 20 %, on battery, 3565 mV, checksum 0x39.
        let frame = [0x08, 0x04, 0x00, 0x00, 0x00, 0x02, 0x14, 0x00, 0x0D, 0xED, 0, 0, 0, 0, 0, 0, 0x39];
        assert_eq!(parse(&frame), Some(Reading { level: 20, power: 0, millivolts: 3565 }));
    }

    #[test]
    fn charging_and_full() {
        assert!(parse(&reply(76, 1)).unwrap().charging());
        assert!(parse(&reply(100, 2)).unwrap().charging());
        assert!(!parse(&reply(76, 3)).unwrap().charging());
    }

    #[test]
    fn rejects_bad_frames() {
        let good = reply(42, 0);
        assert_eq!(parse(&good).unwrap().level, 42);
        let mut bad = good.clone();
        bad[16] ^= 0xFF;
        assert_eq!(parse(&bad), None);
        assert_eq!(parse(&good[..16]), None);
        assert_eq!(parse(&reply(101, 0)), None);
        assert_eq!(parse(&reply(42, 4)), None);
        let mut event = good;
        event[1] = 0x0A;
        assert_eq!(parse(&event), None);
    }
}
