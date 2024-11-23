use super::*;
use futures::future;
use mockall::mock;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::{pin, Pin};
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};

pub struct NoOpWaker;

impl Wake for NoOpWaker {
    fn wake(self: Arc<Self>) {}
}

pub type MockError = Error<<MockScsiTransport as ScsiTransport>::Error>;

mock! {
    pub ScsiTransportInner {
        pub fn command_in(
            &mut self,
            cmd: &[u8],
            data: &mut [u8]
        ) -> impl Future<Output = Result<usize, MockError>>;

        pub fn command_out(
            &mut self,
            cmd: &[u8],
            data: &[u8]
        ) -> impl Future<Output = Result<usize, MockError>>;

        pub fn command_nodata(
            &mut self,
            cmd: &[u8],
        ) -> impl Future<Output = Result<usize, MockError>>;
    }
}

pub struct MockScsiTransport {
    pub inner: MockScsiTransportInner,
}

impl MockScsiTransport {
    pub fn new() -> Self {
        Self {
            inner: MockScsiTransportInner::new(),
        }
    }
}

impl Debug for MockScsiTransport {
    fn fmt(&self, _: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        Ok(())
    }
}

impl ScsiTransport for MockScsiTransport {
    type Error = ();

    fn command(
        &mut self,
        cmd: &[u8],
        data: DataPhase,
    ) -> impl Future<Output = Result<usize, MockError>> {
        match data {
            DataPhase::In(data) => self.inner.command_in(cmd, data),
            DataPhase::Out(data) => self.inner.command_out(cmd, data),
            DataPhase::None => self.inner.command_nodata(cmd),
        }
    }
}

struct Fixture<'a> {
    c: &'a mut core::task::Context<'a>,
    d: ScsiDevice<MockScsiTransport>,
}

fn do_test<
    SetupFn: FnMut(&mut MockScsiTransportInner),
    TestFn: FnMut(Fixture),
>(
    mut setup: SetupFn,
    mut test: TestFn,
) {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockScsiTransport::new();

    setup(&mut hc.inner);

    let f = Fixture {
        c: &mut c,
        d: ScsiDevice::new(hc),
    };

    test(f);
}

trait PollExtras<T> {
    fn to_option(self) -> Option<T>;
}

impl<T> PollExtras<T> for Poll<T> {
    fn to_option(self) -> Option<T> {
        match self {
            Poll::Ready(t) => Some(t),
            _ => None,
        }
    }
}

pub trait ExtraExpectations {
    fn expect_request_sense(&mut self);
}

impl ExtraExpectations for MockScsiTransportInner {
    fn expect_request_sense(&mut self) {
        self.expect_command_in()
            .times(1)
            .withf(|c, _| c[0] == 3)
            .returning(command_ok_with(RequestSenseReply {
                sense_key: 1,
                additional_sense_code: 0xB,
                additional_sense_code_qualifier: 1,
                ..Default::default()
            }));
    }
}

pub trait ContextExtras {
    fn check_ok<T, F: Future<Output = Result<T, MockError>>>(
        &mut self,
        fut: F,
    ) -> T;

    fn check_fails<
        T: Debug + PartialEq,
        F: Future<Output = Result<T, MockError>>,
    >(
        &mut self,
        fut: F,
    );

    fn check_fails_custom<
        T: Debug + PartialEq,
        F: Future<Output = Result<T, MockError>>,
    >(
        &mut self,
        fut: F,
        e: MockError,
    );

    fn check_pends<T, F: Future<Output = Result<T, MockError>>>(
        &mut self,
        fut: F,
    );
}

impl ContextExtras for core::task::Context<'_> {
    fn check_ok<T, F: Future<Output = Result<T, MockError>>>(
        &mut self,
        fut: F,
    ) -> T {
        let fut = pin!(fut);
        let result = fut.poll(self).to_option().unwrap();
        result.unwrap()
    }

    fn check_fails<
        T: Debug + PartialEq,
        F: Future<Output = Result<T, MockError>>,
    >(
        &mut self,
        fut: F,
    ) {
        let fut = pin!(fut);
        let result = fut.poll(self).to_option().unwrap();
        assert_eq!(result.unwrap_err(), Error::Scsi(ScsiError::Overheat));
    }

    fn check_fails_custom<
        T: Debug + PartialEq,
        F: Future<Output = Result<T, MockError>>,
    >(
        &mut self,
        fut: F,
        e: MockError,
    ) {
        let fut = pin!(fut);
        let result = fut.poll(self).to_option().unwrap();
        assert_eq!(result.unwrap_err(), e);
    }

    fn check_pends<T, F: Future<Output = Result<T, MockError>>>(
        &mut self,
        fut: F,
    ) {
        let mut fut = pin!(fut);
        let result = fut.as_mut().poll(self);
        assert!(result.is_pending());
        let result2 = fut.as_mut().poll(self);
        assert!(result2.is_pending());
    }
}

