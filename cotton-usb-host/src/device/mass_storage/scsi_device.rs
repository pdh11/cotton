use crate::debug;
use crate::device::mass_storage::{Error, ScsiTransport};
use crate::host_controller::DataPhase;
use core::str;

/// READ (10)
/// Seagate SCSI Commands Reference Manual s3.16
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct Read10 {
    operation_code: u8,
    flags: u8,
    lba_be: [u8; 4],
    group: u8,
    transfer_length_be: [u8; 2],
    control: u8,
}

impl Read10 {
    fn new(lba: u32, count: u16) -> Self {
        assert!(core::mem::size_of::<Self>() == 10);
        Self {
            operation_code: 0x28,
            flags: 0,
            lba_be: lba.to_be_bytes(),
            transfer_length_be: count.to_be_bytes(),
            group: 0,
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for Read10 {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for Read10 {}

/// READ (16)
/// Seagate SCSI Commands Reference Manual s3.18
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct Read16 {
    operation_code: u8,
    flags: u8,
    lba_be: [u8; 8],
    transfer_length_be: [u8; 4],
    group: u8,
    control: u8,
}

impl Read16 {
    fn new(lba: u64, count: u32) -> Self {
        assert!(core::mem::size_of::<Self>() == 16);
        Self {
            operation_code: 0x88,
            flags: 0,
            lba_be: lba.to_be_bytes(),
            transfer_length_be: count.to_be_bytes(),
            group: 0,
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for Read16 {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for Read16 {}

/// WRITE (10)
/// Seagate SCSI Commands Reference Manual s3.60
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct Write10 {
    operation_code: u8,
    flags: u8,
    lba_be: [u8; 4],
    group: u8,
    transfer_length_be: [u8; 2],
    control: u8,
}

impl Write10 {
    fn new(lba: u32, count: u16) -> Self {
        assert!(core::mem::size_of::<Self>() == 10);
        Self {
            operation_code: 0x2A,
            flags: 0,
            lba_be: lba.to_be_bytes(),
            transfer_length_be: count.to_be_bytes(),
            group: 0,
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for Write10 {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for Write10 {}

/// WRITE (16)
/// Seagate SCSI Commands Reference Manual s3.62
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct Write16 {
    operation_code: u8,
    flags: u8,
    lba_be: [u8; 8],
    transfer_length_be: [u8; 4],
    group: u8,
    control: u8,
}

impl Write16 {
    fn new(lba: u64, count: u32) -> Self {
        assert!(core::mem::size_of::<Self>() == 16);
        Self {
            operation_code: 0x8A,
            flags: 0,
            lba_be: lba.to_be_bytes(),
            transfer_length_be: count.to_be_bytes(),
            group: 0,
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for Write16 {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for Write16 {}

/// READ CAPACITY (10)
/// Seagate SCSI Commands Reference Manual s3.23.2
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct ReadCapacity10 {
    operation_code: u8,
    reserved1: u8,
    lba_be: [u8; 4],
    reserved6: [u8; 3],
    control: u8,
}

impl ReadCapacity10 {
    fn new() -> Self {
        assert!(core::mem::size_of::<Self>() == 10);
        Self {
            operation_code: 0x25,
            reserved1: 0,
            lba_be: [0u8; 4],
            reserved6: [0; 3],
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for ReadCapacity10 {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for ReadCapacity10 {}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Default)]
#[repr(C)]
struct ReadCapacity10Reply {
    lba: [u8; 4],
    block_size: [u8; 4],
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for ReadCapacity10Reply {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for ReadCapacity10Reply {}

/// READ CAPACITY (16)
/// Seagate SCSI Commands Reference Manual s3.23.2
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct ReadCapacity16 {
    operation_code: u8,
    service_action: u8,
    lba_be: [u8; 8],
    allocation_length_be: [u8; 4],
    reserved: u8,
    control: u8,
}

impl ReadCapacity16 {
    fn new() -> Self {
        assert!(core::mem::size_of::<Self>() == 16);
        Self {
            operation_code: 0x9E,
            service_action: 0x10,
            lba_be: [0u8; 8],
            allocation_length_be: [0, 0, 0, 32],
            reserved: 0,
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for ReadCapacity16 {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for ReadCapacity16 {}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Default)]
#[repr(C)]
struct ReadCapacity16Reply {
    lba: [u8; 8],
    block_size: [u8; 4],
    flags: [u8; 2],
    lowest_aligned_lba: [u8; 2],
    reserved: [u8; 16],
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for ReadCapacity16Reply {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for ReadCapacity16Reply {}

/// TEST UNIT READY
/// Seagate SCSI Commands Reference Manual s3.53
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct TestUnitReady {
    operation_code: u8,
    reserved: [u8; 4],
    control: u8,
}

impl TestUnitReady {
    fn new() -> Self {
        assert!(core::mem::size_of::<Self>() == 6);
        Self {
            operation_code: 0x00,
            reserved: [0u8; 4],
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for TestUnitReady {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for TestUnitReady {}

/// REQUEST SENSE
/// Seagate SCSI Commands Reference Manual s3.37
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct RequestSense {
    operation_code: u8,
    desc: u8,
    reserved: [u8; 2],
    allocation_length: u8,
    control: u8,
}

impl RequestSense {
    fn new() -> Self {
        assert!(core::mem::size_of::<Self>() == 6);
        Self {
            operation_code: 3,
            desc: 0,
            reserved: [0; 2],
            allocation_length: 18,
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for RequestSense {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for RequestSense {}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct RequestSenseReply {
    response_code: u8,
    reserved1: u8,
    sense_key: u8,
    information: [u8; 4],
    additional_length: u8,
    command_specific_information: [u8; 4],
    additional_sense_code: u8,
    additional_sense_code_qualifier: u8,
    fru_code: u8,
    sense_key_specific: [u8; 3],
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for RequestSenseReply {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for RequestSenseReply {}

/// REPORT SUPPORTED OPERATION CODES
/// Seagate SCSI Commands Reference Manual s3.34
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct ReportSupportedOperationCodes {
    operation_code: u8,
    service_action: u8,
    reporting_options: u8,
    requested_opcode: u8,
    requested_service_action_be: [u8; 2],
    allocation_length_be: [u8; 4],
    reserved: u8,
    control: u8,
}

impl ReportSupportedOperationCodes {
    fn new(opcode: u8, service_action: Option<u16>) -> Self {
        assert!(core::mem::size_of::<Self>() == 12);
        Self {
            operation_code: 0xA3,
            service_action: 0x0C,
            reporting_options: 3,
            requested_opcode: opcode,
            requested_service_action_be: service_action
                .unwrap_or_default()
                .to_be_bytes(),
            allocation_length_be: [0, 0, 0, 4],
            reserved: 0,
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for ReportSupportedOperationCodes {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for ReportSupportedOperationCodes {}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Default)]
#[repr(C)]
struct ReportSupportedOperationCodesReply {
    reserved: u8,
    support: u8,
    cdb_size: [u8; 2],
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for ReportSupportedOperationCodesReply {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for ReportSupportedOperationCodesReply {}

/// INQUIRY
/// Seagate SCSI Commands Reference Manual s3.6
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone)]
#[repr(C)]
struct Inquiry {
    operation_code: u8,
    evpd: u8,
    page_code: u8,
    allocation_length_be: [u8; 2],
    control: u8,
}

impl Inquiry {
    fn new(evpd: Option<u8>, len: u16) -> Self {
        assert!(core::mem::size_of::<Self>() == 6);
        Self {
            operation_code: 0x12,
            evpd: evpd.is_some() as u8,
            page_code: evpd.unwrap_or_default(),
            allocation_length_be: len.to_be_bytes(),
            control: 0,
        }
    }
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for Inquiry {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for Inquiry {}

/// Standard INQUIRY data
/// Seagate SCSI Commands Reference Manual s3.6.2
///
/// This is the compulsory leading 36 bytes; the actual data might be
/// larger (but the device truncates it, and tells us that it's done
/// so via the "residue" field of the command status wrapper).
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct StandardInquiryData {
    peripheral_device_type: u8,
    removable: u8,
    version: u8,
    data_format: u8,
    additional_length: u8,
    flags: [u8; 3],
    vendor_id: [u8; 8],
    product_id: [u8; 16],
    product_revision: [u8; 4],
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for StandardInquiryData {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for StandardInquiryData {}

/// Inquiry Block Limits page
/// Seagate SCSI Commands Reference Manual s5.4.5
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct BlockLimitsPage {
    peripheral_device_type: u8,
    page_code: u8,
    page_length: [u8; 2],
    wsnz: u8,
    max_compare_and_write: u8,
    optimal_transfer_length_granularity: [u8; 2],
    maximum_transfer_length: [u8; 4],
    optimal_transfer_length: [u8; 4], // 16

    maximum_prefetch_length: [u8; 4],
    maximum_unmap_lba_count: [u8; 4],
    maximum_unmap_block_descriptor_count: [u8; 4],
    optimal_unmap_granularity: [u8; 4], // 32

    unmap_granularity_alignemnt: [u8; 4],
    maximum_write_same_length: [u8; 8],
    maximum_atomic_transfer_length: [u8; 4], // 48

    atomic_alignment: [u8; 4],
    atomic_transfer_length_granularity: [u8; 4],
    maximum_atomic_transfer_length_with_atomic_boundary: [u8; 4],
    maximum_atomic_boundary_size: [u8; 4],
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for BlockLimitsPage {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for BlockLimitsPage {}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum PeripheralType {
    Disk = 0,
    Sequential = 1,
    Printer = 2,
    Processor = 3,
    WriteOnce = 4,
    Optical = 5,
    Scanner = 6,
    OpticalMemory = 7,
    Changer = 8,
    Communications = 9,
    Obsolete10 = 0xa,
    Obsolete11 = 0xb,
    StorageArray = 0xc,
    EnclosureServices = 0xd,
    SimplifiedDirect = 0xe,
    OpticalCardReader = 0xf,
    BridgeController = 0x10,
    ObjectStorage = 0x11,
    Automation = 0x12,
    Reserved13 = 0x13,
    Reserved14 = 0x14,
    Reserved15 = 0x15,
    Reserved16 = 0x16,
    Reserved17 = 0x17,
    Reserved18 = 0x18,
    Reserved19 = 0x19,
    Reserved1A = 0x1A,
    Reserved1B = 0x1B,
    Reserved1C = 0x1C,
    Reserved1D = 0x1D,
    WellKnownUnit = 0x1E,
    Other = 0x1F,
}

pub struct InquiryData {
    pub peripheral_type: PeripheralType,
    pub is_removable: bool,
}

/// | Test device  | {R,W,RC}(10) | {R,W,RC}(16) |  BLP  |  RSOC  |
/// | ---          | :---:        | :---:        | :---: | :---:  |
/// | Black (4G)   |    Y         | Y | - | - |
/// | Green (16G)  |    Y         | Y | - | - |
/// | Handbag (8G) |    Y         | - | - | - |
/// | Poker (1G)   |    Y         | - | - | - |
///
pub struct ScsiDevice<T: ScsiTransport> {
    transport: T,
}

impl<T: ScsiTransport> ScsiDevice<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    async fn try_upgrade_error<R>(
        &mut self,
        e: Result<R, Error>,
    ) -> Result<R, Error> {
        if let Err(Error::CommandFailed) = e {
            if let Ok(r) = self.request_sense().await {
                const ERRORS3: &[(u8, u8, u8, Error)] = &[
                    (2, 4, 1, Error::BecomingReady),
                    (2, 4, 2, Error::StartUnitRequired),
                    (2, 4, 3, Error::ManualInterventionRequired),
                    (2, 4, 4, Error::FormatInProgress),
                    (2, 4, 9, Error::SelfTestInProgress),
                    (2, 4, 0x22, Error::PowerCycleRequired),
                    (1, 0x0B, 0x01, Error::Overheat),
                    (1, 0x0B, 0x02, Error::EnclosureDegraded),
                    (3, 0x0C, 0x00, Error::WriteError),
                    (3, 0x0C, 0x02, Error::WriteReallocationFailed),
                    (1, 0x11, 0x00, Error::UnrecoveredReadError),
                    (1, 0x11, 0x01, Error::ReadRetriesExhausted),
                    (1, 0x11, 0x02, Error::ReadErrorTooLong),
                    (3, 0x11, 0x04, Error::ReadReallocationFailed),
                    (3, 0x14, 0x00, Error::LogicalBlockNotFound),
                    (3, 0x14, 0x01, Error::RecordNotFound),
                    (5, 0x26, 0x00, Error::InvalidFieldInParameterList),
                    (5, 0x26, 0x01, Error::ParameterNotSupported),
                    (5, 0x26, 0x02, Error::ParameterValueInvalid),
                    (4, 0x3E, 0x03, Error::LogicalUnitSelfTestFailed),
                    (4, 0x42, 0x00, Error::SelfTestFailed),
                ];
                const ERRORS2: &[(u8, u8, Error)] = &[
                    (3, 0x14, Error::PositioningError),
                    (5, 0x1A, Error::ParameterListLengthError),
                    (0xE, 0x1D, Error::MiscompareDuringVerify),
                    (5, 0x20, Error::InvalidCommandOperationCode),
                    (0xD, 0x21, Error::LogicalBlockAddressOutOfRange),
                    (5, 0x24, Error::InvalidFieldInCDB),
                    (5, 0x25, Error::LogicalUnitNotSupported),
                ];
                const ERRORS1: &[(u8, Error)] = &[
                    (2, Error::NotReady),
                    (3, Error::MediumError),
                    (4, Error::HardwareError),
                    (5, Error::IllegalRequest),
                    (6, Error::UnitAttention),
                    (7, Error::DataProtect),
                    (8, Error::BlankCheck),
                    (9, Error::VendorSpecific),
                    (10, Error::CopyAborted),
                    (11, Error::Aborted),
                    (13, Error::VolumeOverflow),
                    (14, Error::Miscompare),
                ];

                for i in ERRORS3 {
                    if r.sense_key == i.0
                        && r.additional_sense_code == i.1
                        && r.additional_sense_code_qualifier == i.2
                    {
                        return Err(i.3);
                    }
                }
                for i in ERRORS2 {
                    if r.sense_key == i.0 && r.additional_sense_code == i.1 {
                        return Err(i.2);
                    }
                }
                for i in ERRORS1 {
                    if r.sense_key == i.0 {
                        return Err(i.1);
                    }
                }
            }
        }
        e
    }

    async fn command_response<
        C: bytemuck::Pod,
        R: bytemuck::NoUninit + bytemuck::AnyBitPattern + Default,
    >(
        &mut self,
        cmd: C,
    ) -> Result<R, Error> {
        let mut r = R::default();
        let rc = self
            .transport
            .command(
                bytemuck::bytes_of(&cmd),
                DataPhase::In(bytemuck::bytes_of_mut(&mut r)),
            )
            .await;
        let sz = self.try_upgrade_error(rc).await?;
        if sz < core::mem::size_of::<R>() {
            return Err(Error::ProtocolError);
        }
        Ok(r)
    }

    /// Read capacity (32-bit LBA version, supports <2TB only)
    pub async fn read_capacity_10(&mut self) -> Result<(u32, u32), Error> {
        let rc = self.command_response(ReadCapacity10::new()).await;
        let reply: ReadCapacity10Reply = self.try_upgrade_error(rc).await?;
        let blocks = u32::from_be_bytes(reply.lba);
        let block_size = u32::from_be_bytes(reply.block_size);
        Ok((blocks, block_size))
    }

    /// Read capacity (64-bit LBA version, supports >2TB)
    ///
    /// Not universally supported.
    pub async fn read_capacity_16(&mut self) -> Result<(u64, u32), Error> {
        let rc = self.command_response(ReadCapacity16::new()).await;
        let reply: ReadCapacity16Reply = self.try_upgrade_error(rc).await?;
        let blocks = u64::from_be_bytes(reply.lba);
        let block_size = u32::from_be_bytes(reply.block_size);
        Ok((blocks, block_size))
    }

    /// Not much supports this one
    pub async fn report_supported_operation_codes(
        &mut self,
        opcode: u8,
        service_action: Option<u16>,
    ) -> Result<bool, Error> {
        let rc = self
            .command_response(ReportSupportedOperationCodes::new(
                opcode,
                service_action,
            ))
            .await;
        let reply: ReportSupportedOperationCodesReply =
            self.try_upgrade_error(rc).await?;
        Ok((reply.support & 7) == 3)
    }

    pub async fn test_unit_ready(&mut self) -> Result<(), Error> {
        let cmd = TestUnitReady::new();
        let rc = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::None)
            .await;
        let _ = self.try_upgrade_error(rc).await?;
        Ok(())
    }

    async fn request_sense(&mut self) -> Result<RequestSenseReply, Error> {
        let cmd = RequestSense::new();
        let mut buf = [0u8; 18];
        let sz = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::In(&mut buf))
            .await?;
        if sz < 18 {
            return Err(Error::ProtocolError);
        }
        let reply = bytemuck::try_from_bytes::<RequestSenseReply>(&buf)
            .map_err(|_| Error::ProtocolError)?;
        debug::println!("{:?}", *reply);
        Ok(*reply)
    }

    pub async fn inquiry(&mut self) -> Result<InquiryData, Error> {
        let rc = self.command_response(Inquiry::new(None, 36)).await;
        let reply: StandardInquiryData = self.try_upgrade_error(rc).await?;
        let data = InquiryData {
            peripheral_type: unsafe {
                core::mem::transmute::<u8, PeripheralType>(
                    reply.peripheral_device_type & 0x1F,
                )
            },
            is_removable: (reply.removable & 0x80) != 0,
        };
        //debug::println!("actual len {}", reply.additional_length + 4);
        if let (Ok(v), Ok(i), Ok(r)) = (
            str::from_utf8(&reply.vendor_id),
            str::from_utf8(&reply.product_id),
            str::from_utf8(&reply.product_revision),
        ) {
            debug::println!("v {} i {} r {}", v, i, r);
        }
        debug::println!(
            "type {:x} removable {}",
            reply.peripheral_device_type,
            reply.removable
        );
        Ok(data)
    }

    /*
    pub async fn supported_vpd_pages(&mut self) -> Result<(), Error> {
        let cmd = Inquiry::new(Some(0), 4);
        let rc = self.command_response(cmd).await;
        let n: [u8; 4] = self.try_upgrade_error(rc).await?;
        debug::println!("vpd 0x{:x}", n);

        if n[3] >= 3 {
            let cmd = Inquiry::new(Some(0), 7);
            let rc = self.command_response(cmd).await;
            let arr: [u8; 7] = self.try_upgrade_error(rc).await?;
            debug::println!("vpd {:?}", arr);
        }
        Ok(())
    }
    */

    /// Return Vital Product Data, Block Limits Page
    ///
    /// Which is meant to contain important information like maximum write
    /// size and optimum write granularity, but not much seems to support it.
    pub async fn block_limits_page(
        &mut self,
    ) -> Result<BlockLimitsPage, Error> {
        let cmd = Inquiry::new(Some(0xB0), 64);
        assert!(core::mem::size_of::<BlockLimitsPage>() == 64);
        let rc = self.command_response(cmd).await;
        let page = self.try_upgrade_error(rc).await?;
        Ok(page)
    }

    /// Read sector(s), 32-bit LBA version
    ///
    pub async fn read_10(
        &mut self,
        start_block: u32,
        count: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        let cmd = Read10::new(start_block, count);
        let rc = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::In(buf))
            .await;
        let sz = self.try_upgrade_error(rc).await?;
        Ok(sz)
    }

    /// Read sector(s), 64-bit LBA version
    ///
    /// Not universally supported.
    pub async fn read_16(
        &mut self,
        start_block: u64,
        count: u32,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        let cmd = Read16::new(start_block, count);
        let rc = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::In(buf))
            .await;
        let sz = self.try_upgrade_error(rc).await?;
        Ok(sz)
    }

    /// Write sector(s), 32-bit LBA version
    ///
    pub async fn write_10(
        &mut self,
        start_block: u32,
        count: u16,
        buf: &[u8],
    ) -> Result<usize, Error> {
        let cmd = Write10::new(start_block, count);
        let rc = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::Out(buf))
            .await;
        let sz = self.try_upgrade_error(rc).await?;
        Ok(sz)
    }

    /// Write sector(s), 64-bit LBA version
    ///
    /// Not universally supported.
    pub async fn write_16(
        &mut self,
        start_block: u64,
        count: u32,
        buf: &[u8],
    ) -> Result<usize, Error> {
        let cmd = Write16::new(start_block, count);
        let rc = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::Out(buf))
            .await;
        let sz = self.try_upgrade_error(rc).await?;
        Ok(sz)
    }
}
