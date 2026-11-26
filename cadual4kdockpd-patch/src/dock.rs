//! The dock's management chip (Good Way, USB 065F:F60F) and the erase/write/verify sequence.
//!
//! The chip can only write the power controller's flash after erasing the whole chip, and it has no
//! command to read the flash back: the only view of the content is a 16-bit sum of the first N bytes.
//! Protocol details: `dock-capture/gw-updater/PROTOCOL.md`.

use crate::image::{FRAME_DATA, data_frame, frames, padded, sum16};
use std::time::{Duration, Instant};

/// Moves 64-byte HID reports to and from the chip.
pub trait Transport {
    fn send(&mut self, frame: &[u8; 64]) -> Result<(), String>;
    /// The next report from the chip, waiting at most `timeout`. `None`: nothing arrived.
    fn recv(&mut self, timeout: Duration) -> Option<[u8; 64]>;
    fn pause(&mut self, time: Duration) {
        std::thread::sleep(time);
    }
}

/// The only chip firmware this sequence has been run on.
pub const VERSION_E103: [u8; 4] = [0x45, 0x01, 0x00, 0x03];

const VERSION: u8 = 0x08;
const OPEN: u8 = 0x09;
const ERASE: u8 = 0x0a;
const STATUS: u8 = 0x0c;
const SUM: u8 = 0x0d;
const CLOSE: u8 = 0x0e;
/// The chip answers every data frame with this byte and the frame's sequence number.
const ACK: u8 = 0x3b;

// The chip stops waiting for a page program after 20 status reads and acknowledges anyway.
// This is the flash's worst-case page time, so the next page never meets a busy flash.
const PAGE_TIME: Duration = Duration::from_millis(5);
/// Sum of 4096 blank (FF) bytes.
const BLANK_4K: u16 = 0xf000;

pub struct Dock<T: Transport> {
    pub link: T,
    odd_shown: u32,
}

impl<T: Transport> Dock<T> {
    pub fn new(link: T) -> Self {
        Dock { link, odd_shown: 0 }
    }

    fn exchange(&mut self, frame: &[u8; 64], timeout: Duration, accept: impl Fn(&[u8; 64]) -> bool) -> Option<[u8; 64]> {
        while self.link.recv(Duration::ZERO).is_some() {} // nothing left over from an earlier exchange may answer this one
        if let Err(why) = self.link.send(frame) {
            say!("send failed: {why}");
            return None;
        }
        let deadline = Instant::now() + timeout;
        loop {
            let reply = self.link.recv(deadline.saturating_duration_since(Instant::now()))?;
            if accept(&reply) {
                return Some(reply);
            }
            // with an empty flash these lines are the only way to see what the chip is doing
            if self.odd_shown < 40 {
                self.odd_shown += 1;
                say!("unexpected report: {:02x?}", &reply[..12]);
            }
            if Instant::now() >= deadline {
                return None;
            }
        }
    }

    fn cmd(&mut self, code: u8, args: &[u8], timeout: Duration) -> Option<[u8; 64]> {
        let mut f = [0u8; 64];
        f[..5].copy_from_slice(&[0x02, 0x04, 0xfe, 0xff, code]);
        f[5..5 + args.len()].copy_from_slice(args);
        f[63] = 0x5a;
        let reply = self.exchange(&f, timeout, |r| r[1] == code && r[2] == 0xfe && r[3] == 0xff && r[4] == code);
        if reply.is_none() {
            say!("no valid reply to command {code:02X} within {} s", timeout.as_secs());
        }
        reply
    }

    /// The chip's own firmware version, for example `45 01 00 03` for E103.
    pub fn version(&mut self) -> Option<[u8; 4]> {
        self.cmd(VERSION, &[], Duration::from_secs(5)).map(|r| [r[5], r[6], r[7], r[8]])
    }

    /// Gives the chip access to the power controller's flash. Nothing is changed by this.
    pub fn open(&mut self) -> bool {
        self.cmd(OPEN, &[], Duration::from_secs(10)).is_some_and(|r| r[5] == 0)
    }

    /// Releases the flash again.
    pub fn close(&mut self) {
        if self.cmd(CLOSE, &[], Duration::from_secs(5)).is_none() {
            say!("WARNING: the chip did not confirm releasing the flash");
        }
    }

    /// The flash's first status register: bit 0 is "busy", bits 2 and up are write protection.
    pub fn status(&mut self) -> Option<u8> {
        self.cmd(STATUS, &[], Duration::from_secs(5)).map(|r| r[5])
    }

