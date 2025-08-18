use super::*;
use cotton_scsi::scsi_transport;
use cotton_usb_host::host_controller::TransferExtras;
use cotton_usb_host::mocks::{MockHostController, MockHostControllerInner};
use cotton_usb_host::usb_bus::{create_test_device, UsbBus};
use cotton_usb_host::wire::SetupPacket;
use futures::{future, Future};
use std::cell::Cell;
use std::fmt::Debug;
use std::pin::{pin, Pin};
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};

struct NoOpWaker;

impl Wake for NoOpWaker {
    fn wake(self: Arc<Self>) {}
}

type MockError = scsi_transport::Error<UsbError>;
type PinnedFuture = Pin<Box<dyn Future<Output = Result<usize, UsbError>>>>;

/*
fn no_delay(_ms: usize) -> impl Future<Output = ()> {
    future::ready(())
}
*/

fn control_transfer_ok<const N: usize>(
    _: u8,
    _: TransferExtras,
    _: u8,
    _: SetupPacket,
    _: cotton_usb_host::host_controller::DataPhase,
) -> PinnedFuture {
    Box::pin(future::ready(Ok(N)))
}

fn control_transfer_pends(
    _: u8,
    _: TransferExtras,
    _: u8,
    _: SetupPacket,
    _: cotton_usb_host::host_controller::DataPhase,
) -> PinnedFuture {
    Box::pin(future::pending())
}

fn control_transfer_fails(
    _: u8,
    _: TransferExtras,
    _: u8,
    _: SetupPacket,
    _: cotton_usb_host::host_controller::DataPhase,
) -> PinnedFuture {
    Box::pin(future::ready(Err(UsbError::Timeout)))
}

fn bulk_out_ok<const N: usize>(
    _: u8,
    _: u8,
    _: u16,
    _: &[u8],
    _: TransferType,
    _: &Cell<bool>,
) -> PinnedFuture {
    Box::pin(future::ready(Ok(N)))
}

fn bulk_out_fails(
    _: u8,
    _: u8,
    _: u16,
    _: &[u8],
    _: TransferType,
    _: &Cell<bool>,
) -> PinnedFuture {
    Box::pin(future::ready(Err(UsbError::Timeout)))
}

fn bulk_out_pends(
    _: u8,
    _: u8,
    _: u16,
    _: &[u8],
    _: TransferType,
    _: &Cell<bool>,
) -> PinnedFuture {
    Box::pin(future::pending())
}

fn bulk_in_ok_with<F: FnMut(&mut [u8]) -> usize>(
    mut f: F,
) -> impl FnMut(u8, u8, u16, &mut [u8], TransferType, &Cell<bool>) -> PinnedFuture
{
    move |_, _, _, d, _, _| {
        let n = f(d);
        Box::pin(future::ready(Ok(n)))
    }
}

fn bulk_in_fails(
    _: u8,
    _: u8,
    _: u16,
    _: &mut [u8],
    _: TransferType,
    _: &Cell<bool>,
) -> PinnedFuture {
    Box::pin(future::ready(Err(UsbError::Timeout)))
}

fn bulk_in_stalls(
    _: u8,
    _: u8,
    _: u16,
    _: &mut [u8],
    _: TransferType,
    _: &Cell<bool>,
) -> PinnedFuture {
    Box::pin(future::ready(Err(UsbError::Stall)))
}

fn bulk_in_pends(
    _: u8,
    _: u8,
    _: u16,
    _: &mut [u8],
    _: TransferType,
    _: &Cell<bool>,
) -> PinnedFuture {
    Box::pin(future::pending())
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

struct Fixture<'a> {
    c: &'a mut core::task::Context<'a>,
    m: MassStorage<'a, MockHostController>,
}

fn do_test<
    SetupFn: FnMut(&mut MockHostControllerInner),
    TestFn: FnMut(Fixture),
>(
    mut setup: SetupFn,
    mut test: TestFn,
) {
    let w = Waker::from(Arc::new(NoOpWaker));
    let mut c = core::task::Context::from_waker(&w);

    let mut hc = MockHostController::default();

    setup(&mut hc.inner);
    let bus = UsbBus::new(hc);
    // SAFETY: we don't use this with a non-mock bus
    let device = unsafe { create_test_device(2, 2) };

    let f = Fixture {
        c: &mut c,
        m: MassStorage::new(&bus, device).unwrap(),
    };

    test(f);
}

