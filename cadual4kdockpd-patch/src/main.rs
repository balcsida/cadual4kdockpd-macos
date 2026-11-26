//! Switches Try.SRC off in the i-tec CADUAL4KDOCKPD dock's power controller, over USB.
//!
//! A command-line tool, and with `serve` the engine behind the native windows (see `serve.rs`).
//! Background and evidence: `dock-capture/REPORT.md`. Read this crate's README before using it:
//! a write that is interrupted by a power loss leaves the dock unusable.

use std::sync::{Mutex, MutexGuard, mpsc};

/// Set while serving a window: output then goes to the window as JSON lines (see `serve.rs`).
static EVENTS: Mutex<Option<mpsc::Sender<String>>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sends a line to the window, if there is one.
fn event(line: String) {
    if let Some(events) = &*lock(&EVENTS) {
        let _ = events.send(line);
    }
}

/// Prints a line, or sends it to the window. A failed write to the terminal must never stop a flash
/// write half way, so errors are ignored.
fn record(line: String) {
    use std::io::Write as _;
    if lock(&EVENTS).is_some() {
        event(format!("{{\"log\":{}}}", serve::quote(&line)));
    } else {
        let _ = writeln!(std::io::stdout(), "{line}");
    }
}

/// Tells the window how far a long step is, once per percent. An empty name means no step.
fn progress(step: &'static str, done: usize, of: usize) {
    static SHOWN: Mutex<(&str, usize)> = Mutex::new(("", 0));
    let now = (step, done * 100 / of.max(1));
    let mut shown = lock(&SHOWN);
    if *shown != now {
        *shown = now;
        event(format!("{{\"progress\":{{\"label\":{},\"fraction\":{}}}}}", serve::quote(step), now.1 as f64 / 100.0));
    }
}

macro_rules! say {
    ($($arg:tt)*) => { crate::record(format!($($arg)*)) };
}

mod dock;
mod hubs;
mod image;
mod serve;

use dock::{Dock, Transport, VERSION_E103};
use image::{IMAGE_LEN, Images, sum16};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const USAGE: &str = "\
usage:
  cadual4kdockpd-patch check   [IMAGE]    read only: what does the connected dock hold?
  cadual4kdockpd-patch patch   [IMAGE]    switch Try.SRC off
  cadual4kdockpd-patch restore [IMAGE]    write the original image back
  cadual4kdockpd-patch hubs check  [DIR]  read only: which firmware do the dock's two USB hubs hold?
  cadual4kdockpd-patch hubs update [DIR]  optional: update both hubs to i-tec's firmware 93.92 (not on Windows)

DIR is a folder holding Hub_1_9392_L1.ROM and Hub_1_9392_L2.ROM from i-tec's Patch_CADUAL4KDOCKPD.
IMAGE is Cable Matters' DUD15X0D1_Update_E108.exe, or a file holding only the power controller image from it.
Both are needed unless the firmware was built into this program.
Add --after-failed-write to patch or restore only when an earlier run ended with NOT VERIFIED.
Read the README first: losing the dock's power during a write leaves the dock unusable.
The app (CADUAL4KDOCKPD Patch) does the same in a window; it runs this program with `serve`.";

const RESUME: &str = "--after-failed-write";
const WARNING: &str = "\
This will ERASE the dock's power controller flash and write the new image.
  - The dock's own power supply must stay connected until this tool says WRITE VERIFIED.
    If the dock loses power in between, it will not start again without an external flash programmer.
  - Leave the USB-C cable in, keep this window open and keep the computer awake. It takes about 5 minutes.
  - Use a dock you can afford to lose. This has been done on very few units.";
const HUB_WARNING: &str = "\
This will rewrite the firmware of the dock's two USB hubs with i-tec's version 93.92.
  - Keep the dock's power supply and the USB-C cable connected until this tool reports the result.
  - Each hub restarts at the end, so everything plugged into the dock disconnects for a few seconds.
  - This step is optional. It does not affect the connect and disconnect loop on Macs.
  - This tool's hub update has been done on very few units. Use a dock you can afford to lose.";
