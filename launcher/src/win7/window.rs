//! USER32 controls and GDI text. No GPU device, compositor, webview or rendering thread.
use super::catalog::{self, Action};
use super::worker::{Event, Tail, Worker};
use horizon_launcher::account::{SignIn, SignInState};
use horizon_launcher::i18n::tr;
use std::ffi::c_void;

type Handle = isize;
const CHILD: u32 = 0x40000000;
const VISIBLE: u32 = 0x10000000;
const TABSTOP: u32 = 0x10000;
const BORDER: u32 = 0x800000;
const MULTILINE: u32 = 4 | 0x40 | 0x1000 | 0x200000;
const READONLY: u32 = 0x800;
const ID_CATEGORY: usize = 100;
const ID_ACTION: usize = 101;
const ID_RUN: usize = 102;
const ID_CANCEL: usize = 103;
const ID_SIGN_IN: usize = 104;
const ID_COPY: usize = 105;
const ID_OPEN: usize = 106;
const ID_FORM: usize = 107;
const ID_OUTPUT: usize = 108;
const ID_CONSOLE_INPUT: usize = 109;
const ID_SEND: usize = 110;
const WM_COMMAND: u32 = 0x111;

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
struct ScrollInfo {
    size: u32,
    mask: u32,
    min: i32,
    max: i32,
    page: u32,
    position: i32,
    track: i32,
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
    fn GetClientRect(hwnd: Handle, rect: *mut Rect) -> i32;
    fn SetWindowTextW(hwnd: Handle, text: *const u16) -> i32;
    fn GetWindowTextLengthW(hwnd: Handle) -> i32;
    fn GetWindowTextW(hwnd: Handle, text: *mut u16, count: i32) -> i32;
    fn SendMessageW(hwnd: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    fn SetTimer(hwnd: Handle, id: usize, ms: u32, proc: *const c_void) -> usize;
    fn KillTimer(hwnd: Handle, id: usize) -> i32;
    fn EnableWindow(hwnd: Handle, enabled: i32) -> i32;
    fn SetFocus(hwnd: Handle) -> Handle;
    fn GetParent(hwnd: Handle) -> Handle;
    fn GetDC(hwnd: Handle) -> Handle;
    fn ReleaseDC(hwnd: Handle, dc: Handle) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
    fn MessageBoxW(hwnd: Handle, text: *const u16, title: *const u16, kind: u32) -> i32;
    fn SetScrollInfo(hwnd: Handle, bar: i32, info: *const ScrollInfo, redraw: i32) -> i32;
    fn GetScrollInfo(hwnd: Handle, bar: i32, info: *mut ScrollInfo) -> i32;
    fn SetProcessDPIAware() -> i32;
    fn SystemParametersInfoW(action: u32, parameter: u32, value: *mut c_void, flags: u32) -> i32;
    fn OpenClipboard(hwnd: Handle) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: Handle) -> Handle;
    fn CloseClipboard() -> i32;
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
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalLock(memory: Handle) -> *mut c_void;
    fn GlobalUnlock(memory: Handle) -> i32;
    fn GlobalFree(memory: Handle) -> Handle;
}
#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: Handle,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
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
        let mut buffer = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let count = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..count.max(0) as usize])
    }
}
fn send(hwnd: Handle, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe { SendMessageW(hwnd, msg, wp, lp) }
}
fn checked(hwnd: Handle) -> bool {
    send(hwnd, 0xf0, 0, 0) == 1
}

struct FieldView {
    use_value: Handle,
    input: Handle,
    label: Handle,
    help: Handle,
    y: i32,
    height: i32,
}
struct App {
    hwnd: Handle,
    category: Handle,
    action: Handle,
    about: Handle,
    form: Handle,
    output: Handle,
    console_input: Handle,
    send_input: Handle,
    status: Handle,
    run: Handle,
    cancel: Handle,
    sign_in_button: Handle,
    code: Handle,
    copy: Handle,
    open: Handle,
    font: Handle,
    dpi: i32,
    actions: Vec<Action>,
    categories: Vec<String>,
    shown: Vec<usize>,
    fields: Vec<FieldView>,
    scroll: i32,
    extent: i32,
    worker: Option<Worker>,
    sign_in: Option<SignIn>,
    url: String,
    user_code: String,
    log: Tail,
    handoff: Option<serde_json::Value>,
}