fn status_ok(data: &mut [u8]) -> usize {
    data.len()
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
        assert_eq!(
            result.unwrap_err(),
            MockError::Transport(UsbError::Timeout)
        );
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

#[test]
fn test_new() {
    do_test(|_| {}, |_| {});
}

#[test]
fn test_new_fails() {
    let hc = MockHostController::default();
    let bus = UsbBus::new(hc);

    // SAFETY: we don't use this with a non-mock bus
    let device = unsafe { create_test_device(2, 0) }; // no IN eps
    assert!(MassStorage::new(&bus, device).is_err());

    // SAFETY: we don't use this with a non-mock bus
    let device = unsafe { create_test_device(0, 2) }; // no OUT eps
    assert!(MassStorage::new(&bus, device).is_err());
}

#[test]
fn test_command_nodata() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0
                        && d[14] == 1
                        && d[15] == 42
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .returning(bulk_in_ok_with(status_ok));
        },
        |mut f| {
            let result = f.c.check_ok(f.m.command(&[42u8], DataPhase::None));
            assert_eq!(result, 0);
        },
    );
}

#[test]
fn test_command_nodata_short() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[14] == 1
                        && d[15] == 42
                })
                .returning(bulk_out_ok::<1>);
            hc.expect_bulk_in_transfer().times(0);
        },
        |mut f| {
            f.c.check_fails_custom(
                f.m.command(&[42u8], DataPhase::None),
                Error::ProtocolError,
            );
        },
    );
}

#[test]
fn test_command_nodata_fails() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[14] == 1
                        && d[15] == 42
                })
                .returning(bulk_out_fails);
        },
        |mut f| {
            f.c.check_fails(f.m.command(&[42u8], DataPhase::None));
        },
    );
}

#[test]
fn test_command_nodata_pends() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[14] == 1
                        && d[15] == 42
                })
                .returning(bulk_out_pends);
        },
        |mut f| {
            f.c.check_pends(f.m.command(&[42u8], DataPhase::None));
        },
    );
}

#[test]
fn test_command_nodata_reply_short() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[14] == 1
                        && d[15] == 42
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .returning(bulk_in_ok_with(|_| 12));
        },
        |mut f| {
            f.c.check_fails_custom(
                f.m.command(&[42u8], DataPhase::None),
                Error::ProtocolError,
            );
        },
    );
}

#[test]
fn test_command_nodata_reply_pends() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[14] == 1
                        && d[15] == 42
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .returning(bulk_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.m.command(&[42u8], DataPhase::None));
        },
    );
}

#[test]
fn test_command_nodata_reply_fails() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[14] == 1
                        && d[15] == 42
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .returning(bulk_in_fails);
        },
        |mut f| {
            f.c.check_fails(f.m.command(&[42u8], DataPhase::None));
        },
    );
}

#[test]
fn test_command_in() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0x80
                        && d[14] == 2
                        && d[15] == 43
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_in_ok_with(|_| 512));
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 13)
                .returning(bulk_in_ok_with(status_ok));
        },
        |mut f| {
            let mut buf = [0; 512];
            let result =
                f.c.check_ok(f.m.command(&[43, 43], DataPhase::In(&mut buf)));
            assert_eq!(result, 512);
        },
    );
}

#[test]
fn test_command_in_pends() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0x80
                        && d[14] == 2
                        && d[15] == 43
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_in_pends);
        },
        |mut f| {
            let mut buf = [0; 512];
            f.c.check_pends(f.m.command(&[43, 43], DataPhase::In(&mut buf)));
        },
    );
}

#[test]
fn test_command_in_fails() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0x80
                        && d[14] == 2
                        && d[15] == 43
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_in_fails);
        },
        |mut f| {
            let mut buf = [0; 512];
            f.c.check_fails(f.m.command(&[43, 43], DataPhase::In(&mut buf)));
        },
    );
}

#[test]
fn test_command_in_stalls() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0x80
                        && d[14] == 2
                        && d[15] == 43
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_in_stalls);
            hc.expect_control_transfer()
                .times(1)
                .returning(control_transfer_ok::<0>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 13)
                .returning(bulk_in_ok_with(status_ok));
        },
        |mut f| {
            let mut buf = [0; 512];
            let result =
                f.c.check_ok(f.m.command(&[43, 43], DataPhase::In(&mut buf)));
            assert_eq!(result, 0);
        },
    );
}

#[test]
fn test_command_in_clear_stall_pends() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0x80
                        && d[14] == 2
                        && d[15] == 43
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_in_stalls);
            hc.expect_control_transfer()
                .times(1)
                .returning(control_transfer_pends);
        },
        |mut f| {
            let mut buf = [0; 512];
            f.c.check_pends(f.m.command(&[43, 43], DataPhase::In(&mut buf)));
        },
    );
}

#[test]
fn test_command_in_clear_stall_fails() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0x80
                        && d[14] == 2
                        && d[15] == 43
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_in_stalls);
            hc.expect_control_transfer()
                .times(1)
                .returning(control_transfer_fails);
        },
        |mut f| {
            let mut buf = [0; 512];
            f.c.check_fails(f.m.command(&[43, 43], DataPhase::In(&mut buf)));
        },
    );
}

