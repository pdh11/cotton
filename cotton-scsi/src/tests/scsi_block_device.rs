use super::*;
use crate::scsi_device::tests::{
    command_in_fails, command_in_pends, command_ok_with, command_out_fails,
    command_out_ok, command_out_pends, ContextExtras, ExtraExpectations,
    MockScsiTransport, MockScsiTransportInner, NoOpWaker,
};
use crate::scsi_device::{
    ReadCapacity10Reply, ReadCapacity16Reply,
    ReportSupportedOperationCodesReply,
};
use std::sync::Arc;
use std::task::Waker;

struct Fixture<'a> {
    c: &'a mut core::task::Context<'a>,
    d: ScsiBlockDevice<MockScsiTransport>,
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
        d: ScsiBlockDevice::new(ScsiDevice::new(hc)),
    };

    test(f);
}

#[test]
fn test_device_info() {
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
            let info = f.c.check_ok(f.d.device_info());
            assert_eq!(info.block_size, 512);
            assert_eq!(info.blocks, 0x1020304);
        },
    );
}

#[test]
fn test_device_info_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.device_info());
        },
    );
}

#[test]
fn test_device_info_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.device_info());
        },
    );
}

#[test]
fn test_device_info_large() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_ok_with(ReadCapacity10Reply {
                    lba: 0xFFFF_FFFF_u32.to_be_bytes(),
                    block_size: 512_u32.to_be_bytes(),
                }));
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
            let info = f.c.check_ok(f.d.device_info());
            assert_eq!(info.block_size, 4096);
            assert_eq!(info.blocks, 0x102030405060708);
        },
    );
}

#[test]
fn test_device_info_large_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_ok_with(ReadCapacity10Reply {
                    lba: 0xFFFF_FFFF_u32.to_be_bytes(),
                    block_size: 512_u32.to_be_bytes(),
                }));
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x9e && c[1] == 0x10 && c[13] >= 32)
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.device_info());
        },
    );
}

#[test]
fn test_device_info_large_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x25)
                .returning(command_ok_with(ReadCapacity10Reply {
                    lba: 0xFFFF_FFFF_u32.to_be_bytes(),
                    block_size: 512_u32.to_be_bytes(),
                }));
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x9e && c[1] == 0x10 && c[13] >= 32)
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.device_info());
        },
    );
}

#[test]
fn test_read_blocks() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x28)
                .returning(command_ok_with([43u8; 512]));
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_ok(f.d.read_blocks(0, 1, &mut buf));
            assert_eq!(buf[0], 43);
        },
    );
}

#[test]
fn test_read_blocks_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x28)
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_fails(f.d.read_blocks(0, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_blocks_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x28)
                .returning(command_in_pends);
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_pends(f.d.read_blocks(0, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_blocks_large() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x88)
                .returning(command_ok_with([44u8; 512]));
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_ok(f.d.read_blocks(0x1_0000_0000, 1, &mut buf));
            assert_eq!(buf[0], 44);
        },
    );
}

#[test]
fn test_read_blocks_large_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x88)
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_fails(f.d.read_blocks(0x1_0000_0000, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_blocks_large_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x88)
                .returning(command_in_pends);
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_pends(f.d.read_blocks(0x1_0000_0000, 1, &mut buf));
        },
    );
}

#[test]
fn test_read_blocks_too_large() {
    do_test(
        |t| {
            t.expect_command_in().times(0);
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_fails_custom(
                f.d.read_blocks(0xFFFF_FFFF_8000_0000, 0x8000_0000, &mut buf),
                Error::Scsi(ScsiError::LogicalBlockAddressOutOfRange),
            )
        },
    );
}

#[test]
fn test_read_blocks_short_read() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0x28)
                .returning(command_ok_with([43u8; 128]));
        },
        |mut f| {
            let mut buf = [0u8; 512];
            f.c.check_fails_custom(
                f.d.read_blocks(0, 1, &mut buf),
                Error::ProtocolError,
            );
        },
    );
}

#[test]
fn test_write_blocks() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, d| c[0] == 0x2A && d[0] == 47)
                .returning(command_out_ok);
        },
        |mut f| {
            let buf = [47u8; 512];
            f.c.check_ok(f.d.write_blocks(0, 1, &buf));
        },
    );
}

#[test]
fn test_write_blocks_fails() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, d| c[0] == 0x2A && d[0] == 47)
                .returning(command_out_fails);
            t.expect_request_sense();
        },
        |mut f| {
            let buf = [47u8; 512];
            f.c.check_fails(f.d.write_blocks(0, 1, &buf));
        },
    );
}

#[test]
fn test_write_blocks_pends() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, d| c[0] == 0x2A && d[0] == 47)
                .returning(command_out_pends);
        },
        |mut f| {
            let buf = [47u8; 512];
            f.c.check_pends(f.d.write_blocks(0, 1, &buf));
        },
    );
}

#[test]
fn test_write_blocks_large() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, d| c[0] == 0x8A && d[0] == 47)
                .returning(command_out_ok);
        },
        |mut f| {
            let buf = [47u8; 512];
            f.c.check_ok(f.d.write_blocks(0x1_0000_0000, 1, &buf));
        },
    );
}

#[test]
fn test_write_blocks_large_fails() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, d| c[0] == 0x8A && d[0] == 47)
                .returning(command_out_fails);
            t.expect_request_sense();
        },
        |mut f| {
            let buf = [47u8; 512];
            f.c.check_fails(f.d.write_blocks(0x1_0000_0000, 1, &buf));
        },
    );
}

#[test]
fn test_write_blocks_large_pends() {
    do_test(
        |t| {
            t.expect_command_out()
                .times(1)
                .withf(|c, d| c[0] == 0x8A && d[0] == 47)
                .returning(command_out_pends);
        },
        |mut f| {
            let buf = [47u8; 512];
            f.c.check_pends(f.d.write_blocks(0x1_0000_0000, 1, &buf));
        },
    );
}

#[test]
fn test_write_blocks_too_large() {
    do_test(
        |t| {
            t.expect_command_out().times(0);
        },
        |mut f| {
            let buf = [0u8; 512];
            f.c.check_fails_custom(
                f.d.write_blocks(0xFFFF_FFFF_8000_0000, 0x8000_0000, &buf),
                Error::Scsi(ScsiError::LogicalBlockAddressOutOfRange),
            )
        },
    );
}

#[test]
fn test_query_commands() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(10)
                .withf(|c, _| c[0] == 0xA3)
                .returning(command_ok_with(
                    ReportSupportedOperationCodesReply {
                        reserved: 0,
                        support: 3,
                        cdb_size: [0; 2],
                    },
                ));
        },
        |mut f| {
            f.c.check_ok(f.d.query_commands());
        },
    );
}

#[test]
fn test_query_commands_fails() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0xA3)
                .returning(command_in_fails);
            t.expect_request_sense();
        },
        |mut f| {
            f.c.check_fails(f.d.query_commands());
        },
    );
}

#[test]
fn test_query_commands_pends() {
    do_test(
        |t| {
            t.expect_command_in()
                .times(1)
                .withf(|c, _| c[0] == 0xA3)
                .returning(command_in_pends);
        },
        |mut f| {
            f.c.check_pends(f.d.query_commands());
        },
    );
}