fn command_nodata_ok(
    _: &[u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    Box::pin(future::ready(Ok(0)))
}

fn command_nodata_fails(
    _: &[u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    Box::pin(future::ready(Err(Error::CommandFailed)))
}

fn command_nodata_pends(
    _: &[u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    Box::pin(future::pending())
}

#[rustfmt::skip]
pub fn command_ok_with<T: bytemuck::NoUninit>(
    reply: T,
) -> impl FnMut(
    &[u8],
    &mut [u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    move |_, d| {
        let size = core::mem::size_of::<T>();
        d[0..size].copy_from_slice(bytemuck::bytes_of(&reply));
        Box::pin(future::ready(Ok(size)))
    }
}

pub fn command_in_pends(
    _: &[u8],
    _: &mut [u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    Box::pin(future::pending())
}

pub fn command_in_fails(
    _: &[u8],
    _: &mut [u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    Box::pin(future::ready(Err(Error::CommandFailed)))
}

pub fn command_out_ok(
    _: &[u8],
    d: &[u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    Box::pin(future::ready(Ok(d.len())))
}

pub fn command_out_fails(
    _: &[u8],
    _: &[u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    Box::pin(future::ready(Err(Error::CommandFailed)))
}

pub fn command_out_pends(
    _: &[u8],
    _: &[u8],
) -> Pin<Box<dyn Future<Output = Result<usize, MockError>>>> {
    Box::pin(future::pending())
}

#[test]
fn test_read_capacity_10() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_ok_with(ReadCapacity10Reply {
                    lba: 0x1020304_u32.to_be_bytes(),
                    block_size: 512_u32.to_be_bytes(),
                }));
        },
        |mut f| {
            let (count, size) = f.c.check_ok(f.d.read_capacity_10());
            assert_eq!(size, 0x200);
            assert_eq!(count, 0x01020304);
        },
    );
}

#[test]
fn test_read_capacity_10_wrong_size() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_ok_with([0u8; 6]));
        },
        |mut f| {
            f.c.check_fails_custom(
                f.d.read_capacity_10(),
                Error::ProtocolError,
            );
        },
    );
}

#[test]
fn test_read_capacity_10_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.read_capacity_10());
        },
    );
}

#[test]
fn test_read_capacity_10_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.read_capacity_10());
        },
    );
}

#[test]
fn test_read_capacity_10_error_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_in_fails);
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.read_capacity_10());
        },
    );
}

#[test]
fn test_read_capacity_16() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x9e && c[1] == 0x10 && c[13] >= 32)
                .returning(command_ok_with(ReadCapacity16Reply {
                    lba: 0x102030405060708_u64.to_be_bytes(),
                    block_size: 4096_u32.to_be_bytes(),
                    flags: [0; 2],
                    lowest_aligned_lba: [0; 2],
                    reserved: [0; 16],
                }));
        },
        |mut f| {
            let (count, size) = f.c.check_ok(f.d.read_capacity_16());
            assert_eq!(size, 4096);
            assert_eq!(count, 0x0102030405060708);
        },
    );
}

#[test]
fn test_read_capacity_16_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x9E)
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.read_capacity_16());
        },
    );
}

#[test]
fn test_read_capacity_16_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x9E)
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.read_capacity_16());
        },
    );
}

#[test]
fn test_unit_ready() {
    do_test(
        |t| {
            t.expect_command_nodata()
                .times(1)
                .withf(|c| c[0] == 0)
                .returning(command_nodata_ok);
        },
        |mut f| {
            f.c.check_ok(f.d.test_unit_ready());
        },
    );
}

#[test]
fn test_unit_ready_fails() {
    do_test(
        |t| {
            t.expect_command_nodata()
                .times(1)
                .withf(|c| c[0] == 0)
                .returning(command_nodata_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.test_unit_ready());
        },
    );
}

#[test]
fn test_unit_ready_pends() {
    do_test(
        |t| {
            t.expect_command_nodata()
                .times(1)
                .withf(|c| c[0] == 0)
                .returning(command_nodata_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.test_unit_ready());
        },
    );
}

#[test]
fn test_unit_ready_error_pends() {
    do_test(
        |t| {
            t.expect_command_nodata()
                .times(1)
                .withf(|c| c[0] == 0)
                .returning(command_nodata_fails);
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.test_unit_ready());
        },
    );
}

