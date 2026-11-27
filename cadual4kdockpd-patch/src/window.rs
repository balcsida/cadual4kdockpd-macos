//! The Windows app's window: the guided flow of `serve.rs`, drawn with plain Win32 controls.
//!
//! The window starts this same program again as `serve` and talks to it over pipes, like the Mac
//! window (`macos/Engine.swift`). A write therefore runs to its end even if the window fails, and
//! nothing here talks to USB. Optional and rarely needed actions are in windows of their own, opened
//! from the "Dock" menu. Every write is confirmed by typing a word; the engine checks it again.
//!
//! Safety: every `unsafe` block here is a Win32 call made on the window's thread, on windows and GDI
//! objects this module created, with buffers that outlive the call.

use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{ChildStdin, Command, ExitCode, Stdio};
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemServices::{SS_EDITCONTROL, SS_LEFT, SS_NOPREFIX, SS_OWNERDRAW};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ---- What the engine reports -------------------------------------------------------------------

/// The outcome of an action, shown next to it.
#[derive(Clone, Debug, PartialEq)]
struct Note {
    ok: bool,
    text: String,
}

/// Where the user is in the flow, as the engine reports it after every change.
#[derive(Clone, Debug, Default)]
struct DockState {
    /// 1 to 5: connect, identify, patch, restart, test. 0 before the engine's first report.
    step: usize,
    present: bool,
    running: Option<String>,
    identified: Option<Note>,
    patched: Option<Note>,
    restore_note: Option<Note>,
    already_patched: bool,
    unplugged: bool,
    reconnects: u64,
    after_failed_write: bool,
    image_error: Option<String>,
}

impl DockState {
    fn from_json(state: &Value) -> Self {
        let note = |key: &str| {
            let note = state[key].as_object()?;
            Some(Note { ok: note.get("ok")?.as_bool()?, text: note.get("text")?.as_str()?.to_string() })
        };
        let text = |key: &str| state[key].as_str().map(String::from);
        let flag = |key: &str| state[key].as_bool().unwrap_or(false);
        let steps = ["connect", "identify", "patch", "restart", "test"];
        DockState {
            step: steps.iter().position(|step| state["step"].as_str() == Some(step)).map_or(0, |i| i + 1),
            present: flag("present"),
            running: text("running"),
            identified: note("identified"),
            patched: note("patched"),
            restore_note: note("restoreNote"),
            already_patched: flag("alreadyPatched"),
            unplugged: flag("unplugged"),
            reconnects: state["reconnects"].as_u64().unwrap_or(0),
            after_failed_write: flag("afterFailedWrite"),
            image_error: text("imageError"),
        }
    }

    fn idle(&self) -> bool {
        self.running.is_none()
    }

    fn runs(&self, job: &str) -> bool {
        self.running.as_deref() == Some(job)
    }

    /// A job that changes the dock: the app must stay open until it ends.
    fn writing(&self) -> bool {
        matches!(self.running.as_deref(), Some("patch" | "restore" | "hubUpdate"))
    }
}

/// Everything the engine has said.
#[derive(Default)]
struct Model {
    state: DockState,
    /// The long step's name and how far it is. An empty name means no step is being measured.
    progress: (String, f64),
    log: Vec<String>,
    /// Why the engine is not running, when it is not.
    stopped: Option<String>,
}

impl Model {
    /// Takes one line from the engine: `{"state":...}`, `{"progress":...}` or `{"log":...}`.
    fn take(&mut self, line: &str) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return self.log.push(line.to_string());
        };
        if let Some(state) = event.get("state") {
            self.state = DockState::from_json(state);
        }
        if let Some(progress) = event.get("progress") {
            self.progress = (progress["label"].as_str().unwrap_or("").to_string(), progress["fraction"].as_f64().unwrap_or(0.0));
        }
        if let Some(text) = event["log"].as_str() {
            self.log.push(text.to_string());
        }
    }
}

// ---- Texts and looks -----------------------------------------------------------------------------

const TITLE: &str = "CADUAL4KDOCKPD Patch";
/// Where a failed start is saved, so it can be sent on.
const ERROR_FILE: &str = "CADUAL4KDOCKPD-Patch-error.txt";
const UPDATER_PAGE: &str = "https://kb.cablematters.com/index.php?View=entry&EntryID=129";
const STEP_TITLES: [&str; 5] = ["Connect the dock", "Identify the dock", "Apply the fix", "Restart the dock", "Test it"];
const PATCH_WARNING: &str = "\u{2022}  It takes about 8 minutes. Nothing is erased unless the dock holds exactly the expected firmware.\n\
     \u{2022}  The dock's power supply must stay connected until this says verified. If the dock loses power in between, it will not start again.\n\
     \u{2022}  Leave the cable in. This app cannot be closed while it runs.";
const RESTORE_WARNING: &str = "\u{2022}  The dock's power supply must stay connected until this says verified.\n\
     \u{2022}  Restart the dock afterwards: cable out, power off for 15 seconds, power on, cable in.";
const HUB_TEXT: &str = "i-tec's update for the dock's two USB hubs does not affect the loop on Macs. On Windows, use \
     i-tec's own program for it, CADUAL4KDOCKPD_9392.exe: Windows needs the driver that program installs to reach a hub. \
     Run it with the dock powered and nothing plugged into the dock, press Update FW, then restart the dock.";

/// Space between two items, in device-independent pixels; the indent of a step's contents.
const GAP: i32 = 10;
const INDENT: i32 = 40;

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
const TEXT: COLORREF = rgb(0x1a, 0x1a, 0x1a);
// 6.3:1 on white
const SECONDARY: COLORREF = rgb(0x5f, 0x5f, 0x5f);
// white on these two has a contrast of at least 4.5:1
const ACCENT: COLORREF = rgb(0x1f, 0x6f, 0xeb);
const DONE: COLORREF = rgb(0x2e, 0x7d, 0x32);
const UPCOMING: COLORREF = rgb(0x80, 0x80, 0x80);
const WHITE: COLORREF = rgb(0xff, 0xff, 0xff);

/// Fonts, by what they are for. The icon fonts are Segoe MDL2 Assets, on every Windows 10 and 11.
#[derive(Clone, Copy, PartialEq)]
enum Face {
    Body,
    Bold,
    Title,
    Step,
    StepBold,
    Mono,
    Icon,
    Circle,
    Mark,
}
type Fonts = [HFONT; 9];

fn make_fonts(dpi: u32) -> Fonts {
    let font = |px: i32, weight: i32, name: &str| {
        let name = wide(name);
        unsafe {
            CreateFontW(-scale(px, dpi), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32, FF_DONTCARE as u32, name.as_ptr())
        }
    };
    let icons = "Segoe MDL2 Assets";
    [font(14, 400, "Segoe UI"), font(14, 600, "Segoe UI"), font(28, 600, "Segoe UI"), font(18, 400, "Segoe UI"), font(18, 600, "Segoe UI"), font(13, 400, "Consolas"), font(16, 400, icons), font(28, 400, icons), font(14, 400, icons)]
}

/// How a message box looks. Each kind has its own icon as well as its own colour.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tone {
    Warning,
    Danger,
    Success,
}