impl App {
    fn scale(&self, value: i32) -> i32 {
        value * self.dpi / 96
    }
    fn control(&self, parent: Handle, class: &str, label: &str, style: u32, id: usize) -> Handle {
        unsafe {
            let handle = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(label).as_ptr(),
                CHILD | VISIBLE | style,
                0,
                0,
                10,
                10,
                parent,
                id as isize,
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null_mut(),
            );
            send(handle, 0x30, self.font as usize, 1);
            handle
        }
    }
    fn button(&self, label: &str, id: usize) -> Handle {
        self.control(self.hwnd, "BUTTON", tr(label), TABSTOP, id)
    }
    fn init(&mut self) {
        self.category = self.control(
            self.hwnd,
            "COMBOBOX",
            "",
            TABSTOP | 3 | 0x200000,
            ID_CATEGORY,
        );
        self.action = self.control(self.hwnd, "COMBOBOX", "", TABSTOP | 3 | 0x200000, ID_ACTION);
        self.about = self.control(self.hwnd, "EDIT", "", READONLY | MULTILINE, 0);
        self.form = self.control(
            self.hwnd,
            "HorizonWin7Form",
            "",
            0x200000 | 0x10000,
            ID_FORM,
        );
        // WS_EX_CONTROLPARENT permits tab navigation through the form's child controls.
        unsafe {
            SetWindowLongPtrW(self.form, -20, 0x10000);
        }
        self.run = self.button("Run", ID_RUN);
        self.cancel = self.button("Cancel", ID_CANCEL);
        self.sign_in_button = self.button("Sign in with Microsoft", ID_SIGN_IN);
        self.code = self.control(self.hwnd, "EDIT", "", TABSTOP | READONLY | BORDER, 0);
        self.copy = self.button("Copy code", ID_COPY);
        self.open = self.button("Open browser", ID_OPEN);
        self.status = self.control(self.hwnd, "STATIC", tr("Ready"), 0, 0);
        self.output = self.control(
            self.hwnd,
            "EDIT",
            "",
            TABSTOP | READONLY | BORDER | MULTILINE,
            ID_OUTPUT,
        );
        send(self.output, 0xc5, 256 * 1024, 0); // EM_SETLIMITTEXT
        self.console_input = self.control(
            self.hwnd,
            "EDIT",
            "",
            TABSTOP | BORDER | 0x80,
            ID_CONSOLE_INPUT,
        );
        send(self.console_input, 0xc5, 4096, 0);
        self.send_input = self.button("Send", ID_SEND);
        for action in &self.actions {
            let category = action.path[0].clone();
            if !self.categories.contains(&category) {
                self.categories.push(category);
            }
        }
        for category in &self.categories {
            send(
                self.category,
                0x143,
                0,
                wide(tr(category)).as_ptr() as isize,
            );
        }
        send(self.category, 0x14e, 0, 0);
        self.select_category();
        self.idle();
    }
    fn selected(&self) -> Option<usize> {
        self.shown
            .get(send(self.action, 0x147, 0, 0) as usize)
            .copied()
    }
    fn select_category(&mut self) {
        let category = send(self.category, 0x147, 0, 0) as usize;
        self.shown = self
            .actions
            .iter()
            .enumerate()
            .filter(|(_, a)| self.categories.get(category) == a.path.first())
            .map(|(i, _)| i)
            .collect();
        send(self.action, 0x14b, 0, 0);
        for &index in &self.shown {
            send(
                self.action,
                0x143,
                0,
                wide(tr(&self.actions[index].path.join(" "))).as_ptr() as isize,
            );
        }
        send(self.action, 0x14e, 0, 0);
        self.select_action();
    }
    fn select_action(&mut self) {
        for field in self.fields.drain(..) {
            for handle in [field.use_value, field.input, field.label, field.help] {
                if handle != 0 {
                    unsafe {
                        DestroyWindow(handle);
                    }
                }
            }
        }
        self.scroll = 0;
        let Some(index) = self.selected() else { return };
        let action = &self.actions[index];
        set(self.about, tr(&action.about));
        let mut y = 8;
        for (i, field) in action.fields.iter().enumerate() {
            let base = 1000 + i * 4;
            let use_value = if field.required && !field.flag {
                0
            } else {
                self.control(
                    self.form,
                    "BUTTON",
                    if field.flag {
                        tr(&field.label)
                    } else {
                        tr("Use")
                    },
                    TABSTOP | 3,
                    base,
                )
            };
            let label = if field.flag {
                0
            } else {
                self.control(self.form, "STATIC", tr(&field.label), 0, base + 1)
            };
            let height = if field.multiple { 56 } else { 24 };
            let input = if field.flag {
                0
            } else if !field.choices.is_empty() {
                let input =
                    self.control(self.form, "COMBOBOX", "", TABSTOP | 3 | 0x200000, base + 2);
                for choice in &field.choices {
                    send(input, 0x143, 0, wide(choice).as_ptr() as isize);
                }
                let selected = field
                    .choices
                    .iter()
                    .position(|v| field.defaults.first() == Some(v))
                    .unwrap_or(0);
                send(input, 0x14e, selected, 0);
                input
            } else {
                self.control(
                    self.form,
                    "EDIT",
                    &field.defaults.join("\r\n"),
                    TABSTOP | BORDER | if field.multiple { MULTILINE } else { 0x80 },
                    base + 2,
                )
            };
            let help_text = if field.multiple {
                format!(
                    "{}\r\n{}",
                    tr(&field.help),
                    tr("One argument per line; spaces are kept.")
                )
            } else {
                tr(&field.help).to_owned()
            };
            let help = self.control(
                self.form,
                "EDIT",
                &help_text,
                READONLY | MULTILINE,
                base + 3,
            );
            self.fields.push(FieldView {
                use_value,
                input,
                label,
                help,
                y,
                height,
            });
            y += height + 64;
        }
        self.extent = y;
        self.layout();
    }
    fn place(&self, handle: Handle, x: i32, y: i32, w: i32, h: i32) {
        if handle != 0 {
            unsafe {
                MoveWindow(
                    handle,
                    self.scale(x),
                    self.scale(y),
                    self.scale(w.max(1)),
                    self.scale(h.max(1)),
                    1,
                );
            }
        }
    }
    fn layout(&mut self) {
        let mut rect = Rect::default();
        unsafe {
            GetClientRect(self.hwnd, &mut rect);
        }
        let w = rect.right * 96 / self.dpi;
        let h = rect.bottom * 96 / self.dpi;
        self.place(self.sign_in_button, 8, 8, 180, 26);
        self.place(self.code, 196, 8, (w - 420).max(80), 26);
        self.place(self.copy, w - 212, 8, 96, 26);
        self.place(self.open, w - 108, 8, 100, 26);
        self.place(self.category, 8, 42, 180, 300);
        self.place(self.action, 196, 42, w - 204, 400);
        self.place(self.about, 8, 74, w - 16, 54);
        let log_height = (h / 4).clamp(70, 170);
        let footer = h - log_height - 44;
        self.place(self.form, 8, 132, w - 16, footer - 140);
        self.place(self.run, 8, footer, 112, 28);
        self.place(self.cancel, 128, footer, 112, 28);
        self.place(self.status, 248, footer + 4, w - 256, 24);
        self.place(self.output, 8, footer + 36, w - 16, log_height - 30);
        self.place(self.console_input, 8, h - 32, w - 104, 24);
        self.place(self.send_input, w - 88, h - 32, 80, 24);
        self.layout_fields();
    }
    fn layout_fields(&mut self) {
        let mut rect = Rect::default();
        unsafe {
            GetClientRect(self.form, &mut rect);
        }
        let w = rect.right * 96 / self.dpi;
        let page = (rect.bottom * 96 / self.dpi).max(1);
        self.scroll = self.scroll.clamp(0, (self.extent - page).max(0));
        let info = ScrollInfo {
            size: std::mem::size_of::<ScrollInfo>() as u32,
            mask: 7,
            min: 0,
            max: self.extent,
            page: page as u32,
            position: self.scroll,
            track: 0,
        };
        unsafe {
            SetScrollInfo(self.form, 1, &info, 1);
        }
        for (i, field) in self.fields.iter().enumerate() {
            let y = field.y - self.scroll;
            if field.input == 0 {
                self.place(field.use_value, 8, y, w - 16, 24);
            } else {
                self.place(field.use_value, 8, y, 56, 24);
                self.place(field.label, 68, y + 3, 155, 24);
                self.place(
                    field.input,
                    228,
                    y,
                    w - 240,
                    if field.height == 24 {
                        200
                    } else {
                        field.height
                    },
                );
                // Combo dropdown needs tall bounds; edits must retain their row height.
                let Some(index) = self.selected() else {
                    continue;
                };
                if self.actions[index].fields[i].choices.is_empty() {
                    self.place(field.input, 228, y, w - 240, field.height);
                }
            }
            self.place(field.help, 68, y + field.height + 4, w - 80, 52);
        }
    }
    fn scroll(&mut self, code: usize, wheel: i32) {
        let mut info = ScrollInfo {
            size: std::mem::size_of::<ScrollInfo>() as u32,
            mask: 0x17,
            min: 0,
            max: 0,
            page: 0,
            position: 0,
            track: 0,
        };
        unsafe {
            GetScrollInfo(self.form, 1, &mut info);
        }
        self.scroll = match code {
            0 => self.scroll - 32,
            1 => self.scroll + 32,
            2 => self.scroll - info.page as i32,
            3 => self.scroll + info.page as i32,
            4 | 5 => info.track,
            6 => 0,
            7 => self.extent,
            _ => self.scroll - wheel * 48 / 120,
        };
        self.layout_fields();
    }
    fn append(&mut self, line: String) {
        self.log.push(line);
    }
    fn flush(&self) {
        set(self.output, &self.log.text());
        send(self.output, 0xb1, usize::MAX, -1);
        send(self.output, 0xb7, 0, 0);
    }
    fn start(&mut self) -> Result<(), String> {
        if self.worker.is_some() || self.sign_in.is_some() {
            return Ok(());
        }
        let index = self
            .selected()
            .ok_or_else(|| tr("Select an action.").to_owned())?;
        let action = &self.actions[index];
        let inputs = self
            .fields
            .iter()
            .zip(&action.fields)
            .map(|(view, field)| {
                if field.flag {
                    return checked(view.use_value).then(|| "true".to_owned());
                }
                if field.required || checked(view.use_value) {
                    Some(text(view.input))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let arguments = action.arguments(&inputs)?;
        // The action's help explains trust and deletion flags; no flag is pre-consented.
        self.worker = Some(Worker::start(&arguments)?);
        self.handoff = None;
        self.append(action.path.join(" "));
        self.flush();
        set(self.status, tr("Working…"));
        self.busy();
        Ok(())
    }
    fn busy(&self) {
        unsafe {
            EnableWindow(self.run, 0);
            EnableWindow(self.sign_in_button, 0);
            EnableWindow(self.cancel, 1);
            EnableWindow(self.send_input, i32::from(self.worker.is_some()));
            SetTimer(self.hwnd, 1, 150, std::ptr::null());
        }
    }
    fn idle(&self) {
        unsafe {
            EnableWindow(self.run, 1);
            EnableWindow(self.sign_in_button, 1);
            EnableWindow(self.cancel, 0);
            EnableWindow(self.send_input, 0);
            KillTimer(self.hwnd, 1);
        }
    }
    fn poll(&mut self) {
        let mut lines = Vec::new();
        let mut finished = None;
        if let Some(worker) = &mut self.worker {
            for event in worker.events.try_iter().take(256) {
                match event {
                    Event::Line(line) => lines.push(line),
                }
            }
            if let Some(result) = worker.poll() {
                // Pipe readers may still have their last buffered lines. Wait for disconnect.
                match worker.events.try_recv() {
                    Ok(Event::Line(line)) => lines.push(line),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => finished = Some(result),
                    _ => {}
                }
            }
        }
        for line in lines {
            if let Some(value) = line
                .strip_prefix("HORIZON_NATIVE_HANDOFF\t")
                .filter(|_| self.worker.as_ref().is_some_and(|w| w.handoff_allowed))
            {
                self.handoff = serde_json::from_str(value).ok();
            } else if let Some(progress) = line.strip_prefix("HORIZON_PROGRESS\t") {
                set(self.status, progress);
            } else {
                self.append(line);
            }
        }
        if let Some(result) = finished {
            self.worker = None;
            match result {
                Ok(()) => {
                    if let Some(handoff) = self.handoff.take() {
                        if let Err(error) = hand_over(handoff) {
                            self.append(error);
                        } else {
                            unsafe {
                                DestroyWindow(self.hwnd);
                            }
                            return;
                        }
                    }
                    set(self.status, tr("Completed"));
                    self.append(tr("Completed").to_owned());
                }
                Err(e) => {
                    set(self.status, tr("Failed"));
                    self.append(e);
                }
            }
            self.idle();
        }
        if let Some(sign_in) = &self.sign_in {
            match sign_in.state() {
                SignInState::Code(code) => {
                    self.url = code.verification_uri;
                    self.user_code = code.user_code;
                    set(self.code, &format!("{}   {}", self.user_code, self.url));
                }
                SignInState::Starting => set(self.status, tr("Signing in…")),
                SignInState::Working(phase) => set(self.status, tr(phase.label())),
                SignInState::Done(result) => {
                    self.sign_in = None;
                    let line = match result {
                        Ok(account) => account.name,
                        Err(error) => error,
                    };
                    self.append(line);
                    self.idle();
                    set(self.status, tr("Ready"));
                }
            }
        }
        self.flush();
    }
    fn command(&mut self, id: usize, notification: usize) -> Result<(), String> {
        // EN_SETFOCUS / CBN_SETFOCUS: keyboard navigation must reveal offscreen fields.
        if id >= 1000 && matches!(notification, 0x100 | 3) {
            if let Some(field) = self.fields.get((id - 1000) / 4) {
                let mut rect = Rect::default();
                unsafe {
                    GetClientRect(self.form, &mut rect);
                }
                let page = rect.bottom * 96 / self.dpi;
                if field.y < self.scroll || field.y + field.height > self.scroll + page {
                    self.scroll = field.y;
                    self.layout_fields();
                }
            }
            return Ok(());
        }
        match (id, notification) {
            (ID_CATEGORY, 1) => self.select_category(),
            (ID_ACTION, 1) => self.select_action(),
            (ID_RUN, 0) => self.start()?,
            (ID_SEND, 0) => {
                if let Some(worker) = &self.worker {
                    worker.send(text(self.console_input))?;
                }
                set(self.console_input, "");
            }
            (ID_CANCEL, 0) => {
                if let Some(sign_in) = &self.sign_in {
                    sign_in.cancel();
                }
                if let Some(worker) = &self.worker {
                    worker.cancel()?;
                }
                set(
                    self.status,
                    tr("Cancellation requested; waiting for a safe boundary."),
                );
            }
            (ID_SIGN_IN, 0) => {
                if self.worker.is_none() && self.sign_in.is_none() {
                    self.sign_in = Some(SignIn::start());
                    self.busy();
                }
            }
            (ID_COPY, 0) => clipboard(self.hwnd, &self.user_code)?,
            _ => {}
        }
        Ok(())
    }
}

fn clipboard(hwnd: Handle, text: &str) -> Result<(), String> {
    let data = wide(text);
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let memory = GlobalAlloc(2, data.len() * 2);
        let ptr = GlobalLock(memory);
        let ok = if ptr.is_null() {
            false
        } else {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.cast::<u16>(), data.len());
            GlobalUnlock(memory);
            EmptyClipboard() != 0 && SetClipboardData(13, memory) != 0
        };
        if !ok && memory != 0 {
            GlobalFree(memory);
        }
        CloseClipboard();
        if ok {
            Ok(())
        } else {
            Err(tr("Could not copy to the clipboard.").into())
        }
    }
}

