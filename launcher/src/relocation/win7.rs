//! Stock Win7 has PowerShell 2, without Get-CimInstance or ConvertTo-Json. Toolhelp32
//! provides a local process snapshot without another executable or newer Windows APIs.
use std::ffi::c_void;
use std::io;

#[repr(C)]
struct Entry {
    size: u32,
    usage: u32,
    process: u32,
    default_heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    priority: i32,
    flags: u32,
    executable: [u16; 260],
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> *mut c_void;
    fn Process32FirstW(snapshot: *mut c_void, entry: *mut Entry) -> i32;
    fn Process32NextW(snapshot: *mut c_void, entry: *mut Entry) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

struct Snapshot(*mut c_void);
impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns a successful Toolhelp snapshot handle.
        unsafe { CloseHandle(self.0) };
    }
}

pub(super) fn processes() -> io::Result<Vec<(String, String)>> {
    // SAFETY: TH32CS_SNAPPROCESS takes no pointer arguments and ignores the process id.
    let snapshot = unsafe { CreateToolhelp32Snapshot(2, 0) };
    if snapshot as isize == -1 || snapshot.is_null() {
        return Err(io::Error::last_os_error());
    }
    let snapshot = Snapshot(snapshot);
    // SAFETY: every field is an integer; zero initialization is valid before setting dwSize.
    let mut entry: Entry = unsafe { std::mem::zeroed() };
    entry.size = std::mem::size_of::<Entry>() as u32;
    let mut entries = Vec::new();
    // SAFETY: snapshot remains live and entry has the documented PROCESSENTRY32W layout/size.
    let mut found = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    loop {
        if found == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(18) {
                // ERROR_NO_MORE_FILES
                return Err(error);
            }
            break;
        }
        let end = entry
            .executable
            .iter()
            .position(|&u| u == 0)
            .unwrap_or(entry.executable.len());
        let name = String::from_utf16_lossy(&entry.executable[..end]);
        // No command line is fabricated. The common legacy guard conservatively treats
        // Java/build tool names as busy even when their managed-root arguments are unknown.
        entries.push((name, String::new()));
        // SAFETY: the same snapshot and live writable entry are used throughout iteration.
        found = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(entries)
}
