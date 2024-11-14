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
    ) -> impl Future<Output = Result<(), Error>>;
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScsiError {
    BadSense,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Error {
    Scsi(ScsiError),
    Usb(UsbError),
}

impl From<UsbError> for Error {
    fn from(e: UsbError) -> Self {
        Error::Usb(e)
    }
}

struct ScsiCommands<T: ScsiTransport> {
    transport: T,
}

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

impl<T: ScsiTransport> ScsiCommands<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub async fn read_capacity_16(&self) -> Result<u64, Error> {
        let cmd = ReadCapacity16::new();
        let mut buf = [0u8; 32];
        self.transport
            .command(bytemuck::bytes_of(&cmd), DataPhase::In(&mut buf))
            .await?;
        // TODO: parse answer from buf
        Ok(0)
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
    command: [u8; 16],
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
    ) -> Result<(), Error> {
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
        self.bus
            .bulk_out_transfer(
                &self.bulk_out,
                &bytemuck::bytes_of(&cbw)[0..31],
            )
            .await?;
        match data {
            DataPhase::In(buf) => {
                let n = self.bus.bulk_in_transfer(&self.bulk_in, buf).await?;
                debug::println!("{:?}", buf);
                n
            }
            DataPhase::Out(buf) => {
                self.bus.bulk_out_transfer(&self.bulk_out, buf).await?
            }
            DataPhase::None => 0,
        };

        let mut response = [0u8; 13];
        self.bus
            .bulk_in_transfer(&self.bulk_in, &mut response)
            .await?;
        debug::println!("response {:?}", response);
        // TODO: was response an error?
        Ok(())
    }
}

pub trait AsyncBlockDevice {
    type E;

    fn capacity(&self) -> impl Future<Output = Result<u64, Self::E>>;
}

pub struct ScsiBlockDevice<T: ScsiTransport> {
    scsi: ScsiCommands<T>,
}

impl<T: ScsiTransport> ScsiBlockDevice<T> {
    pub fn new(scsi: T) -> Self {
        Self {
            scsi: ScsiCommands::new(scsi),
        }
    }
}

impl<T: ScsiTransport> AsyncBlockDevice for ScsiBlockDevice<T> {
    type E = Error;

    async fn capacity(&self) -> Result<u64, Self::E> {
        self.scsi.read_capacity_16().await
    }
}
