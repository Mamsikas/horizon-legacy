//! Sampled CPU consumption, separate from wall time spent inside driver calls.
use std::time::Duration;
use windows::Win32::{
    Foundation::FILETIME,
    System::Threading::{
        GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetSystemTimes, GetThreadTimes,
    },
};
fn ticks(t: FILETIME) -> u64 {
    u64::from(t.dwLowDateTime) | (u64::from(t.dwHighDateTime) << 32)
}

/// Current render/game thread's kernel + user time. Five-second deltas tolerate
/// Win7's coarse accounting without adding GPU synchronization or frame queries.
pub fn thread_cpu_time() -> Option<Duration> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the pseudo handle refers to this live thread, output pointers are
    // initialized locals, and no handle ownership escapes the function.
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
        .ok()?;
    }
    Some(Duration::from_nanos(
        (ticks(kernel) + ticks(user)).saturating_mul(100),
    ))
}

/// Accumulated user + kernel CPU time across this process's threads.
pub fn process_cpu_time() -> Option<Duration> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: process pseudo handle and initialized local output storage.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
        .ok()?;
    }
    Some(Duration::from_nanos(
        (ticks(kernel) + ticks(user)).saturating_mul(100),
    ))
}

/// System (busy, total) CPU ticks. Windows kernel time already includes idle.
pub fn system_cpu_ticks() -> Option<(u64, u64)> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the API only writes these live local FILETIME values; supported
    // since XP SP1, without a PDH service or another profiling dependency.
    unsafe {
        GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()?;
    }
    let total = ticks(kernel) + ticks(user);
    Some((total.saturating_sub(ticks(idle)), total))
}
