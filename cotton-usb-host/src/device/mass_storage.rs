use crate::debug;
use crate::device::identify::UsbIdentify;
use crate::host_controller::{DataPhase, HostController, UsbError};
use crate::usb_bus::{
    BulkIn, BulkOut, DeviceInfo, UnconfiguredDevice, UsbBus, UsbDevice,
};
use core::future::Future;

pub trait ScsiTransport {
    fn command(
        &self,
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
#[derive(Copy, Clone)]
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

    pub async fn read_capacity_16(&self) -> Result<(u64, u32), Error> {
        let cmd = ReadCapacity16::new();
        let mut buf = [0u8; 32];
        let sz = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::In(&mut buf))
            .await?;
        if sz < 32 {
            return Err(Error::ProtocolError);
        }
        let reply = bytemuck::try_from_bytes::<ReadCapacity16Reply>(&buf)
            .map_err(|_| Error::ProtocolError)?;
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

    pub async fn test_unit_ready(&self) -> Result<(), Error> {
        let cmd = TestUnitReady::new();
        self.transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::None)
            .await?;
        Ok(())
    }

    pub async fn inquiry(&self) -> Result<InquiryData, Error> {
        let cmd = Inquiry::new(None, 36);
        let mut buf = [0u8; 36];
        let sz = self
            .transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::In(&mut buf))
            .await?;
        debug::println!("got {}", sz);
        if sz < 36 {
            return Err(Error::ProtocolError);
        }
        debug::println!("casting");
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
        debug::println!("actual len {}", reply.additional_length + 4);
        debug::println!(
            "type {:x} removable {}",
            reply.peripheral_device_type,
            reply.removable
        );
        Ok(data)
    }
}

pub struct MassStorage<'a, HC: HostController> {
    bus: &'a UsbBus<HC>,
    //device: UsbDevice,
    bulk_in: BulkIn,
    bulk_out: BulkOut,
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
        })
    }
}

impl<HC: HostController> UsbIdentify<HC> for MassStorage<'_, HC> {
    fn identify(
        _bus: &UsbBus<HC>,
        _device: &UnconfiguredDevice,
        info: &DeviceInfo,
    ) -> Option<u8> {
        // TODO: examine interface descriptor!
        if info.vid == 0x0781 && info.pid == 0x5567 {
            Some(1)
        } else {
            None
        }
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
        &self,
        cmd: &[u8],
        data: DataPhase<'_>,
    ) -> Result<usize, Error> {
        let len = match data {
            DataPhase::In(ref buf) => buf.len(),
            DataPhase::Out(buf) => buf.len(),
            DataPhase::None => 0,
        };
        let flags = match data {
            DataPhase::In(_) => 0x80,
            _ => 0,
        };
        let cbw = CommandBlockWrapper::new(1, len as u32, flags, cmd);
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

        // TODO: if in and sz<13, read to cbw instead in case command errors
        let response = match data {
            DataPhase::In(buf) => {
                let n = self.bus.bulk_in_transfer(&self.bulk_in, buf).await?;
                debug::println!("{}: {:?}", n, buf);
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
        let residue = u32::from_le_bytes(&csw[8..12]);
         */
        let status = csw[12];
        debug::println!("status {}", status);
        match status {
            0 => Ok(response),
            1 => Err(Error::CommandFailed),
            _ => Err(Error::ProtocolError),
        }
    }
}

pub trait AsyncBlockDevice {
    type E;

    fn capacity(&self) -> impl Future<Output = Result<(u64, u32), Self::E>>;
}

pub struct ScsiBlockDevice<T: ScsiTransport> {
    scsi: ScsiDevice<T>,
}

impl<T: ScsiTransport> ScsiBlockDevice<T> {
    pub fn new(scsi: ScsiDevice<T>) -> Self {
        Self { scsi }
    }
}

impl<T: ScsiTransport> AsyncBlockDevice for ScsiBlockDevice<T> {
    type E = Error;

    async fn capacity(&self) -> Result<(u64, u32), Self::E> {
        self.scsi.read_capacity_16().await
    }
}
