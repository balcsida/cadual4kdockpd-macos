//! The engine behind the two native windows (SwiftUI on macOS, WinUI 3 on Windows).
//!
//! `serve` reads one command per line from standard input and writes one JSON object per line to
//! standard output. It owns the guided flow (connect, identify, apply the fix, restart, test), so the
//! windows only draw it. Everything that touches the dock runs on this one thread: while a job runs,
//! nothing else looks at the USB bus.
//!
//! Commands: `identify`, `patch WORD`, `restore WORD`, `hub-check`, `hub-update WORD`, `restarted`,
//! `skip-check on|off`, `image PATH`. WORD is what the user typed to confirm a write.
//!
//! Output: `{"state":{...}}` after every change, `{"progress":{"label":"Writing","fraction":0.45}}`
//! during long steps (an empty label means none), `{"log":"..."}` for the details view.

use crate::image::Images;
use crate::{Holds, NOT_VERIFIED, hubs, lock};
use std::io::Write as _;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

/// The power controller image on its own, or the vendor updater that contains it.
const IMAGE_FILES: [&str; 2] = ["tps65982-image-1.38.6.bin", "DUD15X0D1_Update_E108.exe"];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Job {
    Identify,
    Patch,
    Restore,
    HubCheck,
    HubUpdate,
}

/// The steps of the main flow, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Step {
    #[default]
    Connect,
    Identify,
    Patch,
    Restart,
    Test,
}

/// The outcome of an action, shown next to it.
type Note = Option<Result<String, String>>;

/// Where the user is in the flow. The windows show exactly this.
#[derive(Debug, Default)]
struct Flow {
    step: Step,
    /// The dock's management chip is on the USB bus.
    present: bool,
    running: Option<Job>,
    identified: Note,
    patched: Note,
    hub_note: Note,
    restore_note: Note,
    already_patched: bool,
    /// Restart step: the dock has been seen leaving the bus.
    unplugged: bool,
    /// Test step: how often the dock has come back.
    reconnects: u32,
    /// The window's form of `--after-failed-write`: a half-written flash would fail the content check.
    after_failed_write: bool,
    image_error: Option<String>,
    hub_roms_error: Option<String>,
}

impl Flow {
    /// The dock came, went or stayed. Only called while no job runs.
    fn dock(&mut self, present: bool) {
        let was_present = std::mem::replace(&mut self.present, present);
        match self.step {
            Step::Connect if present => self.step = Step::Identify,
            // unplugged before anything was written: start again from the top
            Step::Identify | Step::Patch if !present && !self.after_failed_write => {
                (self.step, self.identified, self.patched) = (Step::Connect, None, None);
            }
            Step::Restart if !present => self.unplugged = true,
            Step::Restart if self.unplugged => (self.step, self.reconnects) = (Step::Test, 1),
            Step::Test if present && !was_present => self.reconnects += 1,
            _ => {}
        }
    }

    fn identified(&mut self, found: Result<Holds, String>) {
        self.running = None;
        match found {
            Ok(Holds::Original) => {
                self.identified = Some(Ok("This dock holds the original firmware. It can be patched.".into()));
                self.step = Step::Patch;
            }
            Ok(Holds::Patched) => {
                self.identified = Some(Ok("The fix is already in this dock. Nothing needs to be written.".into()));
                (self.already_patched, self.step) = (true, Step::Test);
            }
            Err(why) => self.identified = Some(Err(why)),
        }
    }

    fn finished(&mut self, job: Job, result: Result<String, String>) {
        self.running = None;
        match (job, result) {
            (Job::Patch, Ok(_)) => {
                self.patched = Some(Ok("Written and verified.".into()));
                (self.after_failed_write, self.unplugged, self.step) = (false, false, Step::Restart);
            }
            (Job::Patch, Err(why)) => {
                // the content check would refuse a half-written flash, so the next try skips it
                self.after_failed_write = why == NOT_VERIFIED;
                self.patched = Some(Err(why));
            }
            (Job::Restore, result) => self.restore_note = Some(result),
            (Job::HubCheck | Job::HubUpdate, result) => self.hub_note = Some(result),
            (Job::Identify, _) => {}
        }
    }

    /// "I have restarted it": for when the window did not see the dock leave and come back.
    fn restarted(&mut self) {
        if self.step == Step::Restart {
            (self.step, self.reconnects) = (Step::Test, u32::from(self.present));
        }
    }