const NOT_VERIFIED: &str = "NOT VERIFIED. DO NOT POWER-CYCLE THE DOCK. Keep its power supply connected and write again.";
const VERIFIED: &str = "WRITE VERIFIED. The dock loads the new image when it restarts: unplug the USB-C cable from the \
computer, unplug the dock's power supply for 15 seconds, then connect power first and the computer second.";

struct Hid(hidapi::HidDevice);

impl Transport for Hid {
    fn send(&mut self, frame: &[u8; 64]) -> Result<(), String> {
        self.0.write(frame).map(|_| ()).map_err(|e| e.to_string())
    }

    fn recv(&mut self, timeout: Duration) -> Option<[u8; 64]> {
        let mut report = [0u8; 64];
        let wait = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        // a report of any length counts as "something arrived"; a short one matches nothing and is logged
        matches!(self.0.read_timeout(&mut report, wait), Ok(n) if n > 0).then_some(report)
    }
}

const WAIT_FOR_DOCK: Duration = Duration::from_secs(300);
const WAITING: &str = "\
waiting for the dock (USB 065F:F60F), up to 5 minutes...
On a Mac an unpatched dock connects on about one plug-in in three. If nothing happens within 5 seconds
of plugging in, unplug the cable, wait 3 seconds and plug it in again. Once connected, it stays connected.";

/// Waits for the dock's management chip, opens it and reads its firmware version.
///
/// The tool cannot shorten the wait: while the dock loops there is no USB link to talk over.
fn connect() -> Result<(Dock<Hid>, [u8; 4]), String> {
    let mut api = hidapi::HidApi::new().map_err(|e| format!("cannot start USB HID access: {e}"))?;
    let start = Instant::now();
    let mut told = false;
    loop {
        let entries: Vec<_> = api.device_list().filter(|d| (d.vendor_id(), d.product_id()) == (0x065f, 0xf60f)).cloned().collect();
        for entry in &entries {
            // Windows may list one entry per report collection: take the one that answers
            let Ok(device) = entry.open_device(&api) else { continue };
            let mut dock = Dock::new(Hid(device));
            if let Some(version) = dock.version() {
                if entries.len() > 1 {
                    say!("note: {} matching HID entries, using the first that answers. Connect only one dock.", entries.len());
                }
                return Ok((dock, version));
            }
        }
        if start.elapsed() > WAIT_FOR_DOCK {
            let why = if entries.is_empty() { "dock not found: no USB device 065F:F60F appeared" } else { "the dock's management chip did not answer" };
            return Err(why.into());
        }
        if !told {
            told = true;
            say!("{WAITING}");
        }
        std::thread::sleep(Duration::from_millis(500));
        let _ = api.refresh_devices();
    }
}

/// The vendor's power controller image, when it was built into the program (feature `embedded-image`).
#[cfg(feature = "embedded-image")]
const EMBEDDED_IMAGE: Option<&[u8]> = Some(include_bytes!(env!("DOCK_IMAGE_FILE")));
#[cfg(not(feature = "embedded-image"))]
const EMBEDDED_IMAGE: Option<&[u8]> = None;

/// i-tec's two hub images, when they were built into the program.
#[cfg(feature = "embedded-image")]
const EMBEDDED_HUB_ROMS: Option<(&[u8], &[u8])> = Some((include_bytes!(env!("DOCK_HUB_L1_FILE")), include_bytes!(env!("DOCK_HUB_L2_FILE"))));
#[cfg(not(feature = "embedded-image"))]
const EMBEDDED_HUB_ROMS: Option<(&[u8], &[u8])> = None;

/// The two hub images, from the named folder or else the built-in ones.
fn hub_roms(dir: Option<&str>) -> Result<hubs::Roms, String> {
    match (dir, EMBEDDED_HUB_ROMS) {
        (Some(dir), _) => hubs::Roms::from_dir(std::path::Path::new(dir)),
        (None, Some((first, second))) => hubs::Roms::new(first, second),
        (None, None) => Err(format!("this build has no built-in hub firmware: give the folder holding {} and {}", hubs::FIRST_FILE, hubs::SECOND_FILE)),
    }
}

