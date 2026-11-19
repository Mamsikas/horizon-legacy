//! The standalone Win7 launcher. The modern GUI and its account flow are unchanged.
pub mod flow;
#[cfg(target_arch = "x86")]
mod resource_search;
#[cfg(windows)]
pub mod window;
