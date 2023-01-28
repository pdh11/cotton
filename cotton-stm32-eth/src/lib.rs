#![no_std]

#[cfg(target_os="none")]
use defmt as log;
#[cfg(not(target_os="none"))]
use log;

pub fn hello() {
    log::error!("Hello world!");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_hello() {
        use super::hello;
        hello();
    }
}
