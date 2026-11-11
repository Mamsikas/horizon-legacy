//! Win7 text clipboard shim. arboard's file-list implementation requires Win8
//! PathCchStripPrefix, although the game only requests Unicode text.
use std::{marker::PhantomData, rc::Rc};
use windows::{
    Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
        System::{DataExchange::*, Memory::*},
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
const UNICODE_TEXT: u32 = 13;
#[derive(Debug)]
pub enum Error {
    ContentNotAvailable,
    Win32,
}
impl From<windows::core::Error> for Error {
    fn from(_: windows::core::Error) -> Self {
        Self::Win32
    }
}
/// This hidden HWND and its clipboard calls belong to their creating thread.
pub struct Clipboard {
    owner: HWND,
    _thread: PhantomData<Rc<()>>,
}
struct Open;
impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: Open exists only after a successful OpenClipboard on this thread.
        let _ = unsafe { CloseClipboard() };
    }
}
struct Locked(HGLOBAL);
impl Drop for Locked {
    fn drop(&mut self) {
        // SAFETY: the matching successful GlobalLock remains held by this guard.
        let _ = unsafe { GlobalUnlock(self.0) };
    }
}
struct Allocation(Option<HGLOBAL>);
impl Drop for Allocation {
    fn drop(&mut self) {
        if let Some(handle) = self.0 {
            // SAFETY: this allocation has not been handed to SetClipboardData.
            let _ = unsafe { GlobalFree(handle) };
        }
    }
}
impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        // SAFETY: STATIC is a system window class; no callbacks or borrowed user data.
        // A real owner HWND is required for EmptyClipboard/SetClipboardData.
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                None,
                None,
                None,
            )?
        };
        Ok(Self {
            owner,
            _thread: PhantomData,
        })
    }
    fn open(&self) -> Result<Open, Error> {
        // SAFETY: owner is alive on this thread; the guard always closes the clipboard.
        unsafe {
            OpenClipboard(self.owner)?;
        }
        Ok(Open)
    }
    pub fn get_text(&mut self) -> Result<String, Error> {
        let _open = self.open()?;
        // SAFETY: format probing and the borrowed global handle are protected by Open.
        unsafe {
            if IsClipboardFormatAvailable(UNICODE_TEXT).is_err() {
                return Err(Error::ContentNotAvailable);
            }
            let handle = HGLOBAL(GetClipboardData(UNICODE_TEXT)?.0);
            let bytes = GlobalSize(handle);
            if bytes < 2 || bytes > isize::MAX as usize {
                return Err(Error::ContentNotAvailable);
            }
            let pointer = GlobalLock(handle);
            if pointer.is_null() {
                return Err(Error::Win32);
            }
            let _locked = Locked(handle);
            let text = std::slice::from_raw_parts(pointer.cast::<u16>(), bytes / 2);
            let length = text.iter().position(|&c| c == 0).unwrap_or(text.len());
            Ok(String::from_utf16_lossy(&text[..length]))
        }
    }
    pub fn set_text(&mut self, text: String) -> Result<(), Error> {
        let mut utf16: Vec<u16> = text.encode_utf16().collect();
        utf16.push(0);
        let _open = self.open()?;
        // SAFETY: bounded matching allocation/copy; ownership transfers only on success.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, utf16.len() * 2)?;
            let mut allocation = Allocation(Some(handle));
            let pointer = GlobalLock(handle);
            if pointer.is_null() {
                return Err(Error::Win32);
            }
            {
                let _locked = Locked(handle);
                std::ptr::copy_nonoverlapping(utf16.as_ptr(), pointer.cast::<u16>(), utf16.len());
            }
            EmptyClipboard()?;
            SetClipboardData(UNICODE_TEXT, HANDLE(handle.0))?;
            allocation.0 = None;
        }
        Ok(())
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: !Send/!Sync keeps the owned HWND on its creating thread.
        let _ = unsafe { DestroyWindow(self.owner) };
    }
}
