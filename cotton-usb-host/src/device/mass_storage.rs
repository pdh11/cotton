use crate::debug;
use crate::device::identify::IdentifyFromDescriptors;
use crate::host_controller::{DataPhase, HostController, UsbError};
use crate::usb_bus::{BulkIn, BulkOut, UsbBus, UsbDevice};
use crate::wire::{
    ConfigurationDescriptor, DescriptorVisitor, InterfaceDescriptor,
};
use core::future::Future;
use core::str;

pub trait ScsiTransport {
    fn command(
        &mut self,
        cmd: &[u8],
        data: DataPhase,
    ) -> impl Future<Output = Result<usize, Error>>;
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    CommandFailed,
    ProtocolError,
    Usb(UsbError),
}

impl From<UsbError> for Error {
    fn from(e: UsbError) -> Self {
        Error::Usb(e)
    }
}

pub struct ScsiDevice<T: ScsiTransport> {
    transport: T,
}

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
            reporting_options: 3, //if service_action.is_some() { 2 } else { 1 },
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
#[derive(Copy, Clone)]
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

impl<T: ScsiTransport> ScsiDevice<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    async fn command_response<
        C: bytemuck::Pod,
        R: bytemuck::NoUninit + bytemuck::AnyBitPattern + Default,
    >(
        &mut self,
        cmd: C,
    ) -> Result<R, Error> {
        let mut r = R::default();
        let sz = self
            .transport
            .command(
                bytemuck::bytes_of(&cmd),
                DataPhase::In(bytemuck::bytes_of_mut(&mut r)),
            )
            .await?;
        if sz < core::mem::size_of::<R>() {
            return Err(Error::ProtocolError);
        }
        Ok(r)
    }

    /// Read capacity (basic version supports <2TB only)
    ///
    /// Support:
    /// Sandisk cruzer blade (black 4G) Y
    /// Sandisk cruzer blade (green 16G) Y
    /// Handbag Y
    /// Poker Y
    pub async fn read_capacity_10(&mut self) -> Result<(u32, u32), Error> {
        let reply: ReadCapacity10Reply =
            self.command_response(ReadCapacity10::new()).await?;
        let blocks = u32::from_be_bytes(reply.lba);
        let block_size = u32::from_be_bytes(reply.block_size);
        let capacity = (blocks as u64) * (block_size as u64);
        debug::println!(
            "{} blocks x {} bytes = {} B / {} KB / {} MB / {} GB",
            blocks,
            block_size,
            capacity,
            capacity >> 10,
            capacity >> 20,
            capacity >> 30
        );
        Ok((blocks, block_size))
    }

    /// Read capacity (extended version supports >2TB)
    ///
    /// Support:
    /// Sandisk cruzer blade (black 4G) Y
    /// Sandisk cruzer blade (green 16G) Y
    /// Handbag N
    /// Poker N
    pub async fn read_capacity_16(&mut self) -> Result<(u64, u32), Error> {
        let reply: ReadCapacity16Reply =
            self.command_response(ReadCapacity16::new()).await?;
        let blocks = u64::from_be_bytes(reply.lba);
        let block_size = u32::from_be_bytes(reply.block_size);
        let capacity = blocks * (block_size as u64);
        debug::println!(
            "{} blocks x {} bytes = {} B / {} KB / {} MB / {} GB",
            blocks,
            block_size,
            capacity,
            capacity >> 10,
            capacity >> 20,
            capacity >> 30
        );
        Ok((blocks, block_size))
    }

    /// Not much supports this one
    pub async fn report_supported_operation_codes(
        &mut self,
        opcode: u8,
        service_action: Option<u16>,
    ) -> Result<bool, Error> {
        let reply: ReportSupportedOperationCodesReply = self
            .command_response(ReportSupportedOperationCodes::new(
                opcode,
                service_action,
            ))
            .await?;
        Ok((reply.support & 7) == 3)
    }

    pub async fn test_unit_ready(&mut self) -> Result<(), Error> {
        let cmd = TestUnitReady::new();
        let rc = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::None)
            .await;
        debug::println!("tur: {:?}", rc);
        rc?;
        Ok(())
    }

    pub async fn request_sense(&mut self) -> Result<u8, Error> {
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
        debug::println!("{:?}", reply);
        Ok(reply.sense_key)
    }

    pub async fn inquiry(&mut self) -> Result<InquiryData, Error> {
        let cmd = Inquiry::new(None, 36);
        let mut buf = [0u8; 36];
        let sz = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::In(&mut buf))
            .await?;
        if sz < 36 {
            return Err(Error::ProtocolError);
        }
        let reply = bytemuck::try_from_bytes::<StandardInquiryData>(&buf)
            .map_err(|_| Error::ProtocolError)?;
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

    /// Not much supports this one
    pub async fn block_limits_page(
        &mut self,
    ) -> Result<BlockLimitsPage, Error> {
        let cmd = Inquiry::new(Some(0xB0), 64);
        assert!(core::mem::size_of::<BlockLimitsPage>() == 64);
        self.command_response(cmd).await
    }

    /// Read sector(s)
    ///
    /// Support:
    /// Sandisk cruzer blade (black 4G)
    /// Sandisk cruzer blade (green 16G) Y
    /// Handbag
    /// Poker
    pub async fn read_16(
        &mut self,
        start_block: u64,
        count: u32,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        let cmd = Read16::new(start_block, count);
        let sz = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::In(buf))
            .await?;
        Ok(sz)
    }
}

