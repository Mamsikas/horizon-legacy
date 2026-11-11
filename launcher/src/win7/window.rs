//! A small USER32/GDI window, with all filesystem/network work on one background thread.
use super::flow::{self, Event, Settings};
use crate::i18n::tr;
use std::{
    ffi::c_void,
    fs,
    io::Write,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};

type Handle = isize;
const CHILD: u32 = 0x40000000;
const VISIBLE: u32 = 0x10000000;
const TAB: u32 = 0x10000;
const BORDER: u32 = 0x800000;
const PLAY: usize = 100;
const CANCEL: usize = 101;
const BROWSE: usize = 102;
const CLIENT: usize = 103;
const AUTOMATIC: usize = 104;
const FOLDER: usize = 105;
const LOG: usize = 106;
const DETAILS: usize = 107;
const NAME: usize = 108;
const MINECRAFT: usize = 109;

#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Message {
    hwnd: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: Point,
    private: u32,
}
#[repr(C)]
struct Class {
    style: u32,
    procedure: Option<unsafe extern "system" fn(Handle, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu: *const u16,
    name: *const u16,
}
#[repr(C)]
struct Create {
    parameter: *mut c_void,
    instance: Handle,
    menu: Handle,
    parent: Handle,
    cy: i32,
    cx: i32,
    y: i32,
    x: i32,
    style: i32,
    name: *const u16,
    class: *const u16,
    ex_style: u32,
}
#[repr(C)]
struct BrowseInfo {
    owner: Handle,
    root: *const c_void,
    display: *mut u16,
    title: *const u16,
    flags: u32,
    callback: Option<unsafe extern "system" fn(Handle, u32, isize, isize) -> i32>,
    parameter: isize,
    image: i32,
}
#[repr(C)]
struct OpenFile {
    size: u32,
    owner: Handle,
    instance: Handle,
    filter: *const u16,
    custom_filter: *mut u16,
    max_custom_filter: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_dir: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    extension_offset: u16,
    default_extension: *const u16,
    data: isize,
    hook: *const c_void,
    template: *const u16,
    reserved: *mut c_void,
    reserved_number: u32,
    flags_ex: u32,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassW(class: *const Class) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        parameter: *mut c_void,
    ) -> Handle;
    fn DefWindowProcW(hwnd: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    fn SetWindowLongPtrW(hwnd: Handle, index: i32, value: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: Handle, index: i32) -> isize;
    fn GetMessageW(message: *mut Message, hwnd: Handle, min: u32, max: u32) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn IsDialogMessageW(hwnd: Handle, message: *const Message) -> i32;
    fn PostQuitMessage(code: i32);
    fn ShowWindow(hwnd: Handle, how: i32) -> i32;
    fn DestroyWindow(hwnd: Handle) -> i32;
    fn MoveWindow(hwnd: Handle, x: i32, y: i32, width: i32, height: i32, repaint: i32) -> i32;
    fn SetWindowPos(
        hwnd: Handle,
        after: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn GetClientRect(hwnd: Handle, rect: *mut Rect) -> i32;
    fn SetWindowTextW(hwnd: Handle, text: *const u16) -> i32;
    fn GetWindowTextLengthW(hwnd: Handle) -> i32;
    fn GetWindowTextW(hwnd: Handle, text: *mut u16, count: i32) -> i32;
    fn SendMessageW(hwnd: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    fn SetTimer(hwnd: Handle, id: usize, ms: u32, proc: *const c_void) -> usize;
    fn KillTimer(hwnd: Handle, id: usize) -> i32;
    fn EnableWindow(hwnd: Handle, enabled: i32) -> i32;
    fn SetFocus(hwnd: Handle) -> Handle;
    fn GetDC(hwnd: Handle) -> Handle;
    fn ReleaseDC(hwnd: Handle, dc: Handle) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
    fn MessageBoxW(hwnd: Handle, text: *const u16, title: *const u16, kind: u32) -> i32;
    fn SetProcessDPIAware() -> i32;
    fn SystemParametersInfoW(action: u32, parameter: u32, value: *mut c_void, flags: u32) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn GetDeviceCaps(dc: Handle, index: i32) -> i32;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        output: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> Handle;
    fn DeleteObject(object: Handle) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
}
#[link(name = "comctl32")]
extern "system" {
    fn InitCommonControls();
}
#[link(name = "comdlg32")]
extern "system" {
    fn GetOpenFileNameW(file: *mut OpenFile) -> i32;
}
#[link(name = "shell32")]
extern "system" {
    fn SHBrowseForFolderW(info: *const BrowseInfo) -> *mut c_void;
    fn SHGetPathFromIDListW(item: *const c_void, path: *mut u16) -> i32;
    fn ShellExecuteW(
        hwnd: Handle,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
}
#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *const c_void, apartment: u32) -> i32;
    fn CoTaskMemFree(memory: *const c_void);
    fn CoUninitialize();
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn set(hwnd: Handle, text: &str) {
    unsafe {
        SetWindowTextW(hwnd, wide(text).as_ptr());
    }
}
fn text(hwnd: Handle) -> String {
    unsafe {
        let mut buffer = vec![0; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let n = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&buffer[..n])
    }
}
fn send(hwnd: Handle, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe { SendMessageW(hwnd, msg, wp, lp) }
}
fn show(hwnd: Handle, visible: bool) {
    unsafe {
        ShowWindow(hwnd, if visible { 5 } else { 0 });
    }
}
fn enable(hwnd: Handle, enabled: bool) {
    unsafe {
        EnableWindow(hwnd, i32::from(enabled));
    }
}

struct App {
    hwnd: Handle,
    font: Handle,
    heading_font: Handle,
    dpi: i32,
    settings: Settings,
    title: Handle,
    intro: Handle,
    mc_label: Handle,
    minecraft: Handle,
    browse: Handle,
    name_label: Handle,
    name: Handle,
    client_label: Handle,
    client: Handle,
    choose_client: Handle,
    automatic: Handle,
    status: Handle,
    progress: Handle,
    play: Handle,
    cancel: Handle,
    folder: Handle,
    log_button: Handle,
    details: Handle,
    output: Handle,
    expanded: bool,
    receiver: Option<Receiver<Event>>,
    playing: bool,
    closing: bool,
    game_started: bool,
    dispatching: bool,
    startup_error: Option<String>,
    tail: String,
    log: Option<fs::File>,
}

impl App {
    fn scale(&self, value: i32) -> i32 {
        value * self.dpi / 96
    }
    fn control(&self, class: &str, label: &str, style: u32, id: usize) -> Handle {
        unsafe {
            let control = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(label).as_ptr(),
                CHILD | VISIBLE | style,
                0,
                0,
                1,
                1,
                self.hwnd,
                id as isize,
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null_mut(),
            );
            send(control, 0x30, self.font as usize, 1);
            control
        }
    }
    fn label(&self, label: &str) -> Handle {
        self.control("STATIC", tr(label), 0, 0)
    }
    fn button(&self, label: &str, id: usize) -> Handle {
        self.control("BUTTON", tr(label), TAB, id)
    }
    fn init(&mut self) {
        self.title = self.label("Horizon");
        send(self.title, 0x30, self.heading_font as usize, 1);
        self.intro = self.label("Your Minecraft files. Your nickname. Ready to play.");
        self.mc_label = self.label("Minecraft 26.3 folder");
        self.minecraft = self.control(
            "EDIT",
            &self.settings.minecraft.to_string_lossy(),
            TAB | BORDER | 0x80,
            MINECRAFT,
        );
        self.browse = self.button("Browse…", BROWSE);
        self.name_label = self.label("Nickname (offline)");
        self.name = self.control("EDIT", &self.settings.nickname, TAB | BORDER | 0x80, NAME);
        send(self.name, 0xc5, 16, 0); // EM_SETLIMITTEXT
        self.client_label = self.label("Horizon client");
        self.client = self.control("EDIT", "", TAB | BORDER | 0x80 | 0x800, 0);
        self.choose_client = self.button("Choose…", CLIENT);
        self.automatic = self.button("Automatic", AUTOMATIC);
        self.status = self.label("Choose your Minecraft 26.3 folder, then press Play.");
        self.progress = self.control("msctls_progress32", "", 0, 0);
        send(self.progress, 0x406, 0, 1000); // PBM_SETRANGE32
        self.play = self.control("BUTTON", tr("Play"), TAB | 1, PLAY);
        self.cancel = self.button("Cancel", CANCEL);
        self.folder = self.button("Game folder", FOLDER);
        self.log_button = self.button("Open log", LOG);
        self.details = self.button("Details", DETAILS);
        self.output = self.control(
            "EDIT",
            "",
            TAB | BORDER | 4 | 0x40 | 0x800 | 0x1000 | 0x200000,
            0,
        );
        show(self.output, false);
        show(self.cancel, false);
        self.update_client();
        if let Some(error) = self.startup_error.clone() {
            self.add_log(&error);
            set(self.status, &error);
            enable(self.play, false);
        }
        self.layout();
        unsafe {
            SetFocus(self.name);
            SetTimer(self.hwnd, 1, 120, std::ptr::null());
        }
    }
    fn place(&self, control: Handle, x: i32, y: i32, w: i32, h: i32) {
        unsafe {
            MoveWindow(
                control,
                self.scale(x),
                self.scale(y),
                self.scale(w.max(1)),
                self.scale(h.max(1)),
                1,
            );
        }
    }
    fn layout(&self) {
        let mut rect = Rect::default();
        unsafe {
            GetClientRect(self.hwnd, &mut rect);
        }
        let width = rect.right * 96 / self.dpi;
        let height = rect.bottom * 96 / self.dpi;
        let body = width - 48;
        self.place(self.title, 24, 17, body, 37);
        self.place(self.intro, 24, 60, body, 32);
        self.place(self.mc_label, 24, 99, body, 22);
        self.place(self.minecraft, 24, 125, body - 102, 28);
        self.place(self.browse, width - 118, 124, 94, 30);
        self.place(self.name_label, 24, 169, body, 22);
        self.place(self.name, 24, 195, body.min(260), 28);
        self.place(self.client_label, 24, 239, body, 22);
        self.place(self.client, 24, 265, body - 200, 28);
        self.place(self.choose_client, width - 216, 264, 90, 30);
        self.place(self.automatic, width - 118, 264, 94, 30);
        self.place(self.status, 24, 310, body, 44);
        self.place(self.progress, 24, 357, body, 10);
        self.place(self.folder, 24, 383, 104, 34);
        self.place(self.log_button, 136, 383, 90, 34);
        self.place(self.details, 234, 383, 82, 34);
        self.place(self.cancel, width - 272, 383, 88, 34);
        self.place(self.play, width - 174, 379, 150, 42);
        self.place(self.output, 24, 437, body, height - 455);
    }
    fn update_client(&self) {
        set(
            self.client,
            if self.settings.client.as_os_str().is_empty() {
                tr("Automatic — bundled or downloaded")
            } else {
                self.settings.client.to_str().unwrap_or("")
            },
        );
        enable(
            self.automatic,
            self.receiver.is_none() && !self.settings.client.as_os_str().is_empty(),
        );
    }
    fn read_settings(&mut self) {
        self.settings.nickname = text(self.name).trim().into();
        self.settings.minecraft = PathBuf::from(text(self.minecraft).trim());
    }
    fn add_log(&mut self, line: &str) {
        if let Some(log) = &mut self.log {
            let _ = writeln!(log, "{line}");
        }
        self.tail.push_str(&line.replace('\n', "\r\n"));
        self.tail.push_str("\r\n");
        if self.tail.len() > 64 * 1024 {
            let at = self
                .tail
                .char_indices()
                .find(|(i, _)| *i >= 32 * 1024)
                .map_or(0, |(i, _)| i);
            self.tail.drain(..at);
        }
    }
    fn busy(&self, yes: bool) {
        for control in [
            self.name,
            self.minecraft,
            self.browse,
            self.choose_client,
            self.automatic,
        ] {
            enable(control, !yes);
        }
        enable(self.play, !yes && self.startup_error.is_none());
        show(self.cancel, yes && !self.playing);
    }
    fn start(&mut self) {
        if self.receiver.is_some() || self.startup_error.is_some() {
            return;
        }
        self.read_settings();
        if let Err(error) = self.settings.validate().and_then(|_| self.settings.save()) {
            self.error(&error);
            return;
        }
        let _ = fs::remove_file(flow::cancel_path());
        self.log = fs::File::create(flow::log_path()).ok();
        self.tail.clear();
        self.game_started = false;
        let (tx, rx) = mpsc::channel();
        let settings = self.settings.clone();
        self.receiver = Some(rx);
        self.playing = false;
        self.busy(true);
        set(self.status, tr("Checking game files…"));
        send(self.progress, 0x402, 0, 0);
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                flow::run(settings, &|event| {
                    let _ = tx.send(event);
                })
            }))
            .unwrap_or_else(|_| {
                Err(tr("Setup stopped unexpectedly. Open the log and retry.").into())
            });
            let _ = tx.send(Event::Finished(result));
        });
    }
    fn error(&mut self, error: &str) {
        self.add_log(error);
        set(self.status, error);
        unsafe {
            MessageBoxW(
                self.hwnd,
                wide(error).as_ptr(),
                wide(tr("Horizon")).as_ptr(),
                0x10,
            );
        }
    }
    fn cancel(&mut self) {
        if self.receiver.is_some() && !self.playing {
            let _ = fs::write(flow::cancel_path(), b"cancel");
            set(
                self.status,
                tr("Cancelling — finishing the current file operation…"),
            );
            enable(self.cancel, false);
        }
    }
    fn pump(&mut self) {
        let events: Vec<_> = self
            .receiver
            .iter()
            .flat_map(|r| r.try_iter().take(1000))
            .collect();
        let mut dirty = false;
        for event in events {
            match event {
                Event::Log(line) => {
                    self.add_log(&line);
                    dirty = true;
                }
                Event::Status(status) => {
                    self.add_log(&status);
                    dirty = true;
                    if !self.closing {
                        set(self.status, &status);
                    }
                }
                Event::Progress(p) => {
                    send(self.progress, 0x402, (p.clamp(0., 1.) * 1000.) as usize, 0);
                }
                Event::Playing => {
                    self.playing = true;
                    self.game_started = true;
                    show(self.cancel, false);
                    set(
                        self.status,
                        tr("Horizon is running. You can close this launcher."),
                    );
                    send(self.progress, 0x402, 1000, 0);
                    if self.closing {
                        unsafe {
                            DestroyWindow(self.hwnd);
                        }
                        return;
                    }
                }
                Event::Finished(result) => {
                    let cancelled = flow::cancel_path().exists();
                    self.receiver = None;
                    self.playing = false;
                    self.busy(false);
                    enable(self.cancel, true);
                    // Preserve the nickname/path edits and pick up the worker's installed tag.
                    self.settings.installed = Settings::load().installed;
                    self.update_client();
                    match result {
                        Ok(()) => set(self.status, tr("Ready to play again.")),
                        Err(error) if cancelled => {
                            self.add_log(&error);
                            set(
                                self.status,
                                tr("Setup cancelled. Press Play to continue later."),
                            );
                        }
                        Err(error) => self.error(&error),
                    }
                    dirty = true;
                    if self.closing {
                        unsafe {
                            DestroyWindow(self.hwnd);
                        }
                        return;
                    }
                }
            }
        }
        if dirty && self.expanded {
            set(self.output, &self.tail);
            send(self.output, 0xb1, usize::MAX, -1);
            send(self.output, 0xb7, 0, 0);
        }
    }
    fn command(&mut self, id: usize) {
        match id {
            PLAY => self.start(),
            CANCEL => self.cancel(),
            BROWSE if self.receiver.is_none() => {
                if let Some(path) = browse(self.hwnd, &text(self.minecraft)) {
                    set(self.minecraft, &path.to_string_lossy());
                    self.read_settings();
                    let _ = self.settings.save();
                }
            }
            CLIENT if self.receiver.is_none() => {
                if let Some(path) = choose_client(self.hwnd) {
                    self.settings.client = path;
                    self.read_settings();
                    let _ = self.settings.save();
                    self.update_client();
                }
            }
            AUTOMATIC if self.receiver.is_none() => {
                self.settings.client.clear();
                self.read_settings();
                let _ = self.settings.save();
                self.update_client();
            }
            FOLDER => {
                let path = crate::backend::game_home();
                let _ = fs::create_dir_all(&path);
                open(self.hwnd, &path);
            }
            LOG => {
                let path = if self.game_started {
                    crate::backend::data_dir().join("win7-game.log")
                } else {
                    flow::log_path()
                };
                if !path.exists() {
                    let _ = fs::write(&path, &self.tail);
                }
                open(self.hwnd, &path);
            }
            DETAILS => {
                self.expanded = !self.expanded;
                show(self.output, self.expanded);
                set(
                    self.details,
                    tr(if self.expanded {
                        "Hide details"
                    } else {
                        "Details"
                    }),
                );
                if self.expanded {
                    set(self.output, &self.tail);
                }
                let area = work_area();
                unsafe {
                    SetWindowPos(
                        self.hwnd,
                        0,
                        area.left,
                        area.top,
                        self.scale(680).min(area.right - area.left),
                        self.scale(if self.expanded { 680 } else { 475 })
                            .min(area.bottom - area.top),
                        0x4,
                    );
                }
                self.layout();
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn folder_init(hwnd: Handle, message: u32, _: isize, path: isize) -> i32 {
    if message == 1 && path != 0 {
        SendMessageW(hwnd, 0x467, 1, path);
    }
    0
}
fn browse(owner: Handle, initial: &str) -> Option<PathBuf> {
    let title = wide(tr(
        "Choose the folder containing your Minecraft 26.3 installation",
    ));
    let initial = wide(initial);
    let mut display = [0u16; 260];
    let info = BrowseInfo {
        owner,
        root: std::ptr::null(),
        display: display.as_mut_ptr(),
        title: title.as_ptr(),
        flags: 1 | 0x10 | 0x40,
        callback: Some(folder_init),
        parameter: initial.as_ptr() as isize,
        image: 0,
    };
    unsafe {
        let item = SHBrowseForFolderW(&info);
        if item.is_null() {
            return None;
        }
        let mut path = [0u16; 260];
        let ok = SHGetPathFromIDListW(item, path.as_mut_ptr());
        CoTaskMemFree(item);
        (ok != 0).then(|| {
            PathBuf::from(String::from_utf16_lossy(
                &path[..path.iter().position(|c| *c == 0).unwrap_or(path.len())],
            ))
        })
    }
}
fn choose_client(owner: Handle) -> Option<PathBuf> {
    let mut path = vec![0u16; 32768];
    let filter = wide(&format!("{}\0*.exe\0", tr("Horizon client (*.exe)")));
    let title = wide(tr("Choose horizon-client.exe"));
    unsafe {
        let mut info: OpenFile = std::mem::zeroed();
        info.size = std::mem::size_of::<OpenFile>() as u32;
        info.owner = owner;
        info.filter = filter.as_ptr();
        info.file = path.as_mut_ptr();
        info.max_file = path.len() as u32;
        info.title = title.as_ptr();
        info.flags = 0x1000 | 0x800 | 8; // existing path/file, no cwd change
        (GetOpenFileNameW(&mut info) != 0).then(|| {
            PathBuf::from(String::from_utf16_lossy(
                &path[..path.iter().position(|c| *c == 0).unwrap_or(path.len())],
            ))
        })
    }
}
fn open(owner: Handle, path: &std::path::Path) {
    unsafe {
        ShellExecuteW(
            owner,
            wide("open").as_ptr(),
            wide(&path.to_string_lossy()).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        );
    }
}
fn work_area() -> Rect {
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: 1024,
        bottom: 768,
    };
    unsafe {
        SystemParametersInfoW(48, 0, &mut rect as *mut _ as *mut c_void, 0);
    }
    rect
}

unsafe extern "system" fn procedure(hwnd: Handle, message: u32, wp: usize, lp: isize) -> isize {
    if message == 0x81 {
        // WM_NCCREATE: Box<App> remains stable until the message loop ends.
        let create = &*(lp as *const Create);
        SetWindowLongPtrW(hwnd, -21, create.parameter as isize);
        (*(create.parameter as *mut App)).hwnd = hwnd;
    }
    let ptr = GetWindowLongPtrW(hwnd, -21) as *mut App;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, message, wp, lp);
    }
    // USER32 can re-enter through control notifications, dialogs and DestroyWindow.
    // Never create a second mutable App reference while a handler owns the first.
    if message == 2 {
        KillTimer(hwnd, 1);
        PostQuitMessage(0);
        return 0;
    }
    if message == 0x111 && (wp >> 16) != 0 {
        return 0;
    }
    if (*ptr).dispatching {
        return DefWindowProcW(hwnd, message, wp, lp);
    }
    (*ptr).dispatching = true;
    let app = &mut *ptr;
    let result = match message {
        1 => {
            app.init();
            0
        }
        5 => {
            app.layout();
            0
        }
        0x111 => {
            if (wp >> 16) == 0 {
                app.command(wp & 0xffff);
            }
            0
        }
        0x113 => {
            app.pump();
            0
        }
        0x10 => {
            app.read_settings();
            let _ = app.settings.save();
            if app.receiver.is_some() && !app.playing {
                app.closing = true;
                app.cancel();
            } else {
                DestroyWindow(hwnd);
            }
            0
        }
        0x24 => {
            let area = work_area();
            // MINMAXINFO contains five POINT fields; the fourth is the minimum size.
            *(lp as *mut Point).add(3) = Point {
                x: app.scale(620).min(area.right - area.left),
                y: app.scale(475).min(area.bottom - area.top),
            };
            0
        }
        _ => DefWindowProcW(hwnd, message, wp, lp),
    };
    (*ptr).dispatching = false;
    result
}

