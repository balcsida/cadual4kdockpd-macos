//! Optional: updates the dock's two Genesys GL3523 USB hubs to i-tec's firmware 93.92.
//!
//! The requests are the ones u2glhub documents; the sequence (sector erase, 64-byte writes, status
//! polling, read-back) is the one of fwupd's genesys plugin for hubs without code signing, which is
//! the tool that updated the first dock's hubs. On Windows a program can reach a hub only through a
//! kernel driver. i-tec's own updater installs one; this tool does not, and points to that updater.

// on Windows only the refusal below is used
#![cfg_attr(windows, allow(dead_code))]

use crate::image::sha256_hex;
use std::time::Duration;

/// Vendor control transfers to one hub.
pub trait HubLink {
    fn ctrl_in(&mut self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>, String>;
    fn ctrl_out(&mut self, request: u8, value: u16, index: u16, data: &[u8]) -> Result<(), String>;
    fn pause(&mut self, time: Duration) {
        std::thread::sleep(time);
    }
}

const SWITCH: u8 = 0x81;
const READ: u8 = 0x82;
const WRITE: u8 = 0x83;
const ISP_ENTER: u16 = 0x0001;
const RESET: u16 = 0x0003;
/// Reads the flash's status register; zero means idle.
const STATUS: u16 = 0x0502;
const SECTOR_ERASE: u16 = 0x2001;
const SECTOR: usize = 4096;
const CHUNK: usize = 64;

/// Both hub images are this long; the hubs' 64 KiB flash is blank after it.
pub const ROM_LEN: usize = 0x6000;
pub const TARGET_VERSION: u16 = 0x9392;
const KNOWN_OLD_VERSION: u16 = 0x9305;

const FIRST_SHA256: &str = "9dc14b65f251de4a74aa8f5a613d90607a87333ff71fc26079eb82d4e7c69f6c";
const SECOND_SHA256: &str = "69ba8fe38a9c020bcca428bb795e10e177b2e5101fa58cfa2270c61df1ca3880";
pub const FIRST_FILE: &str = "Hub_1_9392_L1.ROM";
pub const SECOND_FILE: &str = "Hub_1_9392_L2.ROM";

// Firmware header: "XROM" at 0xFC, the USB IDs, then configuration. Byte 0x108 tells the two hubs
// apart (fwupd shows it as HUB_16 and HUB_00); the version is the little-endian word at 0x10E.
const HEADER: usize = 0xfc;
const HEADER_START: [u8; 11] = *b"XROM\x55\xe3\x05\x20\x06\x10\x06";
const LEVEL_AT: usize = 0x108;
const VERSION_AT: usize = 0x10e;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// The hub the computer connects to (i-tec's L1 image).
    First,
    /// The hub behind it (i-tec's L2 image).
    Second,
}

/// i-tec's two hub images, checked against their SHA-256.
pub struct Roms {
    pub first: Vec<u8>,
    pub second: Vec<u8>,
}

impl Roms {
    pub fn new(first: &[u8], second: &[u8]) -> Result<Self, String> {
        if sha256_hex(first) != FIRST_SHA256 || sha256_hex(second) != SECOND_SHA256 {
            return Err(format!("{FIRST_FILE} or {SECOND_FILE} is not the expected 93.92 image"));
        }
        Ok(Roms { first: first.to_vec(), second: second.to_vec() })
    }

    pub fn from_dir(dir: &std::path::Path) -> Result<Self, String> {
        let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| format!("cannot read {name} in {}: {e}", dir.display()));
        Self::new(&read(FIRST_FILE)?, &read(SECOND_FILE)?)
    }

    fn of(&self, level: Level) -> &[u8] {
        if level == Level::First { &self.first } else { &self.second }
    }
}

/// What one hub holds.
#[derive(Debug, PartialEq, Eq)]
pub struct Found {
    pub level: Level,
    pub version: u16,
    /// The flash already equals the 93.92 image for this hub.
    pub current: bool,
}

pub struct Hub<L: HubLink> {
    pub link: L,
}

