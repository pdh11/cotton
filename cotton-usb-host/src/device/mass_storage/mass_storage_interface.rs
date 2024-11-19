use crate::debug;
use crate::device::identify::IdentifyFromDescriptors;
use crate::device::mass_storage::{Error, ScsiTransport};
use crate::host_controller::{DataPhase, HostController, UsbError};
use crate::usb_bus::{BulkIn, BulkOut, TransferType, UsbBus, UsbDevice};
use crate::wire::{
    ConfigurationDescriptor, DescriptorVisitor, InterfaceDescriptor,
};

pub struct MassStorageInterface<'a, HC: HostController> {
    bus: &'a UsbBus<HC>,
    //device: UsbDevice,
    bulk_in: BulkIn,
    bulk_out: BulkOut,
    tag: u32,
}

impl<'a, HC: HostController> MassStorageInterface<'a, HC> {
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
pub struct IdentifyMassStorageInterface {
    current_configuration: Option<u8>,
    msc_configuration: Option<u8>,
}

impl DescriptorVisitor for IdentifyMassStorageInterface {
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

impl IdentifyFromDescriptors for IdentifyMassStorageInterface {
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

impl<HC: HostController> ScsiTransport for MassStorageInterface<'_, HC> {
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
                TransferType::FixedSize,
            )
            .await?;
        //debug::println!("bot {:?}", rc);
        //rc?;

        let response = match data {
            DataPhase::In(buf) => {
                let rc = self
                    .bus
                    .bulk_in_transfer(
                        &self.bulk_in,
                        buf,
                        TransferType::FixedSize,
                    )
                    .await;
                if let Ok(n) = rc {
                    if n > 128 {
                        debug::println!("{}: [...]", n);
                    } else {
                        debug::println!("{}: {:?}", n, buf);
                    }
                }
                rc
            }
            DataPhase::Out(buf) => {
                self.bus
                    .bulk_out_transfer(
                        &self.bulk_out,
                        buf,
                        TransferType::FixedSize,
                    )
                    .await
            }
            DataPhase::None => Ok(0),
        };
        let response = if response == Err(UsbError::Stall) {
            debug::println!("msc bulk stall");
            self.bus.clear_halt(&self.bulk_in).await?;
            0
        } else {
            response?
        };

        let mut csw = [0u8; 13];
        let sz = self
            .bus
            .bulk_in_transfer(&self.bulk_in, &mut csw, TransferType::FixedSize)
            .await?;
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
