use core::ops::Deref;

pub struct InterruptPacket {
    pub size: u8,
    pub data: [u8; 64],
}

impl Default for InterruptPacket {
    fn default() -> Self {
        Self::new()
    }
}

impl InterruptPacket {
    pub const fn new() -> Self {
        Self {
            size: 0,
            data: [0u8; 64],
        }
    }
}

impl Deref for InterruptPacket {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data[0..(self.size as usize)]
    }
}

pub trait InterruptPipe {
    fn set_waker(&mut self, waker: &core::task::Waker);
    fn poll(&mut self) -> Option<InterruptPacket>;
}

pub trait Driver {
    type InterruptPipe<'driver>: InterruptPipe
    where
        Self: 'driver;

    fn alloc_interrupt_pipe(
        &mut self,
    ) -> impl core::future::Future<Output = Self::InterruptPipe<'_>>;
    //    fn alloc_interrupt_pipe<'driver>(&'driver mut self) -> impl core::future::Future<Output = Self::InterruptPipe<'driver>>;
}