impl<L: HubLink> Hub<L> {
    fn read_flash(&mut self, start: usize, len: usize) -> Result<Vec<u8>, String> {
        let mut data = Vec::with_capacity(len);
        while data.len() < len {
            let addr = start + data.len();
            let want = CHUNK.min(len - data.len());
            let chunk = self.link.ctrl_in(READ, ((addr >> 16) as u16 & 0xf) << 12, addr as u16, want)?;
            if chunk.len() != want {
                return Err(format!("short read at {addr:#x}"));
            }
            data.extend(chunk);
        }
        Ok(data)
    }

    /// Read only. Which hub this is, its version, and whether it already holds the new image.
    pub fn inspect(&mut self, roms: &Roms) -> Result<Found, String> {
        let flash = self.read_flash(0, ROM_LEN)?;
        if flash[HEADER..HEADER + HEADER_START.len()] != HEADER_START {
            return Err("this hub's firmware does not have the expected header".into());
        }
        let level = match flash[LEVEL_AT] {
            0x16 => Level::First,
            0x00 => Level::Second,
            other => return Err(format!("unknown hub type {other:#04x}")),
        };
        // the image for this hub must agree on that byte, or the mapping above is wrong
        if roms.of(level)[LEVEL_AT] != flash[LEVEL_AT] {
            return Err("the hub image does not fit this hub".into());
        }
        let version = u16::from_le_bytes([flash[VERSION_AT], flash[VERSION_AT + 1]]);
        let current = flash == roms.of(level);
        if !current && version != KNOWN_OLD_VERSION && version != TARGET_VERSION {
            return Err(format!("unknown hub firmware version {version:04X}"));
        }
        Ok(Found { level, version, current })
    }

    fn wait_idle(&mut self, tries: u32) -> Result<(), String> {
        for _ in 0..tries {
            if self.link.ctrl_in(READ, STATUS, 0, 1)? == [0] {
                return Ok(());
            }
            self.link.pause(Duration::from_millis(30));
        }
        Err("the hub's flash stayed busy".into())
    }

    /// Erases the image's sectors, writes `rom`, reads it back and restarts the hub.
    /// An error after the erase leaves the hub's flash incomplete: the update must be run again.
    pub fn update(&mut self, rom: &[u8], name: &'static str) -> Result<(), String> {
        self.link.ctrl_out(SWITCH, ISP_ENTER, 0, &[])?;
        self.wait_idle(5)?;
        for sector in 0..rom.len().div_ceil(SECTOR) {
            crate::progress(name, sector, 3 * rom.len().div_ceil(SECTOR));
            // index: 0x01, then the 4 KiB sector within the 64 KiB block, then the block (0)
            self.link.ctrl_out(WRITE, SECTOR_ERASE, 0x0100 | (sector as u16) << 4, &[])?;
            self.wait_idle(266)?;
        }
        if self.read_flash(0, rom.len())?.iter().any(|&b| b != 0xff) {
            return Err("the hub's flash is not blank after the erase".into());
        }
        for (n, chunk) in rom.chunks(CHUNK).enumerate() {
            crate::progress(name, rom.len() / CHUNK / 2 + n, rom.len() / CHUNK * 3 / 2);
            self.link.ctrl_out(WRITE, 0, (n * CHUNK) as u16, chunk)?;
            self.wait_idle(16)?;
        }
        if self.read_flash(0, rom.len())? != rom {
            return Err("the hub's flash does not match the image after writing".into());
        }
        // the hub drops off the bus while it answers, so this request normally reports an error
        let _ = self.link.ctrl_out(SWITCH, RESET, 0, &[]);
        Ok(())
    }
}

#[cfg(not(windows))]
mod usb {
    use super::{Found, Hub, HubLink, Level, Roms, TARGET_VERSION};
    use rusb::{Direction, Recipient, RequestType, UsbContext as _};
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(5);

    struct Usb(rusb::DeviceHandle<rusb::Context>);