    /// 16-bit sum of the first `n` bytes of the flash.
    pub fn sum(&mut self, n: usize) -> Option<u16> {
        let length = u32::try_from(n).ok()?.to_be_bytes();
        self.cmd(SUM, &length, Duration::from_secs(240)).map(|r| u16::from_be_bytes([r[5], r[6]]))
    }

    /// Checks that the flash holds `image` followed by blank space: a sum at every 4 KB and one over
    /// the whole image. Takes two to three minutes.
    pub fn holds(&mut self, image: &[u8]) -> Result<(), String> {
        let img = padded(image);
        let lengths: Vec<usize> = (1..).map(|k| k * 4096).take_while(|&n| n <= img.len() + 8192).chain([img.len()]).collect();
        for (done, &n) in lengths.iter().enumerate() {
            crate::progress("Checking the flash", done, lengths.len());
            match self.sum(n) {
                Some(s) if s == sum16(&img, n) => {}
                Some(s) => return Err(format!("the first {n} bytes sum to {s:04X}, expected {:04X}", sum16(&img, n))),
                None => return Err(format!("no checksum came back for the first {n} bytes")),
            }
        }
        Ok(())
    }

    fn send_data(&mut self, frame: &[u8; 64]) -> bool {
        for attempt in 1..=4 {
            let wait = Duration::from_secs(if attempt == 1 { 3 } else { 6 });
            // the chip ignores a frame it has already taken, so sending it again is safe
            if self.exchange(frame, wait, |r| r[1] == ACK && r[2..4] == frame[2..4]).is_some() {
                return true;
            }
            say!("no acknowledgement for frame {}, attempt {attempt}", u16::from_le_bytes([frame[2], frame[3]]));
        }
        false
    }

    fn erase_and_wait(&mut self) -> bool {
        if !self.cmd(ERASE, &[], Duration::from_secs(30)).is_some_and(|r| r[5] == 0) {
            say!("erase command not accepted");
            return false;
        }
        crate::progress("Erasing", 0, 1);
        let start = Instant::now();
        let mut told = 0;
        while start.elapsed() < Duration::from_secs(240) {
            self.link.pause(Duration::from_millis(100));
            let Some(status) = self.status() else { continue };
            if status & 0x01 == 0 {
                say!("erase finished after {:.1} s", start.elapsed().as_secs_f32());
                return true;
            }
            if start.elapsed().as_secs() >= told + 10 {
                told = start.elapsed().as_secs();
                say!("  erasing, {told} s");
            }
        }
        say!("flash still busy after 240 s");
        false
    }

