use assertables::*;
use serial_test::*;
use std::env;
use std::path::Path;
use std::process::{Child, ChildStdout, ChildStderr, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};
use nonblock::NonBlockingReader;

struct DeviceTest {
    child: Child,
    stdout: NonBlockingReader<ChildStdout>,
    output: String,
    stderr: NonBlockingReader<ChildStderr>,
    errors: String,
}

impl DeviceTest {
    fn new(firmware: &str) -> Self {
        let elf = Path::new(env!("CARGO_MANIFEST_DIR")).join(firmware);

        let mut cmd = Command::new("probe-run");
        if let Ok(serial) = env::var("COTTON_PROBE_STM32F746_NUCLEO") {
            cmd.arg("--probe");
            cmd.arg(serial);
        }
        let mut child = cmd.arg("--chip")
            .arg("STM32F746ZGTx")
            .arg(elf)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to execute probe-run");
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        DeviceTest {
            child,
            stdout: NonBlockingReader::from_fd(stdout).unwrap(),
            output: String::new(),
            stderr: NonBlockingReader::from_fd(stderr).unwrap(),
            errors: String::new(),
        }
    }

    fn expect(&mut self, needle: &str, timeout: Duration) {
        let start = Instant::now();

        loop {
            let mut s = String::new();
            self.stdout.read_available_to_string(&mut s).unwrap();
            self.output.push_str(&s);
            println!("s={s}");
            if let Some((_before, after)) = self.output.split_once(needle) {
                eprintln!("OK: {needle}");
                self.output = after.to_string();
                return;
            }

            if start.elapsed() > timeout {
                assert_contains!(self.output, needle);
                return;
            }
            sleep(Duration::from_millis(200));
        }
    }

    fn expect_stderr(&mut self, needle: &str, timeout: Duration) {
        let start = Instant::now();

        loop {
            let mut s = String::new();
            self.stderr.read_available_to_string(&mut s).unwrap();
            self.errors.push_str(&s);
            println!("s={s}");
            if let Some((_before, after)) = self.output.split_once(needle) {
                eprintln!("OK: {needle}");
                self.output = after.to_string();
                return;
            }

            if start.elapsed() > timeout {
                assert_contains!(self.errors, needle);
                return;
            }
            sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for DeviceTest {
    fn drop(&mut self) {
        _ = self.child.kill();
    }
}

#[test]
#[serial]
#[cfg_attr(miri, ignore)]
fn arm_stm32f7_hello() {
    let mut t = DeviceTest::new(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/hello",
    );
    t.expect("Hello STM32F746 Nucleo", Duration::from_secs(5));
}

#[test]
#[serial]
#[cfg_attr(miri, ignore)]
fn arm_stm32f7_dhcp() {
    let mut t = DeviceTest::new(
        "../cross/stm32f746-nucleo/target/thumbv7em-none-eabi/debug/dhcp-rtic",
    );
    t.expect_stderr("(HOST) INFO  success!", Duration::from_secs(30));
    t.expect("DHCP config acquired!", Duration::from_secs(10));
}
