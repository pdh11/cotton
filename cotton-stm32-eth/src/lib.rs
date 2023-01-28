#![cfg_attr(target_os="none", no_std)]

#[cfg(target_os="none")]
use defmt as log;
#[cfg(not(target_os="none"))]
use std as log;

pub fn hello() {
    log::println!("Hello world!");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_hello() {
        use super::hello;
        hello();
    }
}