    fn json(&self) -> String {
        let step = match self.step {
            Step::Connect => "connect",
            Step::Identify => "identify",
            Step::Patch => "patch",
            Step::Restart => "restart",
            Step::Test => "test",
        };
        let running = match self.running {
            None => "null",
            Some(Job::Identify) => "\"identify\"",
            Some(Job::Patch) => "\"patch\"",
            Some(Job::Restore) => "\"restore\"",
            Some(Job::HubCheck) => "\"hubCheck\"",
            Some(Job::HubUpdate) => "\"hubUpdate\"",
        };
        let note = |note: &Note| match note {
            None => "null".to_string(),
            Some(Ok(text)) => format!("{{\"ok\":true,\"text\":{}}}", quote(text)),
            Some(Err(text)) => format!("{{\"ok\":false,\"text\":{}}}", quote(text)),
        };
        let text = |text: &Option<String>| text.as_deref().map_or("null".to_string(), quote);
        format!(
            "{{\"state\":{{\"step\":\"{step}\",\"present\":{},\"running\":{running},\"identified\":{},\"patched\":{},\
             \"hubNote\":{},\"restoreNote\":{},\"alreadyPatched\":{},\"unplugged\":{},\"reconnects\":{},\
             \"afterFailedWrite\":{},\"imageError\":{},\"hubRomsError\":{}}}}}",
            self.present,
            note(&self.identified),
            note(&self.patched),
            note(&self.hub_note),
            note(&self.restore_note),
            self.already_patched,
            self.unplugged,
            self.reconnects,
            self.after_failed_write,
            text(&self.image_error),
            text(&self.hub_roms_error),
        )
    }
}

/// `text` as a JSON string.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Looks for one of `names` next to the program (or next to the .app bundle that holds it), in the
/// working directory, in Downloads, and where the hub update script unpacks i-tec's patch.
fn find_file(names: &[&str]) -> Option<std::path::PathBuf> {
    let program = std::env::current_exe().ok();
    // the program's folder, then up to three levels above it: X.app/Contents/MacOS -> the folder holding X.app
    let around_program = program.iter().flat_map(|p| p.ancestors().skip(1).take(4)).map(|d| d.to_path_buf());
    let downloads = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| std::path::Path::new(&h).join("Downloads"));
    let unpacked = downloads.iter().map(|d| d.join("Patch_CADUAL4KDOCKPD_firmware").join("roms")).collect::<Vec<_>>();
    around_program
        .chain(std::env::current_dir().ok())
        .chain(downloads)
        .chain(unpacked)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|file| file.is_file())
}

/// The firmware the jobs need: built in, or found as files.
struct Files {
    images: Result<Images, String>,
    roms: Result<hubs::Roms, String>,
}

impl Files {
    fn find() -> Self {
        let images = if crate::EMBEDDED_IMAGE.is_some() {
            crate::images(None)
        } else {
            match find_file(&IMAGE_FILES) {
                Some(file) => crate::images(Some(&file.display().to_string())),
                None => Err(format!("Neither {} nor {} was found next to this program or in Downloads.", IMAGE_FILES[0], IMAGE_FILES[1])),
            }
        };
        let roms = if crate::EMBEDDED_HUB_ROMS.is_some() {
            crate::hub_roms(None)
        } else {
            match find_file(&[hubs::FIRST_FILE]) {
                Some(first) => hubs::Roms::from_dir(first.parent().unwrap_or(&first)),
                None => Err(format!("{} and {} were not found next to this program.", hubs::FIRST_FILE, hubs::SECOND_FILE)),
            }
        };
        Files { images, roms }
    }
}

/// Whether the dock's management chip is on the USB bus right now. Sends nothing to the dock.
fn dock_present() -> bool {
    hidapi::HidApi::new().is_ok_and(|usb| usb.device_list().any(|d| (d.vendor_id(), d.product_id()) == (0x065f, 0xf60f)))
}

/// Sends the state to the window if it changed.
fn show(flow: &Flow, shown: &mut String) {
    let now = flow.json();
    if now != *shown {
        crate::event(now.clone());
        *shown = now;
    }
}