impl Tone {
    /// Fill, icon colour and icon, as Windows' own message bars.
    fn look(self) -> (COLORREF, COLORREF, &'static str) {
        match self {
            Tone::Warning => (rgb(0xff, 0xf4, 0xce), rgb(0x9d, 0x5d, 0x00), "\u{E7BA}"),
            Tone::Danger => (rgb(0xfd, 0xe7, 0xe9), rgb(0xc4, 0x2b, 0x1c), "\u{EA39}"),
            Tone::Success => (rgb(0xdf, 0xf6, 0xdd), rgb(0x0f, 0x7b, 0x0f), "\u{E930}"),
        }
    }
}

/// What an owner-drawn control shows.
#[derive(Clone, Debug, PartialEq)]
enum Drawn {
    /// A step's number in a circle: a tick when done, filled when current.
    Badge { number: usize, done: bool, current: bool },
    /// A tinted box for a message that must not be missed.
    Callout { tone: Tone, title: String, message: String },
}

// ---- The windows and their controls --------------------------------------------------------------

const MAIN: usize = 0;
const HUBS: usize = 1;
const MORE: usize = 2;

const ID_IDENTIFY: u16 = 101;
const ID_WRITE_AGAIN: u16 = 102;
const ID_PATCH: u16 = 103;
const ID_PATCH_WORD: u16 = 104;
const ID_RESTARTED: u16 = 105;
const ID_DETAILS: u16 = 106;
const ID_MENU_HUBS: u16 = 110;
const ID_MENU_MORE: u16 = 111;
const ID_RESTORE_ASK: u16 = 120;
const ID_RESTORE_WORD: u16 = 121;
const ID_RESTORE_GO: u16 = 122;
const ID_RESTORE_CANCEL: u16 = 123;
const ID_SKIP_CHECK: u16 = 124;
const ID_IMAGE_PATH: u16 = 125;
const ID_USE_IMAGE: u16 = 126;
const ID_UPDATER_PAGE: u16 = 127;

/// Posted to the main window when the engine has sent lines.
const WM_ENGINE: u32 = WM_APP;

#[derive(Clone, Copy)]
struct MainControls {
    title: HWND,
    subtitle: HWND,
    stopped: HWND,
    close_refused: HWND,
    badges: [HWND; 5],
    titles: [HWND; 5],
    summaries: [HWND; 4],
    connect_text: HWND,
    connect_bar: HWND,
    connect_busy: HWND,
    connect_hint: HWND,
    identify_text: HWND,
    identify_button: HWND,
    identify_bar: HWND,
    identify_busy: HWND,
    identify_note: HWND,
    writing: HWND,
    patch_progress: HWND,
    patch_bar: HWND,
    failed: HWND,
    write_again: HWND,
    missing: HWND,
    confirm: HWND,
    patch_label: HWND,
    patch_word: HWND,
    patch_button: HWND,
    patch_note: HWND,
    restart: HWND,
    restart_lines: [HWND; 3],
    restart_bar: HWND,
    restart_busy: HWND,
    restarted: HWND,
    test_text: HWND,
    present: HWND,
    test_done: HWND,
    test_count: HWND,
    details: HWND,
    log: HWND,
}

#[derive(Clone, Copy)]
struct MoreControls {
    text: HWND,
    restore: HWND,
    confirm: HWND,
    label: HWND,
    word: HWND,
    go: HWND,
    cancel: HWND,
    progress: HWND,
    bar: HWND,
    note: HWND,
    skip_check: HWND,
    missing: HWND,
    how: HWND,
    path: HWND,
    use_image: HWND,
    updater_page: HWND,
}

/// One of the three windows: the flow, the hub firmware and more options.
struct Page {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    scroll: i32,
    /// Every control on the page, with its font.
    children: Vec<(HWND, Face)>,
    /// Has been shown, so it has its place on the screen.
    placed: bool,
}

impl Page {
    fn new(hwnd: HWND) -> Self {
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        Page { hwnd, dpi, fonts: make_fonts(dpi), scroll: 0, children: Vec::new(), placed: false }
    }
}

struct App {
    model: Model,
    pages: Vec<Page>,
    main: MainControls,
    hub_text: HWND,
    more: MoreControls,
    drawn: HashMap<isize, Drawn>,
    colours: HashMap<isize, COLORREF>,
    close_refused: bool,
    restore_asked: bool,
    more_opened: bool,
    details: bool,
    /// Lines of the log already in its box.
    log_shown: usize,
    engine: Option<ChildStdin>,
}

/// What the pages are drawn from: a copy, so no borrow is held while Win32 calls back.
struct View {
    state: DockState,
    progress: (String, f64),
    stopped: Option<String>,
    close_refused: bool,
    restore_asked: bool,
    details: bool,
    main: MainControls,
    hub_text: HWND,
    more: MoreControls,
}

impl App {
    fn view(&self) -> View {
        View {
            state: self.model.state.clone(),
            progress: self.model.progress.clone(),
            stopped: self.model.stopped.clone(),
            close_refused: self.close_refused,
            restore_asked: self.restore_asked,
            details: self.details,
            main: self.main,
            hub_text: self.hub_text,
            more: self.more,
        }
    }
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// A refresh is running: a nested one, from a message it caused, only asks for another round.
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static AGAIN: Cell<bool> = const { Cell::new(false) };
}

/// The engine's lines, from its reader thread to the window. `None`: the engine has stopped.
static LINES: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Runs `f` on the app's data. No Win32 call may be made inside: a message it sends back to the
/// window would find the data borrowed.
fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|app| app.try_borrow_mut().ok()?.as_mut().map(f))
}

// ---- Small helpers -------------------------------------------------------------------------------

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn scale(dip: i32, dpi: u32) -> i32 {
    dip * dpi as i32 / 96
}

fn last_error() -> String {
    std::io::Error::last_os_error().to_string()
}

fn window_text(hwnd: HWND) -> String {
    unsafe {
        let mut buffer = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let got = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..got.max(0) as usize])
    }
}

/// Sets a control's text, if it changed: an unchanged text is not redrawn.
fn set_text(hwnd: HWND, text: &str) {
    if window_text(hwnd) != text {
        unsafe { SetWindowTextW(hwnd, wide(text).as_ptr()) };
    }
}

fn enable(hwnd: HWND, on: bool) {
    unsafe { EnableWindow(hwnd, i32::from(on)) };
}

/// A text colour other than the usual, or `None` for the usual.
fn colour(hwnd: HWND, colour: Option<COLORREF>) {
    let changed = with(|app| match colour {
        Some(colour) => app.colours.insert(hwnd as isize, colour) != Some(colour),
        None => app.colours.remove(&(hwnd as isize)).is_some(),
    });
    if changed == Some(true) {
        unsafe { InvalidateRect(hwnd, null(), 1) };
    }
}

/// What an owner-drawn control shows; redrawn when it changes.
fn mark(hwnd: HWND, drawn: Drawn) {
    let changed = with(|app| app.drawn.insert(hwnd as isize, drawn.clone()) != Some(drawn));
    if changed == Some(true) {
        unsafe { InvalidateRect(hwnd, null(), 1) };
    }
}

/// Switches a progress bar between moving back and forth and showing how far.
fn marquee(bar: HWND, on: bool) {
    unsafe {
        let style = GetWindowLongPtrW(bar, GWL_STYLE);
        if (style & PBS_MARQUEE as isize != 0) != on {
            SetWindowLongPtrW(bar, GWL_STYLE, if on { style | PBS_MARQUEE as isize } else { style & !(PBS_MARQUEE as isize) });
            SendMessageW(bar, PBM_SETMARQUEE, usize::from(on), 30);
        }
    }
}

