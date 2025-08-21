// feature=defmt and os=none? use defmt
//   feature=std? use std
//     neither? use nothing

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub use std::println;

#[cfg(all(target_os = "none", feature = "defmt"))]
pub use defmt::debug as println;

#[cfg(all(
    not(feature = "std"),
    not(all(target_os = "none", feature = "defmt"))
))]
#[macro_export]
macro_rules! println {
    ($fmt:expr) => {};
    ($fmt:expr, $($arg:tt)*) => {};
}

#[cfg(all(
    not(feature = "std"),
    not(all(target_os = "none", feature = "defmt"))
))]
pub use println;
