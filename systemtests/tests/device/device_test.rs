use assertables::*;
use nonblock::NonBlockingReader;
use std::env;
use std::panic;
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

struct DeviceTestInner {
    stdout: NonBlockingReader<ChildStdout>,
    output: String,
    stderr: NonBlockingReader<ChildStderr>,
    errors: String,
}

impl DeviceTestInner {
    fn poll(&mut self) {
        let mut v = Vec::new();
        self.stdout.read_available(&mut v).unwrap();
        let s = String::from_utf8_lossy(&v);
        self.output.push_str(&s);
        if !s.is_empty() {
            eprintln!(
                "{:?}: NEW stdout ({}/{}) {s}",
                Instant::now(),
                s.len(),
                self.output.len()
            );
        }

        let mut v = Vec::new();
        self.stderr.read_available(&mut v).unwrap();
        let s = String::from_utf8_lossy(&v);
        self.errors.push_str(&s);
        if !s.is_empty() {
            eprintln!("{:?}: NEW stderr {s}", Instant::now());
        }
    }
}

pub struct DeviceTest {
    inner: Mutex<DeviceTestInner>,
}

impl DeviceTest {
    fn new(
        chip: &str,
        environment_variable: &str,
        firmware: &str,
    ) -> (Child, Self) {
        let root_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let elf = Path::new(&root_dir).join(firmware);

        let mut cmd = Command::new("probe-rs");
        cmd.arg("run");

        if let Ok(serial) = env::var(environment_variable) {
            cmd.arg("--probe");
            cmd.arg(serial);
        }
        let mut child = cmd
            .arg("--chip")
            .arg(chip)
            .arg(elf)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to execute probe-rs");
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        (
            child,
            DeviceTest {
                inner: Mutex::new(DeviceTestInner {
                    stdout: NonBlockingReader::from_fd(stdout).unwrap(),
                    output: String::new(),
                    stderr: NonBlockingReader::from_fd(stderr).unwrap(),
                    errors: String::new(),
                }),
            },
        )
    }

    pub fn expect(&self, needle: &str, timeout: Duration) {
        let start = Instant::now();
        eprintln!("{:?}: searching stdout for {needle}", Instant::now());

        loop {
            {
                let mut inner = self.inner.lock().unwrap();
                inner.poll();
                if let Some((_before, after)) = inner.output.split_once(needle)
                {
                    eprintln!("OK: {needle}");
                    inner.output = after.to_string();
                    return;
                }

                if start.elapsed() > timeout {
                    eprintln!(
                        "{:?}: FAIL stdout {}",
                        Instant::now(),
                        inner.output
                    );
                    eprintln!(
                        "{:?}: FAIL stderr {}",
                        Instant::now(),
                        inner.errors
                    );
                    assert_contains!(inner.output, needle);
                    return;
                }
            }
            sleep(Duration::from_millis(200));
        }
    }

    pub fn expect_stderr(&self, needle: &str, timeout: Duration) {
        let start = Instant::now();
        eprintln!("{:?}: searching stderr for {needle}", Instant::now());

        loop {
            {
                let mut inner = self.inner.lock().unwrap();
                inner.poll();
                if let Some((_before, after)) = inner.errors.split_once(needle)
                {
                    eprintln!("OK: {needle}");
                    inner.errors = after.to_string();
                    return;
                }

                if start.elapsed() > timeout {
                    eprintln!(
                        "{:?}: FAIL stdout {}",
                        Instant::now(),
                        inner.output
                    );
                    eprintln!(
                        "{:?}: FAIL stderr {}",
                        Instant::now(),
                        inner.errors
                    );
                    assert_contains!(inner.errors, needle);
                    return;
                }
            }
            sleep(Duration::from_millis(200));
        }
    }
}

pub fn device_test<F: FnOnce(DeviceTest) -> () + panic::UnwindSafe>(
    chip: &str,
    environment_variable: &str,
    firmware: &str,
    f: F,
) {
    let (mut child, t) = DeviceTest::new(chip, environment_variable, firmware);
    let result = panic::catch_unwind(|| f(t));
    let status = child.try_wait();
    if let Ok(Some(status)) = status {
        eprintln!("probe-rs exited: {}", status);
    } else {
        _ = child.kill();
    }
    assert!(result.is_ok());
}