/// Carries out one line from the window. A write runs only with the word the user typed for it.
fn obey(line: &str, flow: &mut Flow, files: &mut Files, shown: &mut String) {
    let (command, argument) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
    let job = match (command, argument.trim()) {
        ("identify", _) if flow.step == Step::Identify => Job::Identify,
        ("patch", word) if flow.step == Step::Patch && (flow.after_failed_write || word == "ERASE") => Job::Patch,
        ("restore", "ERASE") => Job::Restore,
        ("hub-check", _) => Job::HubCheck,
        ("hub-update", "UPDATE") => Job::HubUpdate,
        ("restarted", _) => return flow.restarted(),
        ("skip-check", on) => {
            flow.after_failed_write = on == "on";
            return;
        }
        ("image", path) => {
            files.images = crate::images(Some(path));
            flow.image_error = files.images.as_ref().err().cloned();
            return;
        }
        _ => return say!("ignored: {line}"),
    };
    flow.running = Some(job);
    show(flow, shown);
    // the window asked for the typed word and the match above checked it
    let confirmed = || true;
    let roms = files.roms.as_ref().map_err(String::clone);
    let images = files.images.as_ref().map_err(|why| format!("the firmware image is missing: {why}"));
    let again = flow.after_failed_write;
    match job {
        Job::Identify => flow.identified(crate::identify()),
        Job::HubCheck => flow.finished(job, roms.and_then(hubs::check)),
        Job::HubUpdate => flow.finished(job, roms.and_then(|roms| hubs::update(roms, confirmed))),
        Job::Patch => flow.finished(job, images.and_then(|i| crate::rewrite(&i.original, &i.patched, again, confirmed))),
        Job::Restore => flow.finished(job, images.and_then(|i| crate::rewrite(&i.patched, &i.original, again, confirmed))),
    }
}