    impl HubLink for Usb {
        fn ctrl_in(&mut self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>, String> {
            let mut data = vec![0u8; len];
            let kind = rusb::request_type(Direction::In, RequestType::Vendor, Recipient::Device);
            let n = self.0.read_control(kind, request, value, index, &mut data, TIMEOUT).map_err(|e| format!("USB read failed: {e}"))?;
            data.truncate(n);
            Ok(data)
        }

        fn ctrl_out(&mut self, request: u8, value: u16, index: u16, data: &[u8]) -> Result<(), String> {
            let kind = rusb::request_type(Direction::Out, RequestType::Vendor, Recipient::Device);
            self.0.write_control(kind, request, value, index, data, TIMEOUT).map(|_| ()).map_err(|e| format!("USB write failed: {e}"))
        }
    }

    /// Every Genesys USB 2 hub half (05E3:0610) on the bus; the dock has two.
    fn hubs() -> Result<Vec<Hub<Usb>>, String> {
        let context = rusb::Context::new().map_err(|e| format!("cannot start USB access: {e}"))?;
        let devices = context.devices().map_err(|e| format!("cannot list USB devices: {e}"))?;
        let mut hubs = Vec::new();
        for device in devices.iter() {
            let is_hub = device.device_descriptor().is_ok_and(|d| (d.vendor_id(), d.product_id()) == (0x05e3, 0x0610));
            if is_hub {
                let handle = device.open().map_err(|e| format!("cannot open a hub: {e}"))?;
                hubs.push(Hub { link: Usb(handle) });
            }
        }
        Ok(hubs)
    }

    /// The dock's hubs and what each holds. Refuses anything but exactly one first and one second hub.
    fn survey(roms: &Roms) -> Result<Vec<(Hub<Usb>, Found)>, String> {
        let mut found = Vec::new();
        for mut hub in hubs()? {
            let state = hub.inspect(roms)?;
            say!("{:?} hub: firmware {:04X}{}", state.level, state.version, if state.current { ", already the 93.92 image" } else { "" });
            found.push((hub, state));
        }
        found.sort_by_key(|(_, state)| state.level);
        let levels: Vec<Level> = found.iter().map(|(_, state)| state.level).collect();
        if levels.is_empty() {
            return Err("The dock's hubs were not found. Connect the dock and wait until its devices appear.".into());
        }
        if levels != [Level::First, Level::Second] {
            return Err(format!("expected one first and one second hub, found {levels:?}. Connect only one dock."));
        }
        Ok(found)
    }

    /// Read only.
    pub fn check(roms: &Roms) -> Result<String, String> {
        let found = survey(roms)?;
        Ok(if found.iter().all(|(_, state)| state.current) {
            "Both hubs already hold firmware 93.92.".into()
        } else {
            "The hubs can be updated to firmware 93.92.".into()
        })
    }

    pub fn update(roms: &Roms, confirm: impl FnOnce() -> bool) -> Result<String, String> {
        let mut found = survey(roms)?;
        if found.iter().all(|(_, state)| state.current) {
            return Ok("Both hubs already hold firmware 93.92. Nothing was changed.".into());
        }
        if !confirm() {
            return Err("stopped. Nothing was changed.".into());
        }
        crate::guard::keep_awake();
        crate::guard::ignore_interrupts();
        // the second hub first: restarting the first one takes the second off the bus
        found.reverse();
        for (hub, state) in &mut found {
            if state.current {
                continue;
            }
            let name = if state.level == Level::First { "Updating the first hub" } else { "Updating the second hub" };
            say!("{name}");
            hub.update(roms.of(state.level), name).map_err(|why| {
                format!("{why}. HUB UPDATE NOT FINISHED: keep the dock powered and connected, and run the hub update again.")
            })?;
            say!("written and read back; the hub restarts");
        }
        crate::progress("", 0, 0);
        drop(found);
        // the dock's devices come back within a few seconds; then read the result from the hubs themselves
        let start = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(3));
            match survey(roms) {
                Ok(found) if found.iter().all(|(_, state)| state.current && state.version == TARGET_VERSION) => {
                    return Ok("HUBS UPDATED: both hold firmware 93.92. Restart the dock (unplug its power for 15 seconds) to finish.".into());
                }
                Ok(_) => return Err("the hubs restarted but do not report the new firmware. Run the hub check.".into()),
                Err(_) if start.elapsed() < Duration::from_secs(45) => say!("waiting for the hubs to come back"),
                Err(why) => return Err(format!("the hubs did not come back: {why}. Replug the dock and run the hub check.")),
            }
        }
    }
}