    /// Erases the flash, writes `image` and verifies it, up to three times. The flash must be open.
    /// `false` means the flash may be empty or half written: the dock must stay powered.
    pub fn write(&mut self, image: &[u8]) -> bool {
        let img = padded(image);
        let frames = frames(&img);
        for attempt in 1..=3 {
            // a failed erase makes the chip release the flash, so open it again before a retry
            if attempt > 1 && !self.open() {
                say!("flash access failed on retry");
                continue;
            }
            say!("--- attempt {attempt}: erasing");
            if !(self.erase_and_wait() && self.sum(4096) == Some(BLANK_4K)) {
                say!("erase did not leave a blank flash");
                continue;
            }
            // The chip keeps bytes of a page that an earlier, aborted write left unfinished, and only
            // a short frame clears them. Send one blank byte, then erase again to reset the address.
            if !self.send_data(&data_frame(1, &[0xff])) {
                say!("flush frame not acknowledged");
                continue;
            }
            self.link.pause(PAGE_TIME);
            if !(self.erase_and_wait() && self.sum(4096) == Some(BLANK_4K) && self.sum(0x30000) == Some(0)) {
                say!("second erase did not leave a blank flash");
                continue;
            }
            say!("programming {} frames", frames.len());
            let start = Instant::now();
            let mut sent = 0;
            for frame in &frames {
                if !self.send_data(frame) {
                    break;
                }
                sent += 1;
                crate::progress("Writing", sent, frames.len());
                if usize::from(frame[1]) < FRAME_DATA {
                    self.link.pause(PAGE_TIME);
                }
                if sent % 500 == 0 {
                    say!("  {sent}/{} frames, {} s", frames.len(), start.elapsed().as_secs());
                }
            }
            if sent != frames.len() {
                say!("programming stopped at frame {} of {}", sent + 1, frames.len());
                continue;
            }
            say!("programmed in {} s; verifying (two to three minutes)", start.elapsed().as_secs());
            match self.holds(image) {
                Ok(()) => return true,
                Err(why) => say!("verification failed: {why}"),
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{IMAGE_LEN, PAGE};
    use std::collections::VecDeque;

    /// A model of the chip's handlers, from its firmware (data handler 0x3E21, erase 0x3309).
    #[derive(Default)]
    struct Sim {
        flash: Vec<u8>,
        buffer: Vec<u8>,
        pointer: usize,
        count: usize,
        writing: bool,
        address: usize,
        sequence: u16,
        busy_polls: u8,
        erases: u32,
        flash_open: bool,
        overflow: bool,
        replies: VecDeque<[u8; 64]>,
        // faults, each used once
        lose_ack: Option<u16>,
        fail_erases: u32,
        skip_page: Option<usize>,
        wrong_byte: Option<usize>,
    }

    impl Sim {
        fn holding(content: &[u8]) -> Self {
            let mut flash = content.to_vec();
            flash.resize(0x40000, 0xff);
            Sim { flash, buffer: vec![0; 1024], sequence: 1, ..Default::default() }
        }

        fn holds(&self, image: &[u8]) -> bool {
            self.flash[..image.len()] == *image && self.flash[image.len()..].iter().all(|&b| b == 0xff)
        }

        fn reply(code: u8, data: &[u8]) -> Option<[u8; 64]> {
            let mut r = [0u8; 64];
            r[..5].copy_from_slice(&[0x02, code, 0xfe, 0xff, code]);
            r[5..5 + data.len()].copy_from_slice(data);
            Some(r)
        }

        fn command(&mut self, f: &[u8; 64]) -> Option<[u8; 64]> {
            match f[4] {
                VERSION => Self::reply(VERSION, &VERSION_E103),
                OPEN => {
                    self.flash_open = true;
                    Self::reply(OPEN, &[0])
                }
                ERASE => {
                    // the handler resets these even when the erase itself fails
                    (self.address, self.sequence, self.writing) = (0, 1, true);
                    if self.fail_erases > 0 || !self.flash_open {
                        self.fail_erases = self.fail_erases.saturating_sub(1);
                        self.flash_open = false;
                        return Self::reply(ERASE, &[1]);
                    }
                    self.erases += 1;
                    self.flash.fill(0xff);
                    self.busy_polls = 3;
                    Self::reply(ERASE, &[0])
                }
                STATUS => {
                    let busy = self.busy_polls > 0;
                    self.busy_polls = self.busy_polls.saturating_sub(1);
                    Self::reply(STATUS, &[if busy { 0x03 } else { 0x00 }, 0x02])
                }
                SUM => {
                    let n = u32::from_be_bytes([f[5], f[6], f[7], f[8]]) as usize;
                    let sum: usize = self.flash[..n].iter().map(|&b| usize::from(b)).sum();
                    Self::reply(SUM, &((sum & 0xffff) as u16).to_be_bytes())
                }
                CLOSE => {
                    (self.writing, self.flash_open) = (false, false);
                    Self::reply(CLOSE, &[0])
                }
                _ => None,
            }
        }

        fn data(&mut self, f: &[u8; 64]) -> Option<[u8; 64]> {
            let n = usize::from(f[1]);
            let sequence = u16::from_le_bytes([f[2], f[3]]);
            if !self.writing || n > FRAME_DATA || sequence != self.sequence {
                return None;
            }
            for &byte in &f[4..4 + n] {
                self.overflow |= self.pointer >= PAGE;
                self.buffer[self.pointer] = byte;
                self.pointer += 1;
            }
            self.count += n;
            if n < FRAME_DATA {
                // page program: bits can only be cleared, and the address wraps inside the page
                self.pointer = 0;
                let page = self.address & !0xff;
                if self.skip_page == Some(page / PAGE) && self.count == PAGE {
                    self.skip_page = None;
                } else {
                    for i in 0..self.count {
                        self.flash[page | ((self.address + i) & 0xff)] &= self.buffer[i];
                    }
                    if let Some(at) = self.wrong_byte.filter(|at| (page..page + PAGE).contains(at)) {
                        self.flash[at] ^= 0x01;
                        self.wrong_byte = None;
                    }
                }
                self.address += self.count;
                self.count = 0;
            }
            self.sequence += 1;
            if self.lose_ack == Some(sequence) {
                self.lose_ack = None;
                return None; // frame taken, acknowledgement lost
            }
            let mut r = [0u8; 64];
            r[..4].copy_from_slice(&[0x02, ACK, f[2], f[3]]);
            Some(r)
        }
    }

    impl Transport for Sim {
        fn send(&mut self, f: &[u8; 64]) -> Result<(), String> {
            if f[0] == 0x02 && f[63] == 0x5a {
                let reply = if f[1..4] == [0x04, 0xfe, 0xff] { self.command(f) } else { self.data(f) };
                self.replies.extend(reply);
            }
            Ok(())
        }
        fn recv(&mut self, _timeout: Duration) -> Option<[u8; 64]> {
            self.replies.pop_front()
        }
        fn pause(&mut self, _time: Duration) {}
    }

    /// Same length as the real image, made-up content.
    fn old_image() -> Vec<u8> {
        let mut x = 0x2545_f491_u32;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        };
        (0..IMAGE_LEN).map(|_| next()).collect()
    }

    fn new_image() -> Vec<u8> {
        let mut image = old_image();
        for at in [0x2804, 0x2805, 0x2806, 0x2807, 0x28f9] {
            image[at] ^= 0x5a;
        }
        image
    }

    /// Writes the new image over a dock that holds the old one. Returns the tool's verdict, whether
    /// the modelled flash really holds the new image, and the number of chip erases.
    fn write_with(fault: impl FnOnce(&mut Sim)) -> (bool, bool, u32) {
        let mut dock = Dock::new(Sim::holding(&old_image()));
        fault(&mut dock.link);
        let verdict = dock.open() && dock.write(&new_image());
        let sim = dock.link;
        (verdict, sim.holds(&padded(&new_image())) && !sim.overflow, sim.erases)
    }

    #[test]
    fn write_should_verify_on_a_healthy_dock() {
        assert_eq!(write_with(|_| {}), (true, true, 2));
    }

    #[test]
    fn write_should_cope_with_a_half_filled_chip_buffer() {
        assert_eq!(write_with(|sim| (sim.pointer, sim.count) = (236, 236)), (true, true, 2));
    }

    #[test]
    fn write_should_start_over_when_an_acknowledgement_is_lost() {
        assert_eq!(write_with(|sim| sim.lose_ack = Some(700)), (true, true, 4));
    }

    #[test]
    fn write_should_start_over_when_the_last_acknowledgement_is_lost() {
        assert_eq!(write_with(|sim| sim.lose_ack = Some(3295)), (true, true, 4));
    }

    #[test]
    fn write_should_reopen_the_flash_after_a_failed_erase() {
        assert_eq!(write_with(|sim| sim.fail_erases = 1), (true, true, 2));
    }

    #[test]
    fn write_should_catch_a_skipped_first_page() {
        assert_eq!(write_with(|sim| sim.skip_page = Some(0)), (true, true, 4));
    }

    #[test]
    fn write_should_catch_a_skipped_middle_page() {
        assert_eq!(write_with(|sim| sim.skip_page = Some(300)), (true, true, 4));
    }

    #[test]
    fn write_should_catch_a_skipped_last_page() {
        assert_eq!(write_with(|sim| sim.skip_page = Some(658)), (true, true, 4));
    }

    #[test]
    fn write_should_catch_a_wrongly_programmed_byte() {
        assert_eq!(write_with(|sim| sim.wrong_byte = Some(0x2900)), (true, true, 4));
    }

    #[test]
    fn write_should_report_failure_and_erase_nothing_when_every_erase_fails() {
        assert_eq!(write_with(|sim| sim.fail_erases = 9), (false, false, 0));
    }

    #[test]
    fn holds_should_accept_the_content_the_flash_has() {
        let mut dock = Dock::new(Sim::holding(&old_image()));
        assert_eq!(dock.holds(&old_image()), Ok(()));
    }

    #[test]
    fn holds_should_reject_content_that_differs_in_five_bytes() {
        let mut dock = Dock::new(Sim::holding(&old_image()));
        assert!(dock.holds(&new_image()).is_err());
    }

    #[test]
    fn version_should_return_the_chip_firmware_version() {
        let mut dock = Dock::new(Sim::holding(&[]));
        assert_eq!(dock.version(), Some(VERSION_E103));
    }
}
