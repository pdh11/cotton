use assertables::*;
use serial_test::*;
use std::env;
use std::path::Path;
use std::process::Command;

use std::io::{self, Write};

#[test]
#[serial]
#[cfg_attr(miri, ignore)]
fn arm_stm32f7_hello() {
    let elf = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/hello",
    );

    let mut cmd = Command::new("probe-run");
    if let Ok(serial) = env::var("COTTON_PROBE_STM32F746_NUCLEO") {
        cmd.arg("--probe");
        cmd.arg(serial);
    }
    let output = cmd
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains!(stdout, "Hello STM32F746 Nucleo");
}
