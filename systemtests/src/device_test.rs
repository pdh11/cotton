use assertables::*;
use nonblock::NonBlockingReader;
use std::env;
use std::panic;
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashSet;
use cotton_ssdp::Service;

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
                inner: Mutex::new(
                    DeviceTestInner {
                        stdout: NonBlockingReader::from_fd(stdout).unwrap(),
                        output: String::new(),
                        stderr: NonBlockingReader::from_fd(stderr).unwrap(),
                        errors: String::new(),
                    }
                ),
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
                if let Some((_before, after)) = inner.output.split_once(needle) {
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
                if let Some((_before, after)) = inner.output.split_once(needle) {
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

#[derive(Default)]
pub struct SsdpTest {
    seen: Arc<Mutex<HashSet<String>>>,
}

impl SsdpTest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn expect_seen(&self, notification_type: &str, timeout: Duration) {
        let start = Instant::now();

        loop {
            {
                let v = self.seen.lock().unwrap();
                if v.contains(notification_type) {
                    return;
                }
                if start.elapsed() > timeout {
                    assert_contains!(v, notification_type);
                    return;
                }
                // drop the lock
            }
            sleep(Duration::from_millis(200));
        }
    }
}

pub fn ssdp_test<F: FnOnce(SsdpTest) -> () + panic::UnwindSafe>(
    advertise: Option<String>,
    f: F,
) {
    let t = SsdpTest::new();
    let mut result = Ok(());
    let done = AtomicBool::new(false);
    let seen2 = t.seen.clone();

    thread::scope(|s| {
        s.spawn(|| {
            const SSDP_TOKEN1: mio::Token = mio::Token(0);
            const SSDP_TOKEN2: mio::Token = mio::Token(1);
            let mut poll = mio::Poll::new().unwrap();
            let mut events = mio::Events::with_capacity(128);

            let mut ssdp = Service::new(
                poll.registry(),
                (SSDP_TOKEN1, SSDP_TOKEN2),
            )
            .unwrap();

            if let Some(nt) = advertise {
                let uuid = uuid::Uuid::new_v4();
                ssdp.advertise(
                    uuid.to_string(),
                    cotton_ssdp::Advertisement {
                        notification_type: nt.to_string(),
                        location: "http://127.0.0.1/test".to_string(),
                    },
                );
            }

            ssdp.subscribe(
                "ssdp:all",
                Box::new(move |r| {
                    println!("HOST GOT {r:?}");
                    if let cotton_ssdp::Notification::Alive {
                        notification_type,
                        ..
                    } = r
                    {
                        let mut v = seen2.lock().unwrap();
                        v.insert(notification_type.clone());
                    }
                }),
            );

            loop {
                poll.poll(&mut events, Some(Duration::from_secs(1)))
                    .unwrap();

                if done.load(atomic::Ordering::Acquire) {
                    return;
                }

                if ssdp.next_wakeup() == std::time::Duration::ZERO {
                    // Timeout
                    ssdp.wakeup();
                }

                for event in &events {
                    match event.token() {
                        SSDP_TOKEN1 => ssdp.multicast_ready(),
                        SSDP_TOKEN2 => ssdp.search_ready(),
                        _ => (),
                    }
                }
            }
        });
        result = panic::catch_unwind(|| f(t));
        done.store(true, atomic::Ordering::Release);
    });
    assert!(result.is_ok());
}