pub fn run(startup_error: Option<String>) {
    unsafe {
        SetProcessDPIAware();
        InitCommonControls();
        let com = CoInitializeEx(std::ptr::null(), 2);
        let dc = GetDC(0);
        let dpi = GetDeviceCaps(dc, 90).max(96);
        ReleaseDC(0, dc);
        let font = |height, weight| {
            CreateFontW(
                -height * dpi / 96,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                1,
                0,
                0,
                0,
                0,
                wide("Segoe UI").as_ptr(),
            )
        };
        let mut app = Box::new(App {
            hwnd: 0,
            font: font(15, 400),
            heading_font: font(29, 600),
            dpi,
            settings: Settings::load(),
            title: 0,
            intro: 0,
            mc_label: 0,
            minecraft: 0,
            browse: 0,
            name_label: 0,
            name: 0,
            client_label: 0,
            client: 0,
            choose_client: 0,
            automatic: 0,
            status: 0,
            progress: 0,
            play: 0,
            cancel: 0,
            folder: 0,
            log_button: 0,
            details: 0,
            output: 0,
            expanded: false,
            receiver: None,
            playing: false,
            closing: false,
            game_started: false,
            dispatching: false,
            startup_error,
            tail: String::new(),
            log: fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(flow::log_path())
                .ok(),
        });
        let name = wide("HorizonWin7Launcher");
        let class = Class {
            style: 3,
            procedure: Some(procedure),
            class_extra: 0,
            window_extra: 0,
            instance: GetModuleHandleW(std::ptr::null()),
            icon: LoadIconW(0, 32512usize as *const u16),
            cursor: LoadCursorW(0, 32512usize as *const u16),
            background: 16,
            menu: std::ptr::null(),
            name: name.as_ptr(),
        };
        RegisterClassW(&class);
        let area = work_area();
        let width = app.scale(680).min(area.right - area.left);
        let height = app.scale(475).min(area.bottom - area.top);
        let hwnd = CreateWindowExW(
            0x10000,
            name.as_ptr(),
            wide(tr("Horizon — Windows 7")).as_ptr(),
            0xcf0000,
            area.left + (area.right - area.left - width) / 2,
            area.top + (area.bottom - area.top - height) / 2,
            width,
            height,
            0,
            0,
            class.instance,
            &mut *app as *mut App as *mut c_void,
        );
        if hwnd == 0 {
            MessageBoxW(
                0,
                wide(tr("Could not open the Horizon launcher window.")).as_ptr(),
                wide(tr("Horizon")).as_ptr(),
                0x10,
            );
        } else {
            ShowWindow(hwnd, 5);
            let mut message: Message = std::mem::zeroed();
            while GetMessageW(&mut message, 0, 0, 0) > 0 {
                if IsDialogMessageW(hwnd, &message) == 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
        DeleteObject(app.font);
        DeleteObject(app.heading_font);
        if com >= 0 {
            CoUninitialize();
        }
    }
}
