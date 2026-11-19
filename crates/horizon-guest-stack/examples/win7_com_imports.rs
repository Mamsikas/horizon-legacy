//! Small, non-shipping import-policy regression probe, not a graphics client.
#[cfg(windows)]
fn main() {
    use windows_sys::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree, CoTaskMemRealloc};
    // SAFETY: each successful allocation is freed by its matching COM allocator.
    unsafe {
        let first = CoTaskMemAlloc(16);
        assert!(!first.is_null());
        first.cast::<u8>().write(42);
        let second = CoTaskMemRealloc(first, 32);
        if second.is_null() {
            CoTaskMemFree(first);
            panic!("COM reallocation failed");
        }
        assert_eq!(second.cast::<u8>().read(), 42);
        CoTaskMemFree(second);
    }
}
#[cfg(not(windows))]
fn main() {}