pub struct MassStorage<'a, HC: HostController> {
    bus: &'a UsbBus<HC>,
    //device: UsbDevice,
    bulk_in: BulkIn,
    bulk_out: BulkOut,
    tag: u32,
}

impl<'a, HC: HostController> MassStorage<'a, HC> {
    pub fn new(
        bus: &'a UsbBus<HC>,
        mut device: UsbDevice,
    ) -> Result<Self, Error> {
        let bulk_in = device
            .open_in_endpoint(device.in_endpoints().iter().next().unwrap())?;
        let bulk_out = device.open_out_endpoint(
            device.out_endpoints().iter().next().unwrap(),
        )?;
        Ok(Self {
            bus,
            //device,
            bulk_in,
            bulk_out,
            tag: 1,
        })
    }
}

#[derive(Default)]
pub struct IdentifyMassStorage {
    current_configuration: Option<u8>,
    msc_configuration: Option<u8>,
}

impl DescriptorVisitor for IdentifyMassStorage {
    fn on_configuration(&mut self, c: &ConfigurationDescriptor) {
        self.current_configuration = Some(c.bConfigurationValue);
    }
    fn on_interface(&mut self, i: &InterfaceDescriptor) {
        if i.bInterfaceClass == 8 && i.bInterfaceProtocol == 0x50 {
            self.msc_configuration = self.current_configuration;
        } else {
            debug::println!(
                "class {} subclass {} protocol {}",
                i.bInterfaceClass,
                i.bInterfaceSubClass,
                i.bInterfaceProtocol
            );
        }
    }
}

