use serial_test::*;
use std::process::Command;

use std::io::{self, Write};

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[test]
#[serial]
#[cfg_attr(miri, ignore)]
fn arm_stm32f7_hello() {
    let output = Command::new("probe-run")
        .arg("--chip")
        .arg("STM32F746ZGTx")
        .arg("../cross-stm32f7-nucleo/target-arm/thumbv7em-none-eabi/debug/hello")
        .output()
        .expect("failed to flash STM32F7 Nucleo");

    io::stdout().write_all(&output.stdout).unwrap();

    assert!(find_subsequence(&output.stdout, b"Hello world").is_some());
}
