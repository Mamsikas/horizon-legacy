//! Query the current stack's complete reservation without the Windows 8 API.
use std::{ffi::c_void, mem::MaybeUninit};
use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};

const UNAVAILABLE: &str = "GuestStackUnavailable: cannot inspect the calling thread stack";

fn region(address: *const c_void) -> Result<MEMORY_BASIC_INFORMATION, &'static str> {
    let mut info = MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
    // SAFETY: VirtualQuery inspects an address without dereferencing it. The
    // output points to suitably sized/aligned storage, initialized on success.
    let written = unsafe { VirtualQuery(address, info.as_mut_ptr(), size_of_val(&info)) };
    if written != size_of_val(&info) {
        return Err(UNAVAILABLE);
    }
    // SAFETY: the full structure was initialized by the successful query.
    Ok(unsafe { info.assume_init() })
}

pub(super) fn bounds() -> Result<(usize, usize), &'static str> {
    let marker = 0u8;
    let address = std::ptr::addr_of!(marker).cast::<c_void>();
    let current = region(address)?;
    // AllocationBase includes the reserved/guard pages below the committed
    // stack. StackLimit alone would report only the currently committed part.
    let low = current.AllocationBase as usize;
    let end = |r: MEMORY_BASIC_INFORMATION| {
        (r.BaseAddress as usize)
            .checked_add(r.RegionSize)
            .ok_or(UNAVAILABLE)
    };
    let mut high = end(current)?;
    if low == 0 || low > address as usize || high <= address as usize {
        return Err(UNAVAILABLE);
    }
    // Adjacent regions can have different protections but belong to the same
    // reservation. Stop at its boundary; no pages are read or committed.
    loop {
        let following = region(high as *const c_void)?;
        if following.AllocationBase as usize != low {
            break;
        }
        let next = end(following)?;
        if next <= high {
            return Err(UNAVAILABLE);
        }
        high = next;
    }
    Ok((low, high))
}

#[cfg(all(test, not(target_vendor = "win7")))]
mod tests {
    #[test]
    fn win7_query_matches_modern_limits_for_small_and_guest_stacks() {
        for reservation in [1 << 20, crate::GUEST_STACK_BYTES + crate::HOST_HEADROOM] {
            std::thread::Builder::new()
                .stack_size(reservation)
                .spawn(move || {
                    let actual = super::bounds().unwrap();
                    let (mut low, mut high) = (0, 0);
                    // SAFETY: both output pointers are initialized writable locals;
                    // this comparison runs only on the modern Windows CI runner.
                    unsafe { crate::get_current_thread_stack_limits(&mut low, &mut high) };
                    assert_eq!(actual, (low, high));
                    assert!(high - low >= reservation);
                    assert_eq!(crate::check_current().is_ok(), reservation > 1 << 20);
                })
                .unwrap()
                .join()
                .unwrap();
        }
    }
}