// Only the three explicit storage forms can arm this handoff. A line in a game's output
// cannot request a relocation or restart. The existing worker owns migration and rollback.
fn hand_over(value: serde_json::Value) -> Result<(), String> {
    let mut command =
        horizon_launcher::process::command(std::env::current_exe().map_err(|e| e.to_string())?);
    if let Some(target) = value.get("relocate").and_then(serde_json::Value::as_str) {
        command.arg("--relocate-data").arg(target).arg("--restart");
    } else if value.get("restart").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).to_string());
    }
    horizon_launcher::update_handoff::forget();
    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

unsafe extern "system" fn form_proc(hwnd: Handle, msg: u32, wp: usize, lp: isize) -> isize {
    let parent = unsafe { GetParent(hwnd) };
    if msg == WM_COMMAND {
        return send(parent, msg, wp, lp);
    }
    if matches!(msg, 0x115 | 0x20a) {
        let ptr = unsafe { GetWindowLongPtrW(parent, -21) } as *mut App;
        if let Some(app) = unsafe { ptr.as_mut() } {
            app.scroll(
                if msg == 0x115 { wp & 0xffff } else { 99 },
                (wp >> 16) as i16 as i32,
            );
            return 0;
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

unsafe extern "system" fn window_proc(hwnd: Handle, msg: u32, wp: usize, lp: isize) -> isize {
    if msg == 0x81 {
        // WM_NCCREATE, App outlives the native message loop.
        let create = unsafe { &*(lp as *const Create) };
        unsafe {
            SetWindowLongPtrW(hwnd, -21, create.parameter as isize);
        }
        return 1;
    }
    if msg == 2 {
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    // Edit change notifications need no callback into App (including synchronous SetWindowText).
    if msg == WM_COMMAND && !matches!((wp >> 16) & 0xffff, 0 | 1 | 3 | 0x100) {
        return 0;
    }
    // Control creation/layout synchronously sends many other messages. Do not borrow App
    // during those reentrant notifications while its initiating method still has &mut self.
    if !matches!(msg, 5 | 0x113 | WM_COMMAND | 0x10) {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, -21) } as *mut App;
    if msg == WM_COMMAND && wp & 0xffff == ID_OPEN && wp >> 16 == 0 && !ptr.is_null() {
        // ShellExecute may run a nested message loop. Release the App borrow before calling it.
        let url = unsafe { &*ptr }.url.clone();
        if url.starts_with("https://")
            && unsafe {
                ShellExecuteW(
                    hwnd,
                    wide("open").as_ptr(),
                    wide(&url).as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                )
            } <= 32
        {
            let app = unsafe { &mut *ptr };
            app.append(tr("Could not open the browser. Copy the URL shown above.").into());
            app.flush();
        }
        return 0;
    }
    if let Some(app) = unsafe { ptr.as_mut() } {
        match msg {
            5 if app.form != 0 => {
                app.layout();
                return 0;
            }
            0x113 => {
                app.poll();
                return 0;
            }
            WM_COMMAND => {
                if let Err(error) = app.command(wp & 0xffff, (wp >> 16) & 0xffff) {
                    app.append(error);
                    app.flush();
                    set(app.status, tr("Failed"));
                }
                return 0;
            }
            0x10 => {
                if app.worker.is_some() || app.sign_in.is_some() {
                    app.append(tr("An operation is still running. Cancel it and wait for completion before closing.").to_owned());
                    app.flush();
                    return 0;
                }
                if let Some(sign_in) = &app.sign_in {
                    sign_in.cancel();
                }
                unsafe {
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

pub fn error(message: &str) {
    unsafe {
        MessageBoxW(
            0,
            wide(message).as_ptr(),
            wide(tr("Horizon Launcher")).as_ptr(),
            0x10,
        );
    }
}

pub fn run(location_error: Option<String>) -> Result<(), String> {
    let mut app = Box::new(App {
        hwnd: 0,
        category: 0,
        action: 0,
        about: 0,
        form: 0,
        output: 0,
        console_input: 0,
        send_input: 0,
        status: 0,
        run: 0,
        cancel: 0,
        sign_in_button: 0,
        code: 0,
        copy: 0,
        open: 0,
        font: 0,
        dpi: 96,
        actions: catalog::actions(),
        categories: Vec::new(),
        shown: Vec::new(),
        fields: Vec::new(),
        scroll: 0,
        extent: 0,
        worker: None,
        sign_in: None,
        url: String::new(),
        user_code: String::new(),
        log: Tail::new(),
        handoff: None,
    });
    unsafe {
        SetProcessDPIAware(); // Vista/Win7 API; no GetDpiForWindow/shcore import.
        let dc = GetDC(0);
        if dc != 0 {
            app.dpi = GetDeviceCaps(dc, 88).max(96);
            ReleaseDC(0, dc);
        }
        app.font = CreateFontW(
            -9 * app.dpi / 72,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            wide("Segoe UI").as_ptr(),
        );
        let instance = GetModuleHandleW(std::ptr::null());
        for (name, procedure) in [
            (
                "HorizonWin7",
                window_proc as unsafe extern "system" fn(_, _, _, _) -> _,
            ),
            (
                "HorizonWin7Form",
                form_proc as unsafe extern "system" fn(_, _, _, _) -> _,
            ),
        ] {
            let class_name = wide(name);
            let class = Class {
                style: 3,
                procedure: Some(procedure),
                class_extra: 0,
                window_extra: 0,
                instance,
                icon: LoadIconW(instance, 1usize as *const u16),
                cursor: LoadCursorW(0, 32512usize as *const u16),
                background: 16,
                menu: std::ptr::null(),
                name: class_name.as_ptr(),
            };
            if RegisterClassW(&class) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        let mut work_area = Rect::default();
        SystemParametersInfoW(0x30, 0, (&mut work_area as *mut Rect).cast(), 0);
        let width = if work_area.right > work_area.left {
            app.scale(900).min(work_area.right - work_area.left)
        } else {
            app.scale(900)
        };
        let height = if work_area.bottom > work_area.top {
            app.scale(600).min(work_area.bottom - work_area.top)
        } else {
            app.scale(600)
        };
        app.hwnd = CreateWindowExW(
            0x10000,
            wide("HorizonWin7").as_ptr(),
            wide(tr("Horizon Launcher — Windows 7 (experimental)")).as_ptr(),
            0xcf0000,
            i32::MIN,
            i32::MIN,
            width,
            height,
            0,
            0,
            instance,
            (&mut *app as *mut App).cast(),
        );
        if app.hwnd == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        app.init();
        SetFocus(app.category);
        if let Some(error) = location_error {
            app.append(error);
        }
        if let Some(error) = horizon_launcher::update_handoff::take_failure() {
            app.append(error);
        }
        app.append(tr("Experimental Win7 build: hardware validation and autonomous first installation are pending.").to_owned());
        if horizon_launcher::account::client_id().is_none() {
            app.append(tr(horizon_launcher::account::NOT_CONFIGURED).to_owned());
        }
        app.flush();
        ShowWindow(app.hwnd, 5);
        let mut message: Message = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut message, 0, 0, 0);
            if result == 0 {
                break;
            }
            if result == -1 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if IsDialogMessageW(app.hwnd, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if app.font != 0 {
            DeleteObject(app.font);
        }
    }
    horizon_launcher::account::discard_unread_sessions();
    Ok(())
}
