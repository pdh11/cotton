use super::async_block_device::{AsyncBlockDevice, DeviceInfo};
use super::debug;
use super::scsi_device::ScsiDevice;
use super::scsi_transport::{Error, ScsiError, ScsiTransport};

/// Implementing [`AsyncBlockDevice`] in terms of [`ScsiDevice`]
pub struct ScsiBlockDevice<T: ScsiTransport> {
    /// The underlying SCSI block device
    ///
    /// Made "pub" so that additional SCSI commands can be issued if need be.
    pub scsi: ScsiDevice<T>,
}

impl<T: ScsiTransport> ScsiBlockDevice<T> {
    /// Construct a new block device from a generic SCSI device
    pub fn new(scsi: ScsiDevice<T>) -> Self {
        Self { scsi }
    }

    /// For testing: query supported SCSI commands on this device
    ///
    /// Unfortunately, "Report Supported Operation Codes", which this
    /// relies on, is itself rarely a supported SCSI command: I
    /// haven't found a device yet where this call works. Instead it
    /// always returns `ScsiError::InvalidCommandOperationCode`.
    pub async fn query_commands(&mut self) -> Result<(), Error<T::Error>> {
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
            let ok = self
                .scsi
                .report_supported_operation_codes(c.1, None)
                .await?;
            debug::println!("{} {}", c.0, ok);
        }
        Ok(())
    }
}

impl<T: ScsiTransport> AsyncBlockDevice for ScsiBlockDevice<T> {
    type E = Error<T::Error>;

    async fn device_info(&mut self) -> Result<DeviceInfo, Self::E> {
        let (blocks, block_size) = {
            let capacity10 = self.scsi.read_capacity_10().await?;
            if capacity10.0 != 0xFFFF_FFFF {
                (capacity10.0 as u64, capacity10.1)
            } else {
                self.scsi.read_capacity_16().await?
            }
        };

        Ok(DeviceInfo { blocks, block_size })
    }

    async fn read_blocks(
        &mut self,
        offset: u64,
        count: u32,
        data: &mut [u8],
    ) -> Result<(), Self::E> {
        let end = offset
            .checked_add(count as u64)
            .ok_or(Error::Scsi(ScsiError::LogicalBlockAddressOutOfRange))?;
        let sz = if end < u32::MAX as u64 && count < u16::MAX as u32 {
            self.scsi.read_10(offset as u32, count as u16, data).await?
        } else {
            self.scsi.read_16(offset, count, data).await?
        };
        if sz < data.len() {
            return Err(Error::ProtocolError);
        }
        Ok(())
    }

    async fn write_blocks(
        &mut self,
        offset: u64,
        count: u32,
        data: &[u8],
    ) -> Result<(), Self::E> {
        let end = offset
            .checked_add(count as u64)
            .ok_or(Error::Scsi(ScsiError::LogicalBlockAddressOutOfRange))?;
        if end < u32::MAX as u64 && count < u16::MAX as u32 {
            self.scsi
                .write_10(offset as u32, count as u16, data)
                .await?;
        } else {
            self.scsi.write_16(offset, count, data).await?;
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "std"))]
#[path = "tests/scsi_block_device.rs"]
pub(crate) mod tests;