const WRAP: u32 = DT_WORDBREAK | DT_NOPREFIX | DT_EDITCONTROL;
const CENTRED: u32 = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;

/// Height of `text` in `font`, wrapped at `width`, as a static control draws it.
fn measure(dc: HDC, font: HFONT, text: &str, width: i32) -> i32 {
    let mut text: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT { left: 0, top: 0, right: width.max(1), bottom: 0 };
    unsafe {
        SelectObject(dc, font);
        DrawTextW(dc, text.as_mut_ptr(), text.len() as i32, &mut rect, DT_CALCRECT | WRAP);
    }
    rect.bottom
}

fn draw(dc: HDC, font: HFONT, text: &str, colour: COLORREF, mut rect: RECT, format: u32) {
    let mut text: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        SelectObject(dc, font);
        SetTextColor(dc, colour);
        DrawTextW(dc, text.as_mut_ptr(), text.len() as i32, &mut rect, format);
    }
}

fn callout_height(dc: HDC, fonts: &Fonts, dpi: u32, title: &str, message: &str, width: i32) -> i32 {
    let pad = scale(12, dpi);
    let inner = width - pad * 2 - scale(28, dpi);
    let mut height = pad * 2 + measure(dc, fonts[Face::Bold as usize], title, inner);
    if !message.is_empty() {
        height += scale(4, dpi) + measure(dc, fonts[Face::Body as usize], message, inner);
    }
    height
}

// ---- Creating the controls -----------------------------------------------------------------------

fn control(page: &mut Page, class: &str, style: u32, ex_style: u32, id: u16, face: Face) -> HWND {
    let class = wide(class);
    let hwnd = unsafe {
        CreateWindowExW(ex_style, class.as_ptr(), wide("").as_ptr(), WS_CHILD | WS_CLIPSIBLINGS | style, 0, 0, 0, 0, page.hwnd, usize::from(id) as HMENU, GetModuleHandleW(null()), null())
    };
    unsafe { SendMessageW(hwnd, WM_SETFONT, page.fonts[face as usize] as WPARAM, 0) };
    page.children.push((hwnd, face));
    hwnd
}

fn label(page: &mut Page, face: Face) -> HWND {
    control(page, "STATIC", SS_LEFT | SS_NOPREFIX | SS_EDITCONTROL, 0, 0, face)
}

fn drawn(page: &mut Page) -> HWND {
    control(page, "STATIC", SS_OWNERDRAW, 0, 0, Face::Body)
}

fn button(page: &mut Page, id: u16, main_action: bool) -> HWND {
    let kind = if main_action { BS_DEFPUSHBUTTON } else { BS_PUSHBUTTON };
    control(page, "BUTTON", WS_TABSTOP | kind as u32, 0, id, Face::Body)
}

fn field(page: &mut Page, id: u16) -> HWND {
    control(page, "EDIT", WS_TABSTOP | ES_AUTOHSCROLL as u32, WS_EX_CLIENTEDGE, id, Face::Body)
}

fn bar(page: &mut Page) -> HWND {
    let hwnd = control(page, "msctls_progress32", 0, 0, 0, Face::Body);
    unsafe { SendMessageW(hwnd, PBM_SETRANGE32, 0, 100) };
    hwnd
}

