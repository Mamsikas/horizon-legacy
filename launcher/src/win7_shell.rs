//! Shell links through Win7's ole32 COM ABI. The modern UI retains its windows-rs wrapper;
//! its WinRT error/string support would otherwise import combase.dll into the legacy launcher.
use crate::i18n::{tr, trf};
use std::ffi::{c_void, OsStr};
use std::fs;
use std::os::windows::ffi::OsStrExt;
#[cfg(test)]
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::core::GUID;

// Bind ole32 directly. windows-sys 0.61's COM module also creates a combase import
// library; its CoTaskMemFree symbol can capture older dependencies' ole32 calls.
#[link(name = "ole32")]
extern "system" {
    fn CoCreateInstance(
        class: *const GUID,
        outer: *mut c_void,
        context: u32,
        interface: *const GUID,
        result: *mut *mut c_void,
    ) -> i32;
    fn CoInitializeEx(reserved: *const c_void, model: u32) -> i32;
    fn CoUninitialize();
}
const CLSCTX_INPROC_SERVER: u32 = 1;
const COINIT_APARTMENTTHREADED: u32 = 2;

const SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const SHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const PERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);
const RPC_E_CHANGED_MODE: i32 = 0x80010106u32 as i32;

type SetText = unsafe extern "system" fn(*mut c_void, *const u16) -> i32;

// IShellLinkW and IPersistFile have the published IUnknown prefix. Unused slots retain
// pointer-sized fields, so the called methods have their documented COM vtable offsets.
#[repr(C)]
struct Unknown {
    query: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    _add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct ShellLink {
    unknown: Unknown,
    #[cfg_attr(not(test), allow(dead_code))]
    get_path: unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut c_void, u32) -> i32,
    _get_id_list: usize,
    _set_id_list: usize,
    _get_description: usize,
    set_description: SetText,
    _get_working_directory: usize,
    set_working_directory: SetText,
    _get_arguments: usize,
    set_arguments: SetText,
    _get_hotkey: usize,
    _set_hotkey: usize,
    _get_show_command: usize,
    _set_show_command: usize,
    _get_icon_location: usize,
    set_icon_location: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32,
    _set_relative_path: usize,
    _resolve: usize,
    set_path: SetText,
}

#[repr(C)]
struct PersistFile {
    unknown: Unknown,
    _get_class_id: usize,
    _is_dirty: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    load: unsafe extern "system" fn(*mut c_void, *const u16, u32) -> i32,
    save: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32,
}

struct Apartment(bool);

impl Apartment {
    fn open() -> Result<Self, String> {
        // SAFETY: no reserved parameter and a documented apartment flag; only a successful
        // initialization is balanced by this guard. An existing other apartment stays intact.
        let result = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        if result != RPC_E_CHANGED_MODE {
            check(result)?;
        }
        Ok(Self(result >= 0))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: this thread successfully initialized COM in Apartment::open.
            unsafe { CoUninitialize() };
        }
    }
}

struct Object(*mut c_void);

impl Object {
    fn shell_link() -> Result<Self, String> {
        let mut pointer = null_mut();
        // SAFETY: the CLSID/IID pair creates IShellLinkW and writes one owned COM reference.
        check(unsafe {
            CoCreateInstance(
                &SHELL_LINK,
                null_mut(),
                CLSCTX_INPROC_SERVER,
                &SHELL_LINK_W,
                &mut pointer,
            )
        })?;
        Ok(Self(pointer))
    }

    fn persist_file(&self) -> Result<Self, String> {
        let mut pointer = null_mut();
        // SAFETY: self owns a live COM object; QueryInterface writes an owned IPersistFile.
        check(unsafe { (self.unknown().query)(self.0, &PERSIST_FILE, &mut pointer) })?;
        Ok(Self(pointer))
    }

    unsafe fn unknown(&self) -> &Unknown {
        // SAFETY: each owned COM interface starts with its IUnknown vtable pointer.
        unsafe { &**(self.0 as *const *const Unknown) }
    }

    unsafe fn shell(&self) -> &ShellLink {
        // SAFETY: only the IShellLinkW object returned by shell_link calls this method.
        unsafe { &**(self.0 as *const *const ShellLink) }
    }

    unsafe fn file(&self) -> &PersistFile {
        // SAFETY: only the IPersistFile object returned by persist_file calls this method.
        unsafe { &**(self.0 as *const *const PersistFile) }
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        // SAFETY: the object owns exactly one reference acquired by COM.
        unsafe { (self.unknown().release)(self.0) };
    }
}

