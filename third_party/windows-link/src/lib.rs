#![doc = include_str!("../readme.md")]
#![no_std]

#[doc(hidden)]
#[cfg(all(windows, target_arch = "x86"))]
#[macro_export]
macro_rules! __link {
    ($library:literal $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        #[link(name = $library, kind = "raw-dylib", modifiers = "+verbatim", import_name_type = "undecorated")]
        extern $abi {
            $(#[link_name=$link_name])?
            pub fn $($function)*;
        }
    )
}

#[doc(hidden)]
#[cfg(all(windows, not(target_arch = "x86")))]
#[macro_export]
macro_rules! __link {
    ($library:literal $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        #[link(name = $library, kind = "raw-dylib", modifiers = "+verbatim")]
        extern $abi {
            $(#[link_name=$link_name])?
            pub fn $($function)*;
        }
    )
}

#[doc(hidden)]
#[cfg(not(windows))]
#[macro_export]
macro_rules! __link {
    ($library:literal $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        extern $abi {
            pub fn $($function)*;
        }
    )
}

// Only the historical Win7 target redirects these classic COM allocation APIs.
// They exist in Ole32 on Win7; combase starts with Win8. Other COM/WinRT imports
// retain their original library and are still checked by the package audit.
#[cfg(all(windows, target_vendor = "win7"))]
#[macro_export]
macro_rules! link {
    ("combase.dll" $abi:literal fn CoTaskMemAlloc $($signature:tt)*) => (
        $crate::__link!("ole32.dll" $abi fn CoTaskMemAlloc $($signature)*);
    );
    ("combase.dll" $abi:literal fn CoTaskMemFree $($signature:tt)*) => (
        $crate::__link!("ole32.dll" $abi fn CoTaskMemFree $($signature)*);
    );
    ("combase.dll" $abi:literal fn CoTaskMemRealloc $($signature:tt)*) => (
        $crate::__link!("ole32.dll" $abi fn CoTaskMemRealloc $($signature)*);
    );
    ($($signature:tt)*) => ($crate::__link!($($signature)*);)
}

/// Defines an external function to import.
#[cfg(not(all(windows, target_vendor = "win7")))]
#[macro_export]
macro_rules! link {
    ($($signature:tt)*) => ($crate::__link!($($signature)*);)
}
