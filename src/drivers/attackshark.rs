//! Attack Shark mice (X1, X11 and others) on the Xenta 2.4 GHz receiver (VID 1D57, PID FA60).
//!
//! Protocol from Attack Shark Battery KDE by Gabriel Marques Ferrarezi (MIT,
//! gitlab.com/gabriel17166/Attack-Shark-Battery-KDE) and its Windows port
//! attackshark-battery-windows by daeoxide (MIT, github.com/daeoxide/attackshark-battery-windows).
//! The receiver's interface 2 (on Windows the collection with usage page 000A, 5-byte input)
//! pushes a frame about every 2 s on its own; nothing has to be sent:
//!
//!   03 MM 40 ST NN
//!   03     report ID
//!   MM     model marker, differs per mouse (E5, 55 on the X11, ...): not checked
//!   40     battery frame
//!   ST     01 on battery, 02 on power (not charging), 03 charging
//!   NN     battery level 0..100 % (frozen and approximate while on power)
//!
//! Link-blink frames (`03 MM 01 00 00`) are not battery frames. Right after the link comes up the
//! mouse may send one `03 MM 40 01 64` (100 %) that is wrong, so a frame counts only once the same
//! state and level were seen twice in a row. No frame within the timeout means the mouse is off
//! (the receiver stays enumerated). With the cable plugged in, the receiver goes silent.
use crate::device::{DeviceKind, DeviceStatus, PollContext};
use crate::hid::HidDev;
use crate::util::hex4;

const VID: u16 = 0x1D57;
const RECEIVER: u16 = 0xFA60;
const ON_BATTERY: u8 = 0x01;
const ON_POWER: u8 = 0x02;
const CHARGING: u8 = 0x03;
/// Frames come about every 2 s; two matching ones are needed.
const WAIT_MS: u64 = 5500;

/// Decodes one input report (report ID included): Some((level, state)).
pub fn decode(r: &[u8]) -> Option<(i32, u8)> {
    if r.len() < 5 || r[0] != 0x03 || r[2] != 0x40 || r[4] > 100 {
        return None;
    }
    Some((r[4] as i32, r[3]))
}

/// Picks the reading from the frames seen during one poll: the first one confirmed by an equal
/// frame right after it, else the last one (unless it is the lone 100 % reconnect frame).
pub fn pick(frames: &[(i32, u8)]) -> Option<(i32, u8)> {
    if let Some(w) = frames.windows(2).find(|w| w[0] == w[1]) {
        return Some(w[0]);
    }
    match frames.last() {
        Some(&(100, ON_BATTERY)) if frames.len() == 1 => None,
        other => other.copied(),
    }
}

fn base() -> DeviceStatus {
    DeviceStatus {
        key: format!("attackshark:{}", hex4(RECEIVER)),
        name: "Attack Shark mouse".into(),
        kind: DeviceKind::Mouse,
        via: "2.4 GHz receiver".into(),
        hw: Some((VID, RECEIVER)),
        grace_sec: 60,
        ..Default::default()
    }
}

fn read_frames(d: &HidDev) -> Option<(i32, u8)> {
    let end = crate::util::Deadline::in_ms(WAIT_MS);
    let mut frames = Vec::new();
    loop {
        let left = end.left_ms();
        if left == 0 {
            return pick(&frames);
        }
        if let Some(f) = d.read(left).and_then(|r| decode(&r)) {
            if frames.last() == Some(&f) {
                return Some(f); // confirmed
            }
            frames.push(f);
        }
    }
}

pub fn poll(ctx: &mut PollContext) {
    for (vid, pid) in ctx.products(VID) {
        if pid != RECEIVER {
            continue;
        }
        // The battery collection: page 000A (or a vendor page) with a 5-byte input report.
        let colls = ctx.find(vid, pid, |c| (c.page() == 0x000A || c.page() >= 0xFF00) && c.in_len() >= 5);
        if colls.is_empty() {
            continue;
        }
        let mut found = None;
        for c in &colls {
            let Some(d) = HidDev::open_read_only(c) else { continue };
            found = read_frames(&d);
            if found.is_some() {
                break;
            }
        }
        let s = match found {
            Some((level, state)) => DeviceStatus {
                percent: level,
                charging: state == CHARGING,
                detail: match state {
                    ON_BATTERY => String::new(),
                    ON_POWER => "on power".into(),
                    CHARGING => "charging".into(),
                    other => format!("state {other:02X}"),
                },
                ..base()
            },
            None => DeviceStatus { connected: false, ..base() },
        };
        ctx.add(s, vid, pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_model_marker() {
        assert_eq!(decode(&[0x03, 0xE5, 0x40, 0x01, 0x4B]), Some((75, ON_BATTERY)));
        assert_eq!(decode(&[0x03, 0x55, 0x40, 0x03, 0x32]), Some((50, CHARGING)));
        assert_eq!(decode(&[0x03, 0x00, 0x40, 0x02, 0x50]), Some((80, ON_POWER)));
    }

    #[test]
    fn not_battery_frames() {
        assert_eq!(decode(&[0x03, 0xE5, 0x01, 0x00, 0x00]), None);
        assert_eq!(decode(&[0x03, 0xE5, 0x40, 0x01, 0xC8]), None);
        assert_eq!(decode(&[0x03, 0xE5, 0x40]), None);
        assert_eq!(decode(&[0x02, 0xE5, 0x40, 0x01, 0x32]), None);
    }

    #[test]
    fn confirmation() {
        // Reconnect frame alone is not trusted; the real level confirmed twice wins.
        assert_eq!(pick(&[(100, ON_BATTERY)]), None);
        assert_eq!(pick(&[(100, ON_BATTERY), (62, ON_BATTERY), (62, ON_BATTERY)]), Some((62, ON_BATTERY)));
        assert_eq!(pick(&[(62, ON_BATTERY)]), Some((62, ON_BATTERY)));
        assert_eq!(pick(&[]), None);
    }
}