#[cfg(not(windows))]
pub use usb::{check, update};

#[cfg(windows)]
pub const NOT_ON_WINDOWS: &str = "On Windows, update the hubs with i-tec's own program, CADUAL4KDOCKPD_9392.exe: \
Windows needs the driver that program installs to reach a hub. This tool does the hub update on a Mac.";

#[cfg(windows)]
pub fn check(_roms: &Roms) -> Result<String, String> {
    Err(NOT_ON_WINDOWS.into())
}

#[cfg(windows)]
pub fn update(_roms: &Roms, _confirm: impl FnOnce() -> bool) -> Result<String, String> {
    Err(NOT_ON_WINDOWS.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model of a GL3523 in its flash commands, as far as this module uses them.
    struct Sim {
        flash: Vec<u8>,
        isp: bool,
        busy_polls: u8,
        resets: u32,
        stuck_bit_at: Option<usize>,
    }

    impl Sim {
        fn holding(image: &[u8]) -> Self {
            let mut flash = image.to_vec();
            flash.resize(0x10000, 0xff);
            Sim { flash, isp: false, busy_polls: 0, resets: 0, stuck_bit_at: None }
        }
    }

    impl HubLink for Sim {
        fn ctrl_in(&mut self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>, String> {
            assert_eq!(request, READ);
            if value == STATUS {
                let busy = self.busy_polls > 0;
                self.busy_polls = self.busy_polls.saturating_sub(1);
                return Ok(vec![u8::from(busy)]);
            }
            let addr = usize::from(value >> 12) << 16 | usize::from(index);
            Ok(self.flash[addr..addr + len].to_vec())
        }

        fn ctrl_out(&mut self, request: u8, value: u16, index: u16, data: &[u8]) -> Result<(), String> {
            match (request, value) {
                (SWITCH, ISP_ENTER) => self.isp = true,
                (SWITCH, RESET) => {
                    self.resets += 1;
                    self.isp = false;
                    return Err("the device went away".into());
                }
                (WRITE, SECTOR_ERASE) if self.isp => {
                    let start = usize::from(index & 0xf) << 16 | usize::from(index >> 4 & 0xf) << 12;
                    self.flash[start..start + SECTOR].fill(0xff);
                    self.busy_polls = 2;
                }
                (WRITE, page) if self.isp => {
                    let start = usize::from(page >> 12) << 16 | usize::from(index);
                    for (i, &byte) in data.iter().enumerate() {
                        self.flash[start + i] &= byte; // programming can only clear bits
                        if self.stuck_bit_at == Some(start + i) {
                            self.flash[start + i] |= 0x01;
                        }
                    }
                    self.busy_polls = 1;
                }
                _ => return Err("request refused".into()),
            }
            Ok(())
        }

        fn pause(&mut self, _time: Duration) {}
    }

    /// A made-up image with the real header fields.
    fn rom(level: u8, version: u16, fill: u8) -> Vec<u8> {
        let mut image: Vec<u8> = (0..ROM_LEN).map(|i| (i as u8) ^ fill).collect();
        image[HEADER..HEADER + HEADER_START.len()].copy_from_slice(&HEADER_START);
        image[LEVEL_AT] = level;
        image[VERSION_AT..VERSION_AT + 2].copy_from_slice(&version.to_le_bytes());
        image
    }

    fn roms() -> Roms {
        Roms { first: rom(0x16, TARGET_VERSION, 0xa5), second: rom(0x00, TARGET_VERSION, 0xa5) }
    }

    fn hub_holding(image: &[u8]) -> Hub<Sim> {
        Hub { link: Sim::holding(image) }
    }

    #[test]
    fn inspect_should_recognise_an_old_first_hub() {
        let found = hub_holding(&rom(0x16, 0x9305, 0x3c)).inspect(&roms());
        assert_eq!(found, Ok(Found { level: Level::First, version: 0x9305, current: false }));
    }

    #[test]
    fn inspect_should_recognise_an_updated_second_hub() {
        let found = hub_holding(&roms().second).inspect(&roms());
        assert_eq!(found, Ok(Found { level: Level::Second, version: TARGET_VERSION, current: true }));
    }

    #[test]
    fn inspect_should_refuse_firmware_without_the_header() {
        assert!(hub_holding(&[0u8; ROM_LEN]).inspect(&roms()).is_err());
    }

    #[test]
    fn inspect_should_refuse_an_unknown_version() {
        assert!(hub_holding(&rom(0x16, 0x9100, 0x3c)).inspect(&roms()).is_err());
    }

    #[test]
    fn inspect_should_refuse_an_unknown_hub_type() {
        assert!(hub_holding(&rom(0x55, 0x9305, 0x3c)).inspect(&roms()).is_err());
    }

    #[test]
    fn update_should_leave_exactly_the_new_image_and_restart_the_hub() {
        let mut hub = hub_holding(&rom(0x16, 0x9305, 0x3c));
        let result = hub.update(&roms().first, "test");
        assert_eq!((result, &hub.link.flash[..ROM_LEN] == roms().first.as_slice(), hub.link.resets), (Ok(()), true, 1));
    }

    #[test]
    fn update_should_not_touch_the_flash_beyond_the_image() {
        let mut hub = hub_holding(&rom(0x00, 0x9305, 0x3c));
        hub.link.flash[0x7000] = 0x42;
        let _ = hub.update(&roms().second, "test");
        assert_eq!(hub.link.flash[0x7000], 0x42);
    }

    #[test]
    fn update_should_fail_without_restarting_when_a_byte_does_not_program() {
        let mut hub = hub_holding(&rom(0x16, 0x9305, 0x3c));
        hub.link.stuck_bit_at = Some(0x1235); // a byte whose lowest bit must become 0
        let result = hub.update(&roms().first, "test");
        assert_eq!((result.is_err(), hub.link.resets), (true, 0));
    }

    /// With the real files: `DOCK_HUB_DIR` holds i-tec's two images, `DOCK_HUB_DUMPS` the two 93.05
    /// dumps that fwupd read from a dock (`hub-HUB_16-9305.bin`, `hub-HUB_00-9305.bin`).
    #[test]
    #[ignore = "needs i-tec's hub images and dumps of a real dock, which are not part of this repository"]
    fn real_dumps_should_be_recognised_and_updated_to_the_real_images() {
        let dir = std::env::var("DOCK_HUB_DIR").unwrap_or_default();
        let dumps = std::path::PathBuf::from(std::env::var("DOCK_HUB_DUMPS").unwrap_or_default());
        let Ok(roms) = Roms::from_dir(std::path::Path::new(&dir)) else { panic!("the real hub images are needed") };
        for (dump, level) in [("hub-HUB_16-9305.bin", Level::First), ("hub-HUB_00-9305.bin", Level::Second)] {
            let mut hub = hub_holding(&std::fs::read(dumps.join(dump)).unwrap_or_default());
            assert_eq!(hub.inspect(&roms), Ok(Found { level, version: 0x9305, current: false }));
            assert_eq!(hub.update(roms.of(level), "test"), Ok(()));
            assert_eq!(hub.inspect(&roms), Ok(Found { level, version: TARGET_VERSION, current: true }));
        }
    }

    #[test]
    fn roms_should_reject_files_that_are_not_the_pinned_images() {
        assert!(Roms::new(&rom(0x16, TARGET_VERSION, 0), &rom(0x00, TARGET_VERSION, 0)).is_err());
    }
}