#[test]
fn test_command_out() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0
                        && d[14] == 3
                        && d[15] == 44
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_out_ok::<512>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 13)
                .returning(bulk_in_ok_with(status_ok));
        },
        |mut f| {
            let buf = [0; 512];
            let result =
                f.c.check_ok(f.m.command(&[44, 44, 44], DataPhase::Out(&buf)));
            assert_eq!(result, 512);
        },
    );
}

#[test]
fn test_command_out_pends() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0
                        && d[14] == 3
                        && d[15] == 44
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_out_pends);
        },
        |mut f| {
            let buf = [0; 512];
            f.c.check_pends(f.m.command(&[44, 44, 44], DataPhase::Out(&buf)));
        },
    );
}

#[test]
fn test_command_out_fail_status() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0
                        && d[14] == 3
                        && d[15] == 44
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_out_ok::<512>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 13)
                .returning(bulk_in_ok_with(|d| {
                    d[12] = 1;
                    13
                }));
        },
        |mut f| {
            let buf = [0; 512];
            f.c.check_fails_custom(
                f.m.command(&[44, 44, 44], DataPhase::Out(&buf)),
                Error::CommandFailed,
            );
        },
    );
}

#[test]
fn test_command_out_wild_status() {
    do_test(
        |hc| {
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| {
                    d.len() == 31
                        && d[0] == 0x55
                        && d[1] == 0x53
                        && d[2] == 0x42
                        && d[3] == 0x43
                        && d[12] == 0
                        && d[14] == 3
                        && d[15] == 44
                })
                .returning(bulk_out_ok::<31>);
            hc.expect_bulk_out_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 512)
                .returning(bulk_out_ok::<512>);
            hc.expect_bulk_in_transfer()
                .times(1)
                .withf(|_, _, _, d, _, _| d.len() == 13)
                .returning(bulk_in_ok_with(|d| {
                    d[12] = 135;
                    13
                }));
        },
        |mut f| {
            let buf = [0; 512];
            f.c.check_fails_custom(
                f.m.command(&[44, 44, 44], DataPhase::Out(&buf)),
                Error::ProtocolError,
            );
        },
    );
}

const HANDBAG: &[u8] = &[
    9, 2, 32, 0, 1, 1, 0, 128, 50, 9, 4, 0, 0, 2, 8, 6, 80, 0, 7, 5, 1, 2, 0,
    2, 0, 7, 5, 129, 2, 0, 2, 0,
];

#[test]
fn test_identify_mass_storage() {
    let mut ims = IdentifyMassStorage::default();
    cotton_usb_host::wire::parse_descriptors(HANDBAG, &mut ims);
    assert_eq!(ims.identify(), Some(1));
}

const ELLA: &[u8] = &[
    9, 2, 180, 1, 5, 1, 0, 128, 250, 9, 4, 0, 0, 4, 255, 0, 3, 0, 12, 95, 1,
    0, 10, 0, 4, 4, 1, 0, 4, 0, 7, 5, 2, 2, 0, 2, 0, 7, 5, 8, 2, 0, 2, 0, 7,
    5, 132, 2, 0, 2, 0, 7, 5, 133, 3, 8, 0, 8, 9, 4, 1, 0, 0, 254, 1, 1, 0, 9,
    33, 1, 200, 0, 0, 4, 1, 1, 16, 64, 8, 8, 11, 1, 1, 3, 69, 108, 108, 97,
    68, 111, 99, 107, 8, 11, 2, 3, 1, 0, 32, 5, 9, 4, 2, 0, 1, 1, 1, 32, 5, 9,
    36, 1, 0, 2, 11, 0, 1, 0, 12, 36, 3, 4, 2, 6, 0, 14, 11, 4, 0, 0, 8, 36,
    10, 10, 1, 7, 0, 0, 8, 36, 10, 11, 1, 7, 0, 0, 9, 36, 11, 12, 2, 10, 11,
    3, 0, 17, 36, 2, 13, 1, 1, 0, 10, 6, 63, 0, 0, 0, 0, 0, 0, 4, 34, 36, 6,
    14, 13, 0, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0,
    15, 0, 0, 0, 15, 0, 0, 0, 0, 64, 36, 9, 0, 0, 0, 49, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    64, 36, 9, 0, 0, 0, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 31, 36, 9, 0, 0, 0, 16, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 5,
    131, 3, 6, 0, 8, 9, 4, 3, 0, 0, 1, 2, 32, 5, 9, 4, 3, 1, 1, 1, 2, 32, 5,
    16, 36, 1, 13, 0, 1, 1, 0, 0, 0, 6, 63, 0, 0, 0, 0, 6, 36, 2, 1, 2, 16, 7,
    5, 9, 13, 64, 2, 4, 8, 37, 1, 0, 0, 1, 0, 0, 9, 4, 4, 0, 0, 1, 2, 32, 5,
];

#[test]
fn test_dont_identify_mass_storage() {
    let mut ims = IdentifyMassStorage::default();
    cotton_usb_host::wire::parse_descriptors(ELLA, &mut ims);
    assert_eq!(ims.identify(), None);
}