/// Both flash images, from the named file or else from the built-in image.
fn images(file: Option<&str>) -> Result<Images, String> {
    match (file, EMBEDDED_IMAGE) {
        (Some(path), _) => image::from_updater(&std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?),
        (None, Some(image)) => image::from_updater(image),
        (None, None) => Err("this build has no built-in firmware image: give the image file".into()),
    }
}

fn version_text(v: [u8; 4]) -> String {
    format!("{}{}{}{}", char::from(v[0]), v[1], v[2], v[3])
}

/// Runs `work` with the flash open and always releases it afterwards.
fn with_flash<R>(dock: &mut Dock<Hid>, work: impl FnOnce(&mut Dock<Hid>) -> Result<R, String>) -> Result<R, String> {
    if !dock.open() {
        dock.close();
        return Err("the chip could not open the power controller's flash. Nothing was changed.".into());
    }
    let result = work(dock);
    dock.close();
    progress("", 0, 0);
    result
}

/// Which of the two known images the dock holds, judged by the chip's checksum over the whole image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Holds {
    Original,
    Patched,
}

/// Read only and quick (a few seconds): finds the dock and tells which image it holds.
/// Refuses a dock this tool cannot patch.
fn identify() -> Result<Holds, String> {
    let (mut dock, version) = connect()?;
    if version != VERSION_E103 {
        return Err(format!("This dock's management chip runs firmware {}, not the tested E103. This tool does not support it.", version_text(version)));
    }
    with_flash(&mut dock, |dock| match dock.sum(IMAGE_LEN).ok_or("the dock did not answer with a checksum")? {
        image::ORIGINAL_SUM => Ok(Holds::Original),
        image::PATCHED_SUM => Ok(Holds::Patched),
        other => Err(format!("This dock holds firmware this tool does not know (checksum {other:04X}). It is not supported.")),
    })
}

/// Read only. Returns what the dock holds, in a sentence.
fn check(images: Option<&Images>) -> Result<String, String> {
    let (mut dock, version) = connect()?;
    say!("management chip firmware: {}{}", version_text(version), if version == VERSION_E103 { "" } else { " (not the tested version E103)" });
    with_flash(&mut dock, |dock| {
        say!("flash status: {}", dock.status().map_or("no answer".into(), |s| format!("{s:02X}")));
        let total = dock.sum(IMAGE_LEN).ok_or("no checksum came back")?;
        let (name, image) = match total {
            image::ORIGINAL_SUM => ("the original image: Try.SRC is on, the dock can be patched", images.map(|i| &i.original)),
            image::PATCHED_SUM => ("the patched image: Try.SRC is already off", images.map(|i| &i.patched)),
            _ => return Err(format!("unknown flash content (sum {total:04X}). This tool does not support this dock.")),
        };
        let Some(image) = image else {
            return Ok(format!("The checksum over the whole image matches {name}. Give the image file to compare block by block."));
        };
        say!("the checksum over the whole image matches {name}");
        say!("comparing every 4 KB block (two to three minutes)");
        dock.holds(image).map_err(|why| format!("but the content differs: {why}. This tool does not support this dock."))?;
        Ok(format!("Confirmed, all blocks match: the dock holds {name}."))
    })
}

/// Shows the warning and waits for the word to be typed.
fn typed(warning: &str, word: &str) -> bool {
    say!("\n{warning}\n\nType {word} and press Enter to go ahead, anything else to stop:");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok() && line.trim() == word
}