pub fn run() -> ExitCode {
    // Output goes through a thread of its own: a window that has stopped reading must never be able
    // to hold up a write to the dock.
    let (events, lines) = mpsc::channel::<String>();
    *lock(&crate::EVENTS) = Some(events);
    let writer = std::thread::spawn(move || {
        for line in lines {
            let _ = writeln!(std::io::stdout(), "{line}");
        }
    });
    let (sender, commands) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                // a line that is not UTF-8 is dropped; the window stays connected
                Err(why) if why.kind() == std::io::ErrorKind::InvalidData => {}
                Err(_) => break,
            }
        }
    });

    let mut files = Files::find();
    let mut flow = Flow { image_error: files.images.as_ref().err().cloned(), hub_roms_error: files.roms.as_ref().err().cloned(), ..Flow::default() };
    let mut shown = String::new();
    loop {
        flow.dock(dock_present());
        show(&flow, &mut shown);
        match commands.recv_timeout(Duration::from_secs(1)) {
            Ok(line) => obey(&line, &mut flow, &mut files, &mut shown),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // the window is gone
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    *lock(&crate::EVENTS) = None;
    let _ = writer.join();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_patch() -> Flow {
        let mut flow = Flow::default();
        flow.dock(true);
        flow.identified(Ok(Holds::Original));
        flow
    }

    #[test]
    fn flow_should_walk_through_all_five_steps() {
        let mut flow = Flow::default();
        flow.dock(false);
        assert_eq!(flow.step, Step::Connect);
        flow.dock(true);
        assert_eq!(flow.step, Step::Identify);
        flow.identified(Ok(Holds::Original));
        assert_eq!(flow.step, Step::Patch);
        flow.running = Some(Job::Patch);
        flow.finished(Job::Patch, Ok(crate::VERIFIED.into()));
        assert_eq!((flow.step, flow.running, flow.unplugged), (Step::Restart, None, false));
        flow.dock(true);
        assert_eq!(flow.step, Step::Restart, "still plugged in: not restarted yet");
        flow.dock(false);
        assert!(flow.unplugged);
        flow.dock(true);
        assert_eq!((flow.step, flow.reconnects), (Step::Test, 1));
        flow.dock(true);
        flow.dock(false);
        flow.dock(true);
        assert_eq!(flow.reconnects, 2, "only a return counts, not staying connected");
    }

    #[test]
    fn flow_should_start_again_when_unplugged_before_a_write() {
        let mut flow = at_patch();
        flow.dock(false);
        assert_eq!(flow.step, Step::Connect);
        assert!(flow.identified.is_none());
    }

    #[test]
    fn flow_should_stay_on_the_patch_step_after_an_unverified_write() {
        let mut flow = at_patch();
        flow.finished(Job::Patch, Err(NOT_VERIFIED.into()));
        assert!(flow.after_failed_write);
        flow.dock(false);
        assert_eq!(flow.step, Step::Patch, "the dock must be written again, not identified again");

        let mut flow = at_patch();
        flow.finished(Job::Patch, Err("the flash is busy or write-protected. Nothing was changed.".into()));
        assert!(!flow.after_failed_write, "nothing was erased, so the content check stays");
    }

    #[test]
    fn flow_should_skip_to_the_test_when_the_dock_is_already_patched() {
        let mut flow = Flow::default();
        flow.dock(true);
        flow.identified(Ok(Holds::Patched));
        assert_eq!((flow.step, flow.already_patched), (Step::Test, true));
    }

    #[test]
    fn flow_should_keep_the_identify_step_when_the_dock_is_not_supported() {
        let mut flow = Flow::default();
        flow.dock(true);
        flow.identified(Err("not supported".into()));
        assert_eq!(flow.step, Step::Identify);
        assert_eq!(flow.identified, Some(Err("not supported".into())));
    }

    #[test]
    fn restarted_should_only_act_on_the_restart_step() {
        let mut flow = at_patch();
        flow.restarted();
        assert_eq!(flow.step, Step::Patch);
        flow.finished(Job::Patch, Ok(String::new()));
        flow.restarted();
        assert_eq!((flow.step, flow.reconnects), (Step::Test, 1));
    }

    #[test]
    fn obey_should_refuse_a_write_without_its_typed_word() {
        let mut files = Files { images: Err("none".into()), roms: Err("none".into()) };
        let mut shown = String::new();
        for line in ["patch", "patch erase", "restore", "restore ERASE!", "hub-update", "hub-update ERASE", "identify"] {
            let mut flow = at_patch();
            flow.step = if line == "identify" { Step::Test } else { Step::Patch };
            obey(line, &mut flow, &mut files, &mut shown);
            assert!(flow.patched.is_none() && flow.restore_note.is_none() && flow.hub_note.is_none(), "{line} must not start a job");
        }
        // with the word, the job starts (and stops here, at the missing image)
        let mut flow = at_patch();
        obey("patch ERASE", &mut flow, &mut files, &mut shown);
        assert_eq!(flow.patched, Some(Err("the firmware image is missing: none".into())));
    }

    #[test]
    fn json_should_be_one_line_with_escaped_text() {
        assert_eq!(quote("a \"b\"\\\n\tc"), "\"a \\\"b\\\"\\\\\\n\\u0009c\"");
        let mut flow = at_patch();
        flow.hub_note = Some(Err("line 1\nline 2".into()));
        flow.image_error = Some("no \"file\"".into());
        assert_eq!(
            flow.json(),
            "{\"state\":{\"step\":\"patch\",\"present\":true,\"running\":null,\
             \"identified\":{\"ok\":true,\"text\":\"This dock holds the original firmware. It can be patched.\"},\
             \"patched\":null,\"hubNote\":{\"ok\":false,\"text\":\"line 1\\nline 2\"},\"restoreNote\":null,\
             \"alreadyPatched\":false,\"unplugged\":false,\"reconnects\":0,\"afterFailedWrite\":false,\
             \"imageError\":\"no \\\"file\\\"\",\"hubRomsError\":null}}"
        );
    }

    /// Writes what the engine sends in each situation, one file per situation, for looking at the
    /// windows without a dock: `DOCK_DEMO_DIR=/some/folder cargo test -- --ignored demo`.
    #[test]
    #[ignore = "needs DOCK_DEMO_DIR"]
    fn demo_states_for_the_windows() {
        let Ok(dir) = std::env::var("DOCK_DEMO_DIR") else { return };
        let save = |name: &str, lines: &[String]| {
            let written = std::fs::write(std::path::Path::new(&dir).join(format!("{name}.jsonl")), lines.join("\n") + "\n");
            assert!(written.is_ok(), "cannot write to {dir}");
        };
        let progress = |label: &str, percent: u32| format!("{{\"progress\":{{\"label\":{},\"fraction\":{}}}}}", quote(label), f64::from(percent) / 100.0);
        let log = |text: &str| format!("{{\"log\":{}}}", quote(text));

        let mut flow = Flow::default();
        save("connect", &[flow.json()]);
        flow.dock(true);
        save("identify", &[flow.json()]);
        flow.identified(Err("This dock's management chip runs firmware E108, not the tested E103. This tool does not support it.".into()));
        save("unsupported", &[flow.json()]);
        flow.identified(Ok(Holds::Original));
        save("patch", &[flow.json()]);
        flow.running = Some(Job::Patch);
        save("writing", &[flow.json(), log("--- attempt 1: erasing"), log("erase finished after 3.4 s"), log("programming 3295 frames"), progress("Writing", 45)]);
        flow.finished(Job::Patch, Err(NOT_VERIFIED.into()));
        save("failed", &[flow.json()]);
        flow.finished(Job::Patch, Ok(crate::VERIFIED.into()));
        flow.dock(false);
        save("restart", &[flow.json()]);
        for present in [true, false, true, false, true] {
            flow.dock(present);
        }
        save("test", &[flow.json()]);
        flow.hub_note = Some(Ok("First hub: firmware 9305. Second hub: firmware 9305. Both can be updated to 93.92.".into()));
        flow.restore_note = Some(Err("the dock already holds this image. Nothing was changed.".into()));
        save("sections", &[flow.json(), log("management chip firmware: E103")]);
    }
}
