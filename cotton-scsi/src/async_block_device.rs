use core::future::Future;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub blocks: u64,
    pub block_size: u32,
}

pub trait AsyncBlockDevice {
    type E;

    fn device_info(
        &mut self,
    ) -> impl Future<Output = Result<DeviceInfo, Self::E>>;

    fn read_blocks(
        &mut self,
        offset: u64,
        count: u32,
        data: &mut [u8],
    ) -> impl Future<Output = Result<(), Self::E>>;

    fn write_blocks(
        &mut self,
        offset: u64,
        count: u32,
        data: &[u8],
    ) -> impl Future<Output = Result<(), Self::E>>;
}