fn build_main(p: &mut Page) -> MainControls {
    MainControls {
        title: label(p, Face::Title),
        subtitle: label(p, Face::Body),
        stopped: drawn(p),
        close_refused: drawn(p),
        badges: [(); 5].map(|()| drawn(p)),
        titles: [(); 5].map(|()| label(p, Face::Step)),
        summaries: [(); 4].map(|()| label(p, Face::Body)),
        connect_text: label(p, Face::Body),
        connect_bar: bar(p),
        connect_busy: label(p, Face::Body),
        connect_hint: label(p, Face::Body),
        identify_text: label(p, Face::Body),
        identify_button: button(p, ID_IDENTIFY, true),
        identify_bar: bar(p),
        identify_busy: label(p, Face::Body),
        identify_note: drawn(p),
        writing: drawn(p),
        patch_progress: label(p, Face::Body),
        patch_bar: bar(p),
        failed: drawn(p),
        write_again: button(p, ID_WRITE_AGAIN, true),
        missing: drawn(p),
        confirm: drawn(p),
        // a field takes its name for screen readers from the label made just before it
        patch_label: label(p, Face::Body),
        patch_word: field(p, ID_PATCH_WORD),
        patch_button: button(p, ID_PATCH, true),
        patch_note: drawn(p),
        restart: drawn(p),
        restart_lines: [(); 3].map(|()| label(p, Face::Body)),
        restart_bar: bar(p),
        restart_busy: label(p, Face::Body),
        restarted: button(p, ID_RESTARTED, false),
        test_text: label(p, Face::Body),
        present: label(p, Face::Bold),
        test_done: drawn(p),
        test_count: label(p, Face::Body),
        details: button(p, ID_DETAILS, false),
        log: {
            let style = WS_TABSTOP | WS_VSCROLL | (ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32;
            let log = control(p, "EDIT", style, WS_EX_CLIENTEDGE, 0, Face::Mono);
            unsafe {
                SendMessageW(log, EM_SETLIMITTEXT, 0, 0);
                SetWindowTextW(log, wide("Nothing has been done yet.").as_ptr());
            }
            log
        },
    }
}

fn build_more(p: &mut Page) -> MoreControls {
    MoreControls {
        text: label(p, Face::Body),
        restore: button(p, ID_RESTORE_ASK, false),
        confirm: drawn(p),
        label: label(p, Face::Body),
        word: field(p, ID_RESTORE_WORD),
        go: button(p, ID_RESTORE_GO, true),
        cancel: button(p, ID_RESTORE_CANCEL, false),
        progress: label(p, Face::Body),
        bar: bar(p),
        note: drawn(p),
        skip_check: control(p, "BUTTON", WS_TABSTOP | (BS_AUTOCHECKBOX | BS_MULTILINE) as u32, 0, ID_SKIP_CHECK, Face::Body),
        missing: drawn(p),
        how: label(p, Face::Body),
        path: {
            let path = field(p, ID_IMAGE_PATH);
            unsafe { SendMessageW(path, EM_SETCUEBANNER, 0, wide("Path of the file").as_ptr() as LPARAM) };
            path
        },
        use_image: button(p, ID_USE_IMAGE, false),
        updater_page: button(p, ID_UPDATER_PAGE, false),
    }
}

fn menu() -> HMENU {
    unsafe {
        let bar = CreateMenu();
        let dock = CreatePopupMenu();
        AppendMenuW(dock, MF_STRING, usize::from(ID_MENU_HUBS), wide("&Hub firmware 93.92...").as_ptr());
        AppendMenuW(dock, MF_STRING, usize::from(ID_MENU_MORE), wide("&More options...").as_ptr());
        AppendMenuW(bar, MF_POPUP, dock as usize, wide("&Dock").as_ptr());
        bar
    }
}

// ---- Laying out and drawing the pages ------------------------------------------------------------

/// Places a page's controls from the top down. Whatever is not placed is hidden.
struct Layout {
    page: usize,
    dc: HDC,
    dpi: u32,
    fonts: Fonts,
    left: i32,
    width: i32,
    y: i32,
    scroll: i32,
    shown: HashSet<isize>,
}

impl Layout {
    fn px(&self, dip: i32) -> i32 {
        scale(dip, self.dpi)
    }

    fn space(&mut self, dip: i32) {
        self.y += self.px(dip);
    }

    fn put(&mut self, hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
        unsafe { SetWindowPos(hwnd, null_mut(), x, y - self.scroll, width, height, SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW) };
        self.shown.insert(hwnd as isize);
    }

    fn font_of(&self, hwnd: HWND) -> HFONT {
        unsafe { SendMessageW(hwnd, WM_GETFONT, 0, 0) as HFONT }
    }

    fn line_width(&self, hwnd: HWND, text: &str) -> i32 {
        let text: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE { cx: 0, cy: 0 };
        unsafe {
            SelectObject(self.dc, self.font_of(hwnd));
            GetTextExtentPoint32W(self.dc, text.as_ptr(), text.len() as i32, &mut size);
        }
        size.cx
    }

    /// A block of text, wrapped, `indent` from the left.
    fn text(&mut self, hwnd: HWND, indent: i32, text: &str) {
        set_text(hwnd, text);
        let width = self.width - self.px(indent);
        let height = measure(self.dc, self.font_of(hwnd), text, width);
        self.put(hwnd, self.left + self.px(indent), self.y, width, height);
        self.y += height + self.px(GAP);
    }

    fn callout(&mut self, hwnd: HWND, indent: i32, tone: Tone, title: &str, message: &str) {
        // what a screen reader reads: the box itself is drawn in `paint`
        set_text(hwnd, format!("{title}\n{message}").trim_end());
        mark(hwnd, Drawn::Callout { tone, title: title.to_string(), message: message.to_string() });
        let width = self.width - self.px(indent);
        let height = callout_height(self.dc, &self.fonts, self.dpi, title, message, width);
        self.put(hwnd, self.left + self.px(indent), self.y, width, height);
        self.y += height + self.px(GAP);
    }

    /// Controls side by side, centred on one line. A width of 0 takes what is left.
    fn row(&mut self, indent: i32, items: &[(HWND, i32, i32)]) {
        let gap = self.px(8);
        let used: i32 = items.iter().map(|&(_, width, _)| width + gap).sum::<i32>() - gap;
        let rest = (self.width - self.px(indent) - used).max(self.px(80));
        let height = items.iter().map(|&(_, _, height)| height).max().unwrap_or(0);
        let mut x = self.left + self.px(indent);
        for &(hwnd, width, item_height) in items {
            let width = if width == 0 { rest } else { width };
            self.put(hwnd, x, self.y + (height - item_height) / 2, width, item_height);
            x += width + gap;
        }
        self.y += height + self.px(GAP);
    }

    /// A button with this text, sized to fit it.
    fn button(&self, hwnd: HWND, text: &str) -> (HWND, i32, i32) {
        set_text(hwnd, text);
        ((hwnd), (self.line_width(hwnd, text) + self.px(32)).max(self.px(100)), self.px(32))
    }

    /// A one-line label, sized to fit it.
    fn label(&self, hwnd: HWND, text: &str) -> (HWND, i32, i32) {
        set_text(hwnd, text);
        (hwnd, self.line_width(hwnd, text) + self.px(4), measure(self.dc, self.font_of(hwnd), text, i32::MAX / 4))
    }

    fn field(&self, hwnd: HWND, width: i32) -> (HWND, i32, i32) {
        (hwnd, width, self.px(30))
    }

    fn busy(&mut self, bar: HWND, text: HWND, indent: i32, what: &str) {
        marquee(bar, true);
        let label = self.label(text, what);
        self.row(indent, &[(bar, self.px(80), self.px(6)), label]);
    }

    fn progress(&mut self, text: HWND, bar: HWND, indent: i32, (label, fraction): &(String, f64)) {
        let percent = (fraction * 100.0).round().clamp(0.0, 100.0) as usize;
        self.text(text, indent, &if label.is_empty() { "Working...".to_string() } else { format!("{label}: {percent}%") });
        marquee(bar, label.is_empty());
        unsafe { SendMessageW(bar, PBM_SETPOS, percent, 0) };
        let height = self.px(8);
        self.put(bar, self.left + self.px(indent), self.y, self.width - self.px(indent), height);
        self.y += height + self.px(GAP);
    }

    /// The outcome of an action, next to the action it belongs to.
    fn note(&mut self, hwnd: HWND, indent: i32, note: &Option<Note>) {
        match note {
            Some(note) if !note.ok => self.callout(hwnd, indent, Tone::Danger, "That did not work", &note.text),
            Some(note) if !note.text.is_empty() => self.callout(hwnd, indent, Tone::Success, &note.text, ""),
            _ => {}
        }
    }

    /// Changes a control's font, e.g. bold for the current step's title.
    fn face(&self, hwnd: HWND, face: Face) {
        let page = self.page;
        let changed = with(|app| {
            let entry = app.pages[page].children.iter_mut().find(|(child, _)| *child == hwnd)?;
            Some(std::mem::replace(&mut entry.1, face) != face)
        });
        if changed == Some(Some(true)) {
            unsafe { SendMessageW(hwnd, WM_SETFONT, self.fonts[face as usize] as WPARAM, 1) };
        }
    }
}

fn draw_main(l: &mut Layout, v: &View) {
    let (c, s) = (&v.main, &v.state);
    l.text(c.title, 0, "Fix the dock's connect and disconnect loop");
    l.space(-6);
    l.text(c.subtitle, 0, "For the i-tec CADUAL4KDOCKPD's loop on Macs. Five steps, about 15 minutes.");
    colour(c.subtitle, Some(SECONDARY));
    l.space(8);
    if let Some(why) = &v.stopped {
        l.callout(c.stopped, 0, Tone::Danger, "This app cannot talk to the dock", why);
    }
    if v.close_refused && s.writing() {
        l.callout(c.close_refused, 0, Tone::Warning, "The app stays open until the dock has been written and checked", "");
    }
    // the last step has no step after it, so three plug-ins without a loop mark it done
    let passed = s.step == 5 && s.reconnects >= 3;
    let summaries = [
        "Dock connected.",
        s.identified.as_ref().map_or("", |note| note.text.as_str()),
        if s.already_patched { "Not needed: the fix is already in this dock." } else { "Written and verified." },
        if s.already_patched { "Not needed." } else { "Restarted." },
    ];
    for i in 0..5 {
        let number = i + 1;
        let (done, current) = (s.step > number || (number == 5 && passed), s.step == number);
        mark(c.badges[i], Drawn::Badge { number, done, current });
        set_text(c.badges[i], &format!("Step {number}, {}", if done { "done" } else if current { "current step" } else { "not started" }));
        set_text(c.titles[i], STEP_TITLES[i]);
        l.face(c.titles[i], if current { Face::StepBold } else { Face::Step });
        colour(c.titles[i], if done || current { None } else { Some(SECONDARY) });
        // the badge, and the title beside it
        let size = l.px(28);
        let width = l.width - l.px(INDENT);
        let height = measure(l.dc, l.font_of(c.titles[i]), STEP_TITLES[i], width);
        l.put(c.badges[i], l.left, l.y, size, size);
        l.put(c.titles[i], l.left + l.px(INDENT), l.y + (size - height).max(0) / 2, width, height);
        l.y += size.max(height) + l.px(GAP);
        if let Some(summary) = summaries.get(i).filter(|summary| done && !summary.is_empty()) {
            l.text(c.summaries[i], INDENT, summary);
            colour(c.summaries[i], Some(SECONDARY));
        }
        if current {
            match number {
                1 => connect_step(l, c),
                2 => identify_step(l, c, s),
                3 => patch_step(l, v),
                4 => restart_step(l, c, s),
                _ => test_step(l, c, s),
            }
        }
        l.space(8);
    }
    let details = l.button(c.details, if v.details { "Hide details" } else { "Show details" });
    l.row(0, &[details]);
    if v.details {
        // a box of its own: a new line scrolls in there instead of moving the steps
        let height = l.px(240);
        l.put(c.log, l.left, l.y, l.width, height);
        l.y += height + l.px(GAP);
    }
}

fn connect_step(l: &mut Layout, c: &MainControls) {
    l.text(c.connect_text, INDENT, "Plug in the dock's power supply, then connect the dock to this computer.");
    l.busy(c.connect_bar, c.connect_busy, INDENT, "Looking for the dock...");
    l.text(c.connect_hint, INDENT, "If the dock does not show up within 10 seconds, unplug the cable, wait 3 seconds and plug it in again.");
    colour(c.connect_hint, Some(SECONDARY));
}

fn identify_step(l: &mut Layout, c: &MainControls, s: &DockState) {
    l.text(c.identify_text, INDENT, "A quick look at the dock. It only reads and takes a few seconds.");
    if s.runs("identify") {
        l.busy(c.identify_bar, c.identify_busy, INDENT, "Reading the dock...");
    } else {
        enable(c.identify_button, s.idle());
        let identify = l.button(c.identify_button, if s.identified.is_none() { "Identify the dock" } else { "Try again" });
        l.row(INDENT, &[identify]);
    }
    l.note(c.identify_note, INDENT, &s.identified);
}

fn patch_step(l: &mut Layout, v: &View) {
    let (c, s) = (&v.main, &v.state);
    let writing = s.runs("patch");
    if writing {
        l.callout(c.writing, INDENT, Tone::Warning, "Keep the dock's power supply and the cable connected", "The dock is being checked, erased, written and checked again. This takes about 8 minutes.");
        l.progress(c.patch_progress, c.patch_bar, INDENT, &v.progress);
    } else if s.after_failed_write {
        l.callout(c.failed, INDENT, Tone::Danger, "The write could not be verified. Do not unplug the dock's power supply.", "The dock keeps working as long as it stays powered. Write again now.");
        enable(c.write_again, s.idle());
        let again = l.button(c.write_again, "Write again");
        l.row(INDENT, &[again]);
    } else if let Some(missing) = &s.image_error {
        l.callout(c.missing, INDENT, Tone::Danger, "The firmware image is missing", missing);
    } else {
        l.callout(c.confirm, INDENT, Tone::Warning, "This switches the setting \"Try.SRC\" off. It erases the dock's power controller and writes new firmware.", PATCH_WARNING);
        enable(c.patch_button, s.idle() && window_text(c.patch_word).trim() == "ERASE");
        let row = [l.label(c.patch_label, "Type ERASE to confirm:"), l.field(c.patch_word, l.px(130)), l.button(c.patch_button, "Apply the fix")];
        l.row(INDENT, &row);
        l.note(c.patch_note, INDENT, &s.patched);
    }
}

fn restart_step(l: &mut Layout, c: &MainControls, s: &DockState) {
    l.callout(c.restart, INDENT, Tone::Success, "Written and verified", "The dock uses the new setting once it has been restarted.");
    let lines = [
        format!("1.  Unplug the USB-C cable from this computer.{}", if s.unplugged { "  Done." } else { "" }),
        "2.  Unplug the dock's power supply and wait 15 seconds.".to_string(),
        "3.  Plug the power supply back in, then the cable.".to_string(),
    ];
    for (hwnd, line) in c.restart_lines.iter().zip(&lines) {
        l.text(*hwnd, INDENT, line);
        l.space(-4);
    }
    l.space(4);
    l.busy(c.restart_bar, c.restart_busy, INDENT, if s.unplugged { "Waiting for the dock to come back..." } else { "Waiting for you to unplug the cable..." });
    let restarted = l.button(c.restarted, "I have restarted it");
    l.row(INDENT, &[restarted]);
}

fn test_step(l: &mut Layout, c: &MainControls, s: &DockState) {
    l.text(c.test_text, INDENT, "The loop only shows on a Mac: plug the dock into the Mac, where every plug-in should now bring it up within about 2 seconds. On this PC, unplug and replug the cable a few times to see that the dock still connects.");
    l.text(c.present, INDENT, if s.present { "Dock connected" } else { "Dock not connected" });
    colour(c.present, Some(if s.present { DONE } else { SECONDARY }));
    if s.reconnects >= 3 {
        let title = format!("Connected {} times since the restart", s.reconnects);
        l.callout(c.test_done, INDENT, Tone::Success, &title, "The dock connects as before. You can close this window and test it on the Mac.");
    } else if s.reconnects > 0 {
        l.text(c.test_count, INDENT, &format!("Connected {} time(s) since the restart. Try it three times.", s.reconnects));
    }
}

fn draw_hubs(l: &mut Layout, v: &View) {
    l.text(v.hub_text, 0, HUB_TEXT);
}

fn draw_more(l: &mut Layout, v: &View) {
    let (c, s) = (&v.more, &v.state);
    l.text(c.text, 0, "Writes the dock's original firmware back, with Try.SRC on again.");
    enable(c.restore, s.idle() && s.image_error.is_none() && !v.restore_asked);
    let restore = l.button(c.restore, "Restore the original firmware...");
    l.row(0, &[restore]);
    if v.restore_asked {
        l.callout(c.confirm, 0, Tone::Warning, "This erases the dock's power controller and writes the original firmware", RESTORE_WARNING);
        enable(c.go, s.idle() && window_text(c.word).trim() == "ERASE");
        let row = [l.label(c.label, "Type ERASE to confirm:"), l.field(c.word, l.px(130)), l.button(c.go, "Restore the original"), l.button(c.cancel, "Cancel")];
        l.row(0, &row);
    }
    if s.runs("restore") {
        l.progress(c.progress, c.bar, 0, &v.progress);
    }
    l.note(c.note, 0, &s.restore_note);
    let text = "An earlier write was not verified: skip the content check next time";
    set_text(c.skip_check, text);
    unsafe {
        let checked = SendMessageW(c.skip_check, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT;
        if checked != s.after_failed_write {
            SendMessageW(c.skip_check, BM_SETCHECK, if s.after_failed_write { BST_CHECKED } else { BST_UNCHECKED } as WPARAM, 0);
        }
    }
    enable(c.skip_check, s.idle());
    let height = measure(l.dc, l.font_of(c.skip_check), text, l.width - l.px(24)).max(l.px(20));
    l.put(c.skip_check, l.left, l.y, l.width, height);
    l.y += height + l.px(GAP);
    if let Some(missing) = &s.image_error {
        l.callout(c.missing, 0, Tone::Warning, "The firmware image is missing", missing);
        l.text(c.how, 0, "The tool takes it from Cable Matters' updater, DUD15X0D1_Update_E108.exe. Download it, do not run it, and give its path here.");
        enable(c.use_image, s.idle() && !window_text(c.path).trim().is_empty());
        let row = [l.field(c.path, 0), l.button(c.use_image, "Use this file")];
        l.row(0, &row);
        let page = l.button(c.updater_page, "Open Cable Matters' download page");
        l.row(0, &[page]);
    }
}

/// Lays out one page and sets its scroll bar. A page that is not shown is left as it is.
fn layout(page: usize) {
    let Some((hwnd, dpi, fonts, scroll, children, view)) = with(|app| {
        let p = &app.pages[page];
        (p.hwnd, p.dpi, p.fonts, p.scroll, p.children.iter().map(|&(child, _)| child).collect::<Vec<_>>(), app.view())
    }) else {
        return;
    };
    if page != MAIN && unsafe { IsWindowVisible(hwnd) } == 0 {
        return;
    }
    let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(hwnd, &mut client) };
    let pad = scale(if page == MAIN { 28 } else { 20 }, dpi);
    let width = (client.right - pad * 2).clamp(scale(200, dpi), scale(724, dpi));
    let dc = unsafe { GetDC(hwnd) };
    let mut l = Layout { page, dc, dpi, fonts, left: pad, width, y: pad, scroll, shown: HashSet::new() };
    match page {
        MAIN => draw_main(&mut l, &view),
        HUBS => draw_hubs(&mut l, &view),
        _ => draw_more(&mut l, &view),
    }
    unsafe { ReleaseDC(hwnd, dc) };
    for child in children {
        if !l.shown.contains(&(child as isize)) {
            unsafe { ShowWindow(child, SW_HIDE) };
        }
    }
    let content = l.y - l.px(GAP) + pad;
    let most = (content - client.bottom).max(0);
    if scroll > most {
        with(|app| app.pages[page].scroll = most);
        AGAIN.set(true);
    }
    let info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: content - 1,
        nPage: client.bottom.max(0) as u32,
        nPos: scroll.min(most),
        nTrackPos: 0,
    };
    unsafe { SetScrollInfo(hwnd, SB_VERT, &info, 1) };
}