#[test]
fn test_unit_ready_error_fails() {
    do_test(
        |t| {
            t.expect_command_nodata()
                .times(1)
                .withf(|c| c[0] == 0)
                .returning(command_nodata_fails);
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_ok_with([0u8; 2]));
        },
        |mut f| {
            f.c.check_fails_custom(
                f.d.test_unit_ready(),
                Error::CommandFailed,
            );
        },
    );
}

#[test]
fn test_unit_ready_error_fails2() {
    do_test(
        |t| {
            t.expect_command_nodata()
                .times(1)
                .withf(|c| c[0] == 0)
                .returning(command_nodata_fails);
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_in_fails);
        },
        |mut f| {
            f.c.check_fails_custom(
                f.d.test_unit_ready(),
                Error::CommandFailed,
            );
        },
    );
}

#[test]
fn test_read_10() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x28 && c[1] == 0 && c[5] == 81 && c[8] == 1
                })
                .returning(command_ok_with([42u8; 512]));
        },
        |mut f| {
            let mut buf = [0u8; 512];
            let size = f.c.check_ok(f.d.read_10(81, 1, &mut buf));
            assert_eq!(size, 0x200);
        },
    );
}

#[test]
fn test_read_10_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x28 && c[1] == 0 && c[5] == 81 && c[8] == 1
                })
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_fails(f.d.read_10(81, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_10_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x28 && c[1] == 0 && c[5] == 81 && c[8] == 1
                })
                .returning(command_in_pends);
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_pends(f.d.read_10(81, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_10_error_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x28 && c[1] == 0 && c[5] == 81 && c[8] == 1
                })
                .returning(command_in_fails);
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_in_pends);
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_pends(f.d.read_10(81, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_16() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x88 && c[1] == 0 && c[9] == 81 && c[13] == 1
                })
                .returning(command_ok_with([42u8; 512]));
        },
        |mut f| {
            let mut buf = [0u8; 512];
            let size = f.c.check_ok(f.d.read_16(81, 1, &mut buf));
            assert_eq!(size, 0x200);
        },
    );
}

#[test]
fn test_read_16_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x88 && c[1] == 0 && c[9] == 81 && c[13] == 1
                })
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_fails(f.d.read_16(81, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_16_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x88 && c[1] == 0 && c[9] == 81 && c[13] == 1
                })
                .returning(command_in_pends);
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_pends(f.d.read_16(81, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_16_error_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x88 && c[1] == 0 && c[9] == 81 && c[13] == 1
                })
                .returning(command_in_fails);
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_in_pends);
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_pends(f.d.read_16(81, 1, &mut buf));
        },
    );
}

#[test]
fn test_write_10() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x2A && c[1] == 0 && c[5] == 81 && c[8] == 1
                })
                .returning(command_out_ok);
        },
        |mut f| {
            let buf = [0u8; 512];
            let size = f.c.check_ok(f.d.write_10(81, 1, &buf));
            assert_eq!(size, 0x200);
        },
    );
}

#[test]
fn test_write_10_fails() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x2A && c[1] == 0 && c[5] == 81 && c[8] == 1
                })
                .returning(command_out_fails);
            t.expect_request_sense();
        },
        |mut f| {
            let buf = [0u8; 512];
            f.c.check_fails(f.d.write_10(81, 1, &buf));
        },
    );
}

#[test]
fn test_write_10_pends() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x2A && c[1] == 0 && c[5] == 81 && c[8] == 1
                })
                .returning(command_out_pends);
        },
        |mut f| {
            let buf = [0u8; 512];
            f.c.check_pends(f.d.write_10(81, 1, &buf));
        },
    );
}

#[test]
fn test_write_10_error_pends() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x2A && c[1] == 0 && c[5] == 81 && c[8] == 1
                })
                .returning(command_out_fails);
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_in_pends);
        },
        |mut f| {
            let buf = [0u8; 512];
            f.c.check_pends(f.d.write_10(81, 1, &buf));
        },
    );
}

#[test]
fn test_write_16() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x8A && c[1] == 0 && c[9] == 81 && c[13] == 1
                })
                .returning(command_out_ok);
        },
        |mut f| {
            let buf = [0u8; 512];
            let size = f.c.check_ok(f.d.write_16(81, 1, &buf));
            assert_eq!(size, 0x200);
        },
    );
}

#[test]
fn test_write_16_fails() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x8A && c[1] == 0 && c[9] == 81 && c[13] == 1
                })
                .returning(command_out_fails);
            t.expect_request_sense();
        },
        |mut f| {
            let buf = [0u8; 512];
            f.c.check_fails(f.d.write_16(81, 1, &buf));
        },
    );
}