fn check(result: i32) -> Result<(), String> {
    if result < 0 {
        Err(trf(
            "Shell link failed (HRESULT {code})",
            &[("code", &format!("0x{:08X}", result as u32))],
        ))
    } else {
        Ok(())
    }
}

fn wide(text: &OsStr) -> Result<Vec<u16>, String> {
    let mut units: Vec<_> = text.encode_wide().collect();
    if units.contains(&0) {
        return Err(tr("Shell link text contains a NUL character").into());
    }
    units.push(0);
    Ok(units)
}

/// Win7's shell owns the recycle operation and its confirmation dialogs. In particular we
/// retain its warning if a drive cannot recycle a file, rather than silently deleting it.
pub(crate) fn recycle(path: &Path) -> Result<(), String> {
    #[repr(C)]
    struct FileOperation {
        window: *mut c_void,
        operation: u32,
        from: *const u16,
        to: *const u16,
        flags: u16,
        aborted: i32,
        mappings: *mut c_void,
        title: *const u16,
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHFileOperationW(operation: *mut FileOperation) -> i32;
    }
    let absolute = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // SHFileOperationW cannot accept extended-length paths. Refuse these instead of handing
    // a truncated path to the shell; callers retain the original world or other player file.
    let text: Vec<_> = absolute.as_os_str().encode_wide().collect();
    let text = text.strip_prefix(&[92, 92, 63, 92]).unwrap_or(&text);
    if text.len() >= 260 || text.starts_with(&[85, 78, 67, 92]) {
        return Err(trf(
            "{path}: the Windows 7 recycle bin cannot handle this path; move it to a shorter local path first",
            &[("path", &path.display().to_string())],
        ));
    }
    let mut from = text.to_vec();
    from.push(0);
    from.push(0); // SHFileOperation takes a double-NUL-terminated path list.
    let mut operation = FileOperation {
        window: null_mut(),
        operation: 3, // FO_DELETE
        from: from.as_ptr(),
        to: null(),
        flags: 0x0040 | 0x4000 | 0x0400, // ALLOWUNDO | WANTNUKEWARNING | NOERRORUI
        aborted: 0,
        mappings: null_mut(),
        title: null(),
    };
    // SAFETY: the published SHFILEOPSTRUCTW layout and terminated path list remain live
    // for the blocking call. No no-confirmation flag suppresses the destructive warning.
    let result = unsafe { SHFileOperationW(&mut operation) };
    match (result, operation.aborted) {
        (0, 0) => Ok(()),
        (0, _) => Err(tr("Moving to the recycle bin was cancelled").into()),
        (code, _) => Err(trf(
            "Could not recycle {path} (shell error {code})",
            &[
                ("path", &path.display().to_string()),
                ("code", &code.to_string()),
            ],
        )),
    }
}

pub(crate) fn write(path: &Path, exe: &Path, args: &str, comment: &str) -> Result<(), String> {
    let make = || {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let _apartment = Apartment::open()?;
        let link = Object::shell_link()?;
        let exe_wide = wide(exe.as_os_str())?;
        let args = wide(OsStr::new(args))?;
        let comment = wide(OsStr::new(comment))?;
        let path = wide(path.as_os_str())?;
        // SAFETY: link is IShellLinkW; all string buffers are terminated and remain alive
        // throughout each call. IPersistFile is released before the COM apartment guard.
        unsafe {
            let shell = link.shell();
            check((shell.set_path)(link.0, exe_wide.as_ptr()))?;
            check((shell.set_arguments)(link.0, args.as_ptr()))?;
            if let Some(dir) = exe.parent() {
                let dir = wide(dir.as_os_str())?;
                check((shell.set_working_directory)(link.0, dir.as_ptr()))?;
            }
            check((shell.set_icon_location)(link.0, exe_wide.as_ptr(), 0))?;
            check((shell.set_description)(link.0, comment.as_ptr()))?;
            let file = link.persist_file()?;
            check((file.file().save)(file.0, path.as_ptr(), 1))
        }
    };
    make().map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
pub(crate) fn target(path: &Path) -> Result<std::path::PathBuf, String> {
    let _apartment = Apartment::open()?;
    let link = Object::shell_link()?;
    let path = wide(path.as_os_str())?;
    // SAFETY: each interface has the queried IID and buffers are sized for their calls.
    unsafe {
        let file = link.persist_file()?;
        check((file.file().load)(file.0, path.as_ptr(), 0))?;
        let mut buffer = [0u16; 32768];
        check((link.shell().get_path)(
            link.0,
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            null_mut(),
            0,
        ))?;
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Ok(std::ffi::OsString::from_wide(&buffer[..end]).into())
    }
}
