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

pub struct DeviceTest {
    inner: Mutex<DeviceTestInner>,
}

impl DeviceTest {
    fn new(
        chip: &str,
        environment_variable: &str,
        firmware: &str,
    ) -> (Child, Self) {
        let elf = Path::new(env!("CARGO_MANIFEST_DIR")).join(firmware);

        let mut cmd = Command::new("probe-run");
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
            .expect("failed to execute probe-run");
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

        loop {
            {
                let mut inner = self.inner.lock().unwrap();
                let mut s = String::new();
                inner.stdout.read_available_to_string(&mut s).unwrap();
                inner.output.push_str(&s);
                print!("{s}");
                if let Some((_before, after)) = inner.output.split_once(needle)
                {
                    eprintln!("OK: {needle}");
                    inner.output = after.to_string();
                    return;
                }

                if start.elapsed() > timeout {
                    assert_contains!(inner.output, needle);
                    return;
                }
            }
            sleep(Duration::from_millis(200));
        }
    }

    pub fn expect_stderr(&self, needle: &str, timeout: Duration) {
        let start = Instant::now();

        loop {
            {
                let mut inner = self.inner.lock().unwrap();
                let mut s = String::new();
                inner.stderr.read_available_to_string(&mut s).unwrap();
                inner.errors.push_str(&s);
                print!("{s}");
                if let Some((_before, after)) = inner.output.split_once(needle)
                {
                    eprintln!("OK: {needle}");
                    inner.output = after.to_string();
                    return;
                }

                if start.elapsed() > timeout {
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
    _ = child.kill();
    assert!(result.is_ok());
}
