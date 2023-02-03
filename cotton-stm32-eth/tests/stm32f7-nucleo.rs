use serial_test::*;
use std::path::Path;
use std::process::Command;

use std::io::{self, Write};

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[test]
#[serial]
#[cfg_attr(miri, ignore)]
fn arm_stm32f7_hello() {
    let elf = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../cross-stm32f7-nucleo/target-arm/thumbv7em-none-eabi/debug/hello",
    );

    let output = Command::new("probe-run")
        .arg("--chip")
        .arg("STM32F746ZGTx")
        .arg(elf)
        .output()
        .expect("failed to execute probe-run");

    println!("manifest: {}", env!("CARGO_MANIFEST_DIR"));
    println!("status: {}", output.status);
    io::stdout().write_all(&output.stderr).unwrap();
    io::stdout().write_all(&output.stdout).unwrap();
    assert!(output.status.success());

    assert!(find_subsequence(&output.stdout, b"Hello world").is_some());
}