/// Replaces `from` with `to` on the connected dock, after checking that the dock is the tested kind
/// and holds exactly `from`. `confirm` is asked once, right before the erase.
fn rewrite(from: &[u8], to: &[u8], after_failed_write: bool, confirm: impl FnOnce() -> bool) -> Result<String, String> {
    let (mut dock, version) = connect()?;
    if version != VERSION_E103 {
        return Err(format!("management chip firmware is {}, not the tested E103. Nothing was changed.", version_text(version)));
    }
    let verified = with_flash(&mut dock, |dock| {
        if dock.status() != Some(0x00) {
            return Err("the flash is busy or write-protected. Nothing was changed.".to_string());
        }
        if !after_failed_write {
            let total = dock.sum(IMAGE_LEN).ok_or("no checksum came back. Nothing was changed.")?;
            if total == sum16(to, IMAGE_LEN) {
                return Err("the dock already holds this image. Nothing was changed.".into());
            }
            if total != sum16(from, IMAGE_LEN) {
                return Err(format!("unknown flash content (sum {total:04X}): this is not the dock this tool was made for. Nothing was changed."));
            }
            say!("checking that the dock holds exactly the expected image (two to three minutes)");
            dock.holds(from).map_err(|why| format!("the flash differs from the expected image: {why}. Nothing was changed."))?;
            say!("the dock holds exactly the expected image");
        }
        if !confirm() {
            return Err("stopped. Nothing was changed.".into());
        }
        guard::keep_awake();
        guard::ignore_interrupts();
        Ok(dock.write(to))
    })?;
    if verified { Ok(VERIFIED.into()) } else { Err(NOT_VERIFIED.into()) }
}

fn run(args: &[&str]) -> Result<String, String> {
    let after_failed_write = args.contains(&RESUME);
    let args: Vec<&str> = args.iter().copied().filter(|arg| *arg != RESUME).collect();
    let file = args.get(1).copied();
    match args[..] {
        // without a file and without a built-in image the check still compares the overall checksum
        ["check"] => check(images(None).ok().as_ref()),
        ["check", _] => check(Some(&images(file)?)),
        ["patch"] | ["patch", _] => {
            let images = images(file)?;
            rewrite(&images.original, &images.patched, after_failed_write, || typed(WARNING, "ERASE"))
        }
        ["restore"] | ["restore", _] => {
            let images = images(file)?;
            rewrite(&images.patched, &images.original, after_failed_write, || typed(WARNING, "ERASE"))
        }
        ["hubs", "check"] | ["hubs", "check", _] => hubs::check(&hub_roms(args.get(2).copied())?),
        ["hubs", "update"] | ["hubs", "update", _] => hubs::update(&hub_roms(args.get(2).copied())?, || typed(HUB_WARNING, "UPDATE")),
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if args == ["serve"] {
        return serve::run();
    }
    match run(&args) {
        Ok(done) => {
            say!("{done}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            say!("{why}");
            if why == NOT_VERIFIED {
                say!("Run the same command again with {RESUME} added.");
            }
            ExitCode::FAILURE
        }
    }
}

/// Keeps the operating system from interrupting a write.
mod guard {
    #[cfg(target_os = "macos")]
    pub fn keep_awake() {
        // ends by itself when this process ends
        let _ = std::process::Command::new("caffeinate").args(["-dims", "-w", &std::process::id().to_string()]).spawn();
    }

    #[cfg(windows)]
    pub fn keep_awake() {
        unsafe extern "system" {
            fn SetThreadExecutionState(flags: u32) -> u32;
        }
        // ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
        // SAFETY: a plain Win32 call with constant flags.
        unsafe { SetThreadExecutionState(0x8000_0003) };
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn keep_awake() {}

    #[cfg(unix)]
    pub fn ignore_interrupts() {
        unsafe extern "C" {
            fn signal(signum: i32, handler: usize) -> usize;
        }
        const SIG_IGN: usize = 1;
        // hang-up (closed terminal), Ctrl-C, quit, terminate
        for signum in [1, 2, 3, 15] {
            // SAFETY: installs the "ignore" disposition, which runs no handler code.
            unsafe { signal(signum, SIG_IGN) };
        }
    }

    #[cfg(windows)]
    pub fn ignore_interrupts() {
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(handler: usize, add: i32) -> i32;
        }
        // SAFETY: a null handler with add = TRUE makes the process ignore Ctrl-C.
        unsafe { SetConsoleCtrlHandler(0, 1) };
    }

    #[cfg(not(any(unix, windows)))]
    pub fn ignore_interrupts() {}
}