#[test]
fn test_write_16_pends() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x8A && c[1] == 0 && c[9] == 81 && c[13] == 1
                })
                .returning(command_out_pends);
        },
        |mut f| {
            let buf = [0u8; 512];
            f.c.check_pends(f.d.write_16(81, 1, &buf));
        },
    );
}

#[test]
fn test_write_16_error_pends() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x8A && c[1] == 0 && c[9] == 81 && c[13] == 1
                })
                .returning(command_out_fails);
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_in_pends);
        },
        |mut f| {
            let buf = [0u8; 512];
            f.c.check_pends(f.d.write_16(81, 1, &buf));
        },
    );
}

#[test]
fn test_report_supported_operation_codes() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0xA3
                        && c[1] == 0xC
                        && c[3] == 0xF0
                        && c[4] == 0
                        && c[5] == 0
                })
                .returning(command_ok_with(
                    ReportSupportedOperationCodesReply {
                        reserved: 0,
                        support: 3,
                        cdb_size: [0; 2],
                    },
                ));
        },
        |mut f| {
            let supported =
                f.c.check_ok(f.d.report_supported_operation_codes(0xF0, None));
            assert!(supported);
        },
    );
}

#[test]
fn test_report_supported_operation_codes_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0xA3
                        && c[1] == 0xC
                        && c[3] == 0xF0
                        && c[4] == 0
                        && c[5] == 0
                })
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.report_supported_operation_codes(0xF0, None));
        },
    );
}

#[test]
fn test_report_supported_operation_codes_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0xA3
                        && c[1] == 0xC
                        && c[3] == 0xF0
                        && c[4] == 0
                        && c[5] == 0
                })
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.report_supported_operation_codes(0xF0, None));
        },
    );
}

#[test]
fn test_inquiry() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x12 && c[1] == 0x0 && c[4] >= 36)
                .returning(command_ok_with(StandardInquiryData {
                    peripheral_device_type: 5,
                    removable: 0x80,
                    ..Default::default()
                }));
        },
        |mut f| {
            let data = f.c.check_ok(f.d.inquiry());
            assert_eq!(data.peripheral_type, PeripheralType::Optical);
            assert!(data.is_removable);
        },
    );
}

#[test]
fn test_inquiry_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x12 && c[1] == 0x0 && c[4] >= 36)
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.inquiry());
        },
    );
}

#[test]
fn test_inquiry_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x12 && c[1] == 0x0 && c[4] >= 36)
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.inquiry());
        },
    );
}

#[test]
fn test_block_limits_page() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x12 && c[1] == 1 && c[2] == 176 && c[4] >= 64
                })
                .returning(command_ok_with(BlockLimitsPage {
                    peripheral_device_type: 5,
                    optimal_transfer_length_granularity: 16384u16
                        .to_be_bytes(),
                    ..Default::default()
                }));
        },
        |mut f| {
            let data = f.c.check_ok(f.d.block_limits_page());
            assert_eq!(
                u16::from_be_bytes(data.optimal_transfer_length_granularity),
                16384
            );
        },
    );
}

#[test]
fn test_block_limits_page_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x12 && c[1] == 1 && c[2] == 176 && c[4] >= 64
                })
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.block_limits_page());
        },
    );
}

#[test]
fn test_block_limits_page_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| {
                    c[0] == 0x12 && c[1] == 1 && c[2] == 176 && c[4] >= 64
                })
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.block_limits_page());
        },
    );
}

#[test]
fn test_two_factor_error() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_ok_with(RequestSenseReply {
                    sense_key: 5,
                    additional_sense_code: 0x20,
                    ..Default::default()
                }));
        },
        |mut f| {
            let fut = pin!(f.d.try_upgrade_error(Error::CommandFailed));
            let result = fut.poll(f.c).to_option().unwrap();
            assert_eq!(
                result,
                Error::Scsi(ScsiError::InvalidCommandOperationCode),
            );
        },
    );
}

#[test]
fn test_one_factor_error() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 3)
                .returning(command_ok_with(RequestSenseReply {
                    sense_key: 7,
                    ..Default::default()
                }));
        },
        |mut f| {
            let fut = pin!(f.d.try_upgrade_error(Error::CommandFailed));
            let result = fut.poll(f.c).to_option().unwrap();
            assert_eq!(result, Error::Scsi(ScsiError::DataProtect),);
        },
    );
}

#[test]
fn test_protocol_error_not_sensed() {
    do_test(
        |t| {
            t.expect_command_in().times(0);
        },
        |mut f| {
            let fut = pin!(f.d.try_upgrade_error(Error::ProtocolError));
            let result = fut.poll(f.c).to_option().unwrap();
            assert_eq!(result, Error::ProtocolError,);
        },
    );
}