/// Brings every shown page up to date with the engine's state.
fn refresh() {
    if BUSY.get() {
        AGAIN.set(true);
        return;
    }
    BUSY.set(true);
    // a scroll bar that appears or goes narrows or widens the page: one more round; three are plenty
    for _ in 0..3 {
        AGAIN.set(false);
        for page in [MAIN, HUBS, MORE] {
            layout(page);
        }
        if !AGAIN.get() {
            break;
        }
    }
    BUSY.set(false);
}

/// Draws a badge or a callout (`WM_DRAWITEM`).
fn paint(page: HWND, item: &DRAWITEMSTRUCT) {
    let found = with(|app| {
        let page = app.pages.iter().find(|p| p.hwnd == page)?;
        Some((app.drawn.get(&(item.hwndItem as isize))?.clone(), page.fonts, page.dpi))
    });
    let Some(Some((drawn, fonts, dpi))) = found else { return };
    let (dc, rect) = (item.hDC, item.rcItem);
    // the fonts, pens and colours picked below go back to how the system handed the context over
    let saved = unsafe { SaveDC(dc) };
    unsafe {
        FillRect(dc, &rect, GetSysColorBrush(COLOR_WINDOW));
        SetBkMode(dc, TRANSPARENT as _);
    }
    let font = |face: Face| fonts[face as usize];
    match drawn {
        Drawn::Badge { number, done, current } => {
            let (circle, fill) = if done {
                ("\u{EA3B}", DONE)
            } else if current {
                ("\u{EA3B}", ACCENT)
            } else {
                ("\u{EA3A}", UPCOMING)
            };
            draw(dc, font(Face::Circle), circle, fill, rect, CENTRED);
            if done {
                draw(dc, font(Face::Mark), "\u{E73E}", WHITE, rect, CENTRED);
            } else {
                let face = if current { Face::Bold } else { Face::Body };
                draw(dc, font(face), &number.to_string(), if current { WHITE } else { UPCOMING }, rect, CENTRED);
            }
        }
        Drawn::Callout { tone, title, message } => {
            let (fill, icon_colour, icon) = tone.look();
            unsafe {
                let brush = CreateSolidBrush(fill);
                let pen = CreatePen(PS_SOLID, 1, fill);
                let (old_brush, old_pen) = (SelectObject(dc, brush), SelectObject(dc, pen));
                let round = scale(8, dpi);
                RoundRect(dc, rect.left, rect.top, rect.right, rect.bottom, round, round);
                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                DeleteObject(brush);
                DeleteObject(pen);
            }
            let pad = scale(12, dpi);
            let text_left = rect.left + pad + scale(28, dpi);
            let icon_place = RECT { left: rect.left + pad, top: rect.top + pad + scale(1, dpi), right: text_left, bottom: rect.bottom };
            draw(dc, font(Face::Icon), icon, icon_colour, icon_place, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
            let title_height = measure(dc, font(Face::Bold), &title, rect.right - pad - text_left);
            let title_place = RECT { left: text_left, top: rect.top + pad, right: rect.right - pad, bottom: rect.top + pad + title_height };
            draw(dc, font(Face::Bold), &title, TEXT, title_place, WRAP);
            if !message.is_empty() {
                let top = title_place.bottom + scale(4, dpi);
                draw(dc, font(Face::Body), &message, TEXT, RECT { left: text_left, top, right: rect.right - pad, bottom: rect.bottom }, WRAP);
            }
        }
    }
    unsafe { RestoreDC(dc, saved) };
}

// ---- Acting on the user and on the engine --------------------------------------------------------

/// Sends one command to the engine. The engine ends by itself when this window does.
fn send(command: &str) {
    with(|app| {
        if let Some(engine) = &mut app.engine {
            // a gone engine is reported by the reader thread
            let _ = writeln!(engine, "{command}").and_then(|()| engine.flush());
        }
    });
}

/// Shows the log's last lines.
fn log_to_end(log: HWND) {
    unsafe {
        let end = GetWindowTextLengthW(log).max(0) as usize;
        SendMessageW(log, EM_SETSEL, end, end as LPARAM);
        SendMessageW(log, EM_SCROLLCARET, 0, 0);
    }
}

fn show_page(page: usize) {
    let Some((hwnd, main, placed, dpi)) = with(|app| {
        let main = app.pages[MAIN].hwnd;
        let p = &mut app.pages[page];
        (p.hwnd, main, std::mem::replace(&mut p.placed, true), p.dpi)
    }) else {
        return;
    };
    unsafe {
        if !placed {
            let mut at = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(main, &mut at);
            let (width, height) = if page == HUBS { (520, 240) } else { (600, 560) };
            let offset = scale(48, dpi);
            SetWindowPos(hwnd, null_mut(), at.left + offset, at.top + offset, scale(width, dpi), scale(height, dpi), SWP_NOZORDER | SWP_NOACTIVATE);
        }
        ShowWindow(hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(hwnd);
    }
    refresh();
}

fn command(id: u16, code: u32) {
    let Some(v) = with(|app| app.view()) else { return };
    let typed = |hwnd: HWND| window_text(hwnd).trim().to_string();
    match (id, code) {
        (ID_IDENTIFY, BN_CLICKED) => send("identify"),
        (ID_WRITE_AGAIN, BN_CLICKED) => send("patch"),
        (ID_PATCH, BN_CLICKED) => {
            // the engine checks the typed word again before it starts
            send(&format!("patch {}", typed(v.main.patch_word)));
            set_text(v.main.patch_word, "");
        }
        (ID_RESTARTED, BN_CLICKED) => send("restarted"),
        (ID_DETAILS, BN_CLICKED) => {
            let shown = with(|app| {
                app.details = !app.details;
                app.details
            });
            refresh();
            if shown == Some(true) {
                log_to_end(v.main.log);
            }
            return;
        }
        (ID_MENU_HUBS, _) => return show_page(HUBS),
        (ID_MENU_MORE, _) => return show_page(MORE),
        (ID_RESTORE_ASK, BN_CLICKED) => {
            set_text(v.more.word, "");
            with(|app| app.restore_asked = true);
        }
        (ID_RESTORE_GO, BN_CLICKED) => {
            send(&format!("restore {}", typed(v.more.word)));
            set_text(v.more.word, "");
            with(|app| app.restore_asked = false);
        }
        (ID_RESTORE_CANCEL, BN_CLICKED) => {
            with(|app| app.restore_asked = false);
        }
        (ID_SKIP_CHECK, BN_CLICKED) => {
            let on = unsafe { SendMessageW(v.more.skip_check, BM_GETCHECK, 0, 0) } == BST_CHECKED as LRESULT;
            send(if on { "skip-check on" } else { "skip-check off" });
        }
        (ID_USE_IMAGE, BN_CLICKED) => send(&format!("image {}", typed(v.more.path))),
        (ID_UPDATER_PAGE, BN_CLICKED) => unsafe {
            ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(UPDATER_PAGE).as_ptr(), null(), null(), SW_SHOWNORMAL);
        },
        (ID_PATCH_WORD | ID_RESTORE_WORD | ID_IMAGE_PATH, EN_CHANGE) => {}
        _ => return,
    }
    refresh();
}

/// Takes the lines the engine has sent (`WM_ENGINE`).
fn take_lines() {
    let lines = std::mem::take(&mut *crate::lock(&LINES));
    let Some((log, added, first, open_more)) = with(|app| {
        for line in lines {
            match line {
                Some(line) => app.model.take(&line),
                None => app.model.stopped = Some("The part of this app that talks to the dock has stopped. Close the app and open it again.".to_string()),
            }
        }
        let added = app.model.log[app.log_shown..].join("\r\n");
        let first = app.log_shown == 0;
        app.log_shown = app.model.log.len();
        // a build without the firmware inside: its image file is chosen in "More options"
        let open_more = app.model.state.image_error.is_some() && !app.more_opened;
        app.more_opened |= open_more;
        (app.main.log, added, first, open_more)
    }) else {
        return;
    };
    if !added.is_empty() {
        unsafe {
            if first {
                SetWindowTextW(log, wide(&added).as_ptr());
            } else {
                let end = GetWindowTextLengthW(log).max(0) as usize;
                SendMessageW(log, EM_SETSEL, end, end as LPARAM);
                SendMessageW(log, EM_REPLACESEL, 0, wide(&format!("\r\n{added}")).as_ptr() as LPARAM);
            }
        }
        log_to_end(log);
    }
    if open_more {
        show_page(MORE);
    }
    refresh();
}

fn close(hwnd: HWND) {
    let found = with(|app| (app.pages.iter().position(|p| p.hwnd == hwnd), app.model.state.writing(), app.model.state.runs("restore")));
    match found {
        // closing would leave the user without the result of a write
        Some((Some(MAIN), true, _)) => {
            with(|app| app.close_refused = true);
            refresh();
        }
        // the progress of a restore is shown there
        Some((Some(MORE), _, true)) => {}
        Some((Some(HUBS | MORE), _, _)) => unsafe {
            ShowWindow(hwnd, SW_HIDE);
        },
        _ => unsafe {
            DestroyWindow(hwnd);
        },
    }
}

fn scroll_to(hwnd: HWND, to: impl FnOnce(i32, i32, i32) -> i32) {
    let mut info: SCROLLINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<SCROLLINFO>() as u32;
    info.fMask = SIF_ALL;
    unsafe { GetScrollInfo(hwnd, SB_VERT, &mut info) };
    let position = to(info.nPos, info.nPage as i32, info.nTrackPos).max(0);
    with(|app| {
        if let Some(page) = app.pages.iter_mut().find(|p| p.hwnd == hwnd) {
            page.scroll = position;
        }
    });
    refresh();
}

fn dpi_changed(hwnd: HWND, dpi: u32, suggested: &RECT) {
    let fonts = make_fonts(dpi);
    let found = with(|app| {
        let page = app.pages.iter_mut().find(|p| p.hwnd == hwnd)?;
        page.dpi = dpi;
        Some((std::mem::replace(&mut page.fonts, fonts), page.children.clone()))
    });
    let Some(Some((old, children))) = found else { return };
    unsafe {
        for (child, face) in children {
            SendMessageW(child, WM_SETFONT, fonts[face as usize] as WPARAM, 0);
        }
        for font in old {
            DeleteObject(font);
        }
        let (width, height) = (suggested.right - suggested.left, suggested.bottom - suggested.top);
        SetWindowPos(hwnd, null_mut(), suggested.left, suggested.top, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    refresh();
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let high = ((wparam >> 16) & 0xffff) as u32;
    match message {
        WM_COMMAND => command((wparam & 0xffff) as u16, high),
        WM_CTLCOLORSTATIC => {
            let colour = with(|app| app.colours.get(&lparam).copied()).flatten().unwrap_or(TEXT);
            return unsafe {
                SetTextColor(wparam as HDC, colour);
                SetBkColor(wparam as HDC, GetSysColor(COLOR_WINDOW));
                GetSysColorBrush(COLOR_WINDOW) as LRESULT
            };
        }
        WM_DRAWITEM => {
            // SAFETY: for WM_DRAWITEM, lparam points to the item to draw
            paint(hwnd, unsafe { &*(lparam as *const DRAWITEMSTRUCT) });
            return 1;
        }
        WM_SIZE => refresh(),
        WM_VSCROLL => {
            let line = scale(40, unsafe { GetDpiForWindow(hwnd) });
            scroll_to(hwnd, |position, page, track| match (wparam & 0xffff) as i32 {
                SB_LINEUP => position - line,
                SB_LINEDOWN => position + line,
                SB_PAGEUP => position - page,
                SB_PAGEDOWN => position + page,
                SB_THUMBTRACK | SB_THUMBPOSITION => track,
                SB_TOP => 0,
                SB_BOTTOM => i32::MAX / 4,
                _ => position,
            });
        }
        WM_MOUSEWHEEL => {
            let notches = i32::from(high as u16 as i16);
            let step = scale(60, unsafe { GetDpiForWindow(hwnd) });
            scroll_to(hwnd, |position, _, _| position - notches * step / 120);
        }
        // SAFETY: for WM_DPICHANGED, lparam points to the suggested window rectangle
        WM_DPICHANGED => dpi_changed(hwnd, high, unsafe { &*(lparam as *const RECT) }),
        WM_GETMINMAXINFO => {
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            // SAFETY: for WM_GETMINMAXINFO, lparam points to the sizes to fill in
            let sizes = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            sizes.ptMinTrackSize = POINT { x: scale(420, dpi), y: scale(320, dpi) };
        }
        WM_CLOSE => close(hwnd),
        WM_DESTROY => {
            if hwnd as isize == MAIN_WINDOW.load(Ordering::Relaxed) {
                unsafe { PostQuitMessage(0) };
            }
        }
        WM_ENGINE => take_lines(),
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
    0
}

// ---- Starting ------------------------------------------------------------------------------------

/// Starts `<this program> serve`, and a thread that hands its lines to the window.
fn start_engine(window: HWND) -> Result<ChildStdin, String> {
    let program = std::env::current_exe().map_err(|why| format!("This program cannot find its own file: {why}"))?;
    let mut engine = Command::new(&program)
        .arg("serve")
        // a build for internal use carries the firmware image at the end of this program's own file
        .env("DOCK_IMAGE_IN", &program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|why| format!("The dock engine ({}) could not be started: {why}", program.display()))?;
    let (Some(input), Some(output)) = (engine.stdin.take(), engine.stdout.take()) else {
        return Err("The dock engine started without its pipes.".to_string());
    };
    let window = window as isize;
    std::thread::spawn(move || {
        let mut output = BufReader::new(output);
        let mut line = Vec::new();
        // read on a thread of its own, so a busy window never keeps the engine waiting
        while output.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
            let text = String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string();
            crate::lock(&LINES).push(Some(text));
            line.clear();
            unsafe { PostMessageW(window as HWND, WM_ENGINE, 0, 0) };
        }
        crate::lock(&LINES).push(None);
        unsafe { PostMessageW(window as HWND, WM_ENGINE, 0, 0) };
    });
    // the engine is not waited for: it ends by itself when its input closes, after a running write
    Ok(input)
}

fn start() -> Result<(), String> {
    unsafe {
        let controls = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_STANDARD_CLASSES | ICC_PROGRESS_CLASS };
        InitCommonControlsEx(&controls);
        let instance = GetModuleHandleW(null());
        let class_name = wide("CADUAL4KDOCKPD.Patch");
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(procedure),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as usize as HBRUSH,
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };
        if RegisterClassExW(&class) == 0 {
            return Err(last_error());
        }
        let style = WS_VSCROLL | WS_CLIPCHILDREN;
        let window = |title: &str, style: u32, owner: HWND, menu: HMENU| {
            CreateWindowExW(WS_EX_CONTROLPARENT, class_name.as_ptr(), wide(title).as_ptr(), style, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, owner, menu, instance, null())
        };
        let main = window(TITLE, WS_OVERLAPPEDWINDOW | style, null_mut(), menu());
        if main.is_null() {
            return Err(last_error());
        }
        MAIN_WINDOW.store(main as isize, Ordering::Relaxed);
        // owned by the main window: they stay in front of it and close with it
        let side = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | style;
        let (hubs, more) = (window("Hub firmware 93.92", side, main, null_mut()), window("More options", side, main, null_mut()));
        if hubs.is_null() || more.is_null() {
            return Err(last_error());
        }
        let mut pages = vec![Page::new(main), Page::new(hubs), Page::new(more)];
        let main_controls = build_main(&mut pages[MAIN]);
        let hub_text = label(&mut pages[HUBS], Face::Body);
        let more_controls = build_more(&mut pages[MORE]);
        let engine = start_engine(main);
        let model = Model { stopped: engine.as_ref().err().cloned(), ..Model::default() };
        APP.with(|app| {
            *app.borrow_mut() = Some(App {
                model,
                pages,
                main: main_controls,
                hub_text,
                more: more_controls,
                drawn: HashMap::new(),
                colours: HashMap::new(),
                close_refused: false,
                restore_asked: false,
                more_opened: false,
                details: false,
                log_shown: 0,
                engine: engine.ok(),
            })
        });
        let dpi = GetDpiForWindow(main).max(96);
        SetWindowPos(main, null_mut(), 0, 0, scale(780, dpi), scale(860, dpi), SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        ShowWindow(main, SW_SHOWNORMAL);
        refresh();
    }
    Ok(())
}

/// Saves the error and shows it: a failure must not be silent.
fn report(what: &str, detail: &str) {
    let file = std::env::temp_dir().join(ERROR_FILE);
    let text = format!("{what}\n\n{detail}");
    // showing it still works when saving does not
    let _ = std::fs::write(&file, &text);
    let shown = wide(&format!("{text}\n\nThis text was also saved to {}", file.display()));
    unsafe { MessageBoxW(null_mut(), shown.as_ptr(), wide(TITLE).as_ptr(), MB_ICONERROR | MB_OK) };
}

/// Opens the window and runs until it is closed.
pub fn run() -> ExitCode {
    // the engine is another process: whatever happens here, a write in progress runs to its end
    std::panic::set_hook(Box::new(|info| report("CADUAL4KDOCKPD Patch ran into a problem. If the dock is being written, leave everything connected.", &info.to_string())));
    if let Err(why) = start() {
        report("CADUAL4KDOCKPD Patch could not start.", &why);
        return ExitCode::from(2);
    }
    let mut message: MSG = unsafe { std::mem::zeroed() };
    unsafe {
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            // Tab and Shift+Tab move between the controls of the window the key went to
            let top = GetAncestor(message.hwnd, GA_ROOT);
            if top.is_null() || IsDialogMessageW(top, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    // closes the engine's input: it ends once it is idle
    APP.with(|app| app.borrow_mut().take());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_should_follow_the_engines_state_progress_and_log_lines() {
        let mut model = Model::default();
        model.take(
            "{\"state\":{\"step\":\"patch\",\"present\":true,\"running\":\"patch\",\"identified\":{\"ok\":true,\"text\":\"It can be patched.\"},\
             \"patched\":null,\"hubNote\":null,\"restoreNote\":{\"ok\":false,\"text\":\"no\"},\"alreadyPatched\":false,\"unplugged\":false,\
             \"reconnects\":2,\"afterFailedWrite\":true,\"imageError\":\"missing\",\"hubRomsError\":null}}",
        );
        let state = &model.state;
        assert_eq!((state.step, state.present, state.reconnects, state.after_failed_write), (3, true, 2, true));
        assert!(state.writing() && state.runs("patch") && !state.idle());
        assert_eq!(state.identified, Some(Note { ok: true, text: "It can be patched.".to_string() }));
        assert_eq!(state.restore_note, Some(Note { ok: false, text: "no".to_string() }));
        assert_eq!((state.patched.clone(), state.image_error.as_deref()), (None, Some("missing")));

        model.take("{\"progress\":{\"label\":\"Writing\",\"fraction\":0.45}}");
        assert_eq!(model.progress, ("Writing".to_string(), 0.45));
        model.take("{\"log\":\"erase finished after 2.7 s\"}");
        model.take("not an engine line");
        assert_eq!(model.log, ["erase finished after 2.7 s", "not an engine line"]);
    }
}