impl IdentifyFromDescriptors for IdentifyMassStorage {
    fn identify(&self) -> Option<u8> {
        self.msc_configuration
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
struct CommandBlockWrapper {
    signature: u32, // note CBW is little-endian even though SCSI is big-endian
    tag: u32,
    data_transfer_length: u32,
    flags: u8,
    lun: u8,
    command_length: u8,
    command: [u8; 17],
}

// SAFETY: all fields zeroable
unsafe impl bytemuck::Zeroable for CommandBlockWrapper {}
// SAFETY: no padding, no disallowed bit patterns
unsafe impl bytemuck::Pod for CommandBlockWrapper {}

impl CommandBlockWrapper {
    fn new(
        tag: u32,
        data_transfer_length: u32,
        flags: u8,
        command: &[u8],
    ) -> Self {
        let mut cbw = Self {
            signature: 0x43425355,
            tag,
            data_transfer_length,
            flags,
            lun: 0,
            command_length: command.len() as u8,
            command: Default::default(),
        };
        cbw.command[0..command.len()].copy_from_slice(command);
        cbw
    }
}

impl<HC: HostController> ScsiTransport for MassStorage<'_, HC> {
    async fn command(
        &mut self,
        cmd: &[u8],
        data: DataPhase<'_>,
    ) -> Result<usize, Error> {
        //let rc = self.bus.clear_halt(&self.bulk_in).await;
        //debug::println!("clear {:?}", rc);

        self.tag += 2;

        let len = match data {
            DataPhase::In(ref buf) => buf.len(),
            DataPhase::Out(buf) => buf.len(),
            DataPhase::None => 0,
        };
        let flags = match data {
            DataPhase::In(_) => 0x80,
            _ => 0,
        };
        let cbw = CommandBlockWrapper::new(self.tag, len as u32, flags, cmd);
        // NB the CommandBlockWrapper struct has no padding as
        // defined, but it's one byte too long (an actual, on-the-wire
        // command block wrapper is 31 bytes). So we only send a
        // partial slice of it.
        self.bus
            .bulk_out_transfer(
                &self.bulk_out,
                &bytemuck::bytes_of(&cbw)[0..31],
            )
            .await?;
        //debug::println!("bot {:?}", rc);
        //rc?;

        // TODO: if in and sz<13, read to cbw instead in case command errors
        let response = match data {
            DataPhase::In(buf) => {
                let n = self.bus.bulk_in_transfer(&self.bulk_in, buf).await?;
                if n > 128 {
                    debug::println!("{}: [...]", n);
                } else {
                    debug::println!("{}: {:?}", n, buf);
                }
                n
            }
            DataPhase::Out(buf) => {
                self.bus.bulk_out_transfer(&self.bulk_out, buf).await?
            }
            DataPhase::None => 0,
        };

        let mut csw = [0u8; 13];
        let sz = self.bus.bulk_in_transfer(&self.bulk_in, &mut csw).await?;
        if sz < 13 {
            debug::println!("Bad CSW {}/13", sz);
            return Err(Error::ProtocolError);
        }
        /*
        let sig = u32::from_le_bytes(&csw[0..4]);
        let tag = u32::from_le_bytes(&csw[4..8]);
         */
        let residue = u32::from_le_bytes(csw[8..12].try_into().unwrap());
        let status = csw[12];
        if status != 0 || residue != 0 {
            debug::println!("status {} residue {}", status, residue);
        }
        match status {
            0 => Ok(response),
            1 => Err(Error::CommandFailed),
            _ => Err(Error::ProtocolError),
        }
    }
}

pub trait AsyncBlockDevice {
    type E;

    fn capacity(
        &mut self,
    ) -> impl Future<Output = Result<(u64, u32), Self::E>>;
}

pub struct ScsiBlockDevice<T: ScsiTransport> {
    scsi: ScsiDevice<T>,
}

impl<T: ScsiTransport> ScsiBlockDevice<T> {
    pub fn new(scsi: ScsiDevice<T>) -> Self {
        Self { scsi }
    }

    /*
    pub async fn query_commands(&mut self) -> Result<(),Error> {
        const CMDS: &[(&str, u8)] = &[
            ("READ(6)", 0x08),
            ("READ(10)", 0x28),
            ("READ(12)", 0xA8),
            ("READ(16)", 0x88),
            ("WRITE(6)", 0x0A),
            ("WRITE(10)", 0x2A),
            ("WRITE(12)", 0xAA),
            ("WRITE(16)", 0x8A),
            ("WRITE ATOMIC(16)", 0x9C),
            ("WRITE AND VERIFY(16)", 0x8E),
        ];

        for c in CMDS {
            let ok = self.scsi.report_supported_operation_codes(c.1, None).await?;
            debug::println!("{} {}", c.0, ok);
        }
        Ok(())
    }
     */
}

impl<T: ScsiTransport> AsyncBlockDevice for ScsiBlockDevice<T> {
    type E = Error;

    async fn capacity(&mut self) -> Result<(u64, u32), Self::E> {
        let capacity10 = self.scsi.read_capacity_10().await?;
        if capacity10.0 != 0xFFFF_FFFF {
            return Ok((capacity10.0 as u64, capacity10.1));
        }

        // NB 4 giga*blocks* is a lot, >=2TB
        self.scsi.read_capacity_16().await
    }
}
