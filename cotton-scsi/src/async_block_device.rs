use core::future::Future;

/// Device size and granularity information
///
/// The total size of the device in bytes is `blocks * block_size`.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    /// The total number of sectors (readable/writable blocks) on the device
    pub blocks: u64,

    /// The size of each block
    pub block_size: u32,
}

/// A generic, asynchronous, read/write block device
///
/// Represents a storage device which can only be read or written-to
/// in whole blocks, at block-aligned addresses (where the size of a
/// *block* is as provided in `DeviceInfo.block_size` -- very commonly
/// 512 bytes, though devices with 4096-byte blocks are also seen).
pub trait AsyncBlockDevice {
    /// The type of errors which this device can report
    type E;

    /// Return the capacity and block-size (granularity) of this device
    fn device_info(
        &mut self,
    ) -> impl Future<Output = Result<DeviceInfo, Self::E>>;

    /// # Read a block or blocks from the device
    ///
    /// Reads `count` blocks from the device, starting from the `offset`-th
    /// block (0-based), and stores the data in the supplied buffer.
    ///
    /// The buffer should be at least `count * DeviceInfo.block_size`
    /// bytes in size; if not, this call returns an error. In particular, this
    /// means that *partial* blocks cannot be read (or written) -- just whole
    /// blocks.
    fn read_blocks(
        &mut self,
        offset: u64,
        count: u32,
        data: &mut [u8],
    ) -> impl Future<Output = Result<(), Self::E>>;

    /// # Write a block or blocks to the device
    ///
    /// Writes the data in the supplied buffer to `count` blocks on
    /// the device, starting from the `offset`-th block (0-based).
    ///
    /// The buffer should be at least `count * DeviceInfo.block_size`
    /// bytes in size; if not, this call returns an error. In particular, this
    /// means that *partial* blocks cannot be written (or read) -- just whole
    /// blocks.
    fn write_blocks(
        &mut self,
        offset: u64,
        count: u32,
        data: &[u8],
    ) -> impl Future<Output = Result<(), Self::E>>;
}
