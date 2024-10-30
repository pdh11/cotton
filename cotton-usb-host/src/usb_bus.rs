use crate::bitset::{BitIterator, BitSet};
use crate::debug;
use crate::host_controller::{
    DeviceStatus, HostController, InterruptPacket, MultiInterruptPipe,
};
use crate::interrupt::{InterruptStream, MultiInterruptStream};
use crate::topology::Topology;
use crate::types::{
    ConfigurationDescriptor, DescriptorVisitor, DeviceInfo,
    EndpointDescriptor, HubDescriptor, SetupPacket, UsbDevice, UsbError,
    UsbSpeed, CLASS_REQUEST, CLEAR_FEATURE, CONFIGURATION_DESCRIPTOR,
    DEVICE_DESCRIPTOR, DEVICE_TO_HOST, GET_DESCRIPTOR, GET_STATUS,
    HOST_TO_DEVICE, HUB_CLASSCODE, HUB_DESCRIPTOR, PORT_POWER, PORT_RESET,
    RECIPIENT_OTHER, SET_ADDRESS, SET_CONFIGURATION, SET_FEATURE,
};
use core::cell::RefCell;
use futures::future::FutureExt;
use futures::{Stream, StreamExt};

pub use crate::host_controller::DataPhase;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(PartialEq, Eq)]
/// A device-related event has occurred.
///
/// This is the type of events returned from
/// [`UsbBus::device_events()`] or [`UsbBus::device_events_no_hubs`].
/// The important events are hot-plug ("`Connect`") and
/// hot-unplug("`Disconnect`"). Device events are how your code can
/// detect the presence of USB devices and start to communicate with
/// them.
///
pub enum DeviceEvent {
    /// A new device has been connected. It has been given an address,
    /// but has not yet been configured (state "Address" in USB 2.0
    /// figure 9-1). Your code can read the device's descriptors to
    /// confirm its identity and figure out what to do with it, but
    /// must then call [`UsbBus::configure()`] before communicating
    /// with it "for real".
    ///
    /// The `UsbDevice` object encapsulates the newly-assigned USB device
    /// address. Basic information about the device -- sufficient, perhaps, to
    /// select an appropriate driver from those available -- is in the
    /// supplied `DeviceInfo`.
    Connect(UsbDevice, DeviceInfo),

    /// A previously-reported device has become disconnected. This event
    /// includes a _set_ of affected devices -- if a hub has become
    /// disconnected, then every device downstream of it has simultaneously
    /// _also_ become disconnected.
    ///
    /// The devices are represented by a bitmap: bit N set means that USB
    /// device address N is part of this set.
    ///
    /// (So bit zero is never set, because 0 is never a valid assigned USB
    /// device address.)
    Disconnect(BitSet),

    /// A device appears to have been connected, but is not
    /// successfully responding to the mandatory enumeration commands.
    /// This usually indicates inadequate power supply, or perhaps
    /// damaged cabling.
    EnumerationError(u8, u8, UsbError),

    /// There is nothing currently to report. (This event is sometimes sent
    /// for internal reasons, and can be ignored.)
    None,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Default, PartialEq, Eq)]
pub struct BasicConfiguration {
    pub num_configurations: u8,
    pub configuration_value: u8,
    pub in_endpoints: u16,
    pub out_endpoints: u16,
}

impl DescriptorVisitor for BasicConfiguration {
    fn on_configuration(&mut self, c: &ConfigurationDescriptor) {
        self.num_configurations += 1;
        self.configuration_value = c.bConfigurationValue;
    }
    fn on_endpoint(&mut self, i: &EndpointDescriptor) {
        if (i.bEndpointAddress & 0x80) == 0x80 {
            self.in_endpoints |= 1 << (i.bEndpointAddress & 15);
        } else {
            self.out_endpoints |= 1 << (i.bEndpointAddress & 15);
        }
    }
}

#[derive(Default)]
struct HubState {
    topology: Topology,
    currently_resetting: Option<(u8, u8)>,
    //needs_reset: BitSet,
}

/// A USB host bus.
///
/// This object represents the (portable) concept of a host's view of
/// a whole bus of USB devices. It is constructed from a
/// [`HostController`] object which encapsulates the driver for
/// specific USB-host-controller hardware.
///
/// Starting from a `UsbBus` object, you can obtain details of any USB
/// devices attached to this host, and start communicating with them as
/// needed.
///
/// Devices with multiple USB host controllers will require a `UsbBus`
/// object for each of them.
///
pub struct UsbBus<HC: HostController> {
    driver: HC,
    hub_pipes: RefCell<HC::MultiInterruptPipe>,
    hub_state: RefCell<HubState>,
}

impl<HC: HostController> UsbBus<HC> {
    /// Create a new USB host bus from a host-controller driver
    pub fn new(driver: HC) -> Self {
        let hp = driver.multi_interrupt_pipe();

        Self {
            driver,
            hub_pipes: RefCell::new(hp),
            hub_state: Default::default(),
        }
    }

    /// Obtain a stream of hotplug/hot-unplug events
    ///
    /// This stream is how the USB host stack informs your code that a
    /// USB device is available for use. Once you have [created a
    /// `UsbBus` object](UsbBus::new()), you can call `device_events()` and
    /// get a stream of [`DeviceEvent`] objects:
    ///
    /// ```no_run
    /// # use cotton_usb_host::host_controller::HostController;
    /// # use std::pin::{pin, Pin};
    /// # use std::task::{Context, Poll, Waker};
    /// # use cotton_usb_host::usb_bus::UsbBus;
    /// # use cotton_usb_host::host_controller::{InterruptPipe, MultiInterruptPipe, DataPhase, DeviceStatus};
    /// # use cotton_usb_host::host_controller::InterruptPacket;
    /// # use cotton_usb_host::types::{SetupPacket, UsbError};
    /// # use futures::{Stream, StreamExt};
    /// # struct Driver;
    /// # struct Foo;
    /// # impl Stream for Foo {
    /// # type Item = DeviceStatus;
    /// # fn poll_next(
    /// #       mut self: Pin<&mut Self>,
    /// #       cx: &mut Context<'_>,
    /// #   ) -> Poll<Option<Self::Item>> { todo!() }
    /// # }
    /// # impl<'a> InterruptPipe for &'a Foo {
    /// #     fn set_waker(&self, waker: &core::task::Waker) { todo!() }
    /// #     fn poll(&self) -> Option<InterruptPacket> { todo!() }
    /// # }
    /// # impl InterruptPipe for Foo {
    /// #     fn set_waker(&self, waker: &core::task::Waker) { todo!() }
    /// #     fn poll(&self) -> Option<InterruptPacket> { todo!() }
    /// # }
    /// # impl MultiInterruptPipe for Foo {
    /// # fn try_add(
    /// #  &mut self,
    /// # address: u8,
    /// # endpoint: u8,
    /// #       max_packet_size: u8,
    /// #    interval_ms: u8,
    /// #   ) -> Result<(), UsbError> { todo!() }
    /// # fn remove(&mut self, address: u8) { todo!() }
    /// # }
    /// # impl HostController for Driver {
    /// #     type InterruptPipe<'driver> = &'driver Foo;
    /// #     type MultiInterruptPipe = Foo; type DeviceDetect = Foo;
    /// # fn device_detect(&self) -> Self::DeviceDetect { todo!() }
    /// # fn control_transfer<'a>(&self,
    /// #   address: u8,
    /// #       packet_size: u8,
    /// #       setup: SetupPacket,
    /// #       data_phase: DataPhase<'a>,
    /// #   ) -> impl core::future::Future<Output = Result<usize, UsbError>> {
    /// #  async { todo!() } }
    /// # fn alloc_interrupt_pipe(
    /// # &self,
    /// #  address: u8,
    /// #    endpoint: u8,
    /// #   max_packet_size: u16,
    /// #    interval_ms: u8,
    /// # ) -> impl core::future::Future<Output = Self::InterruptPipe<'_>> {
    /// # async { todo!() } }
    /// #
    /// # fn multi_interrupt_pipe(&self) -> Self::MultiInterruptPipe { todo!() }
    /// # }
    /// # let driver = Driver;
    /// # pollster::block_on(async {
    /// let bus = UsbBus::new(driver);
    /// let mut device_stream = pin!(bus.device_events());
    /// loop {
    ///     let event = device_stream.next().await;
    ///     // ... process the event ...
    /// }
    /// # });
    /// ```
    ///
    /// When using this method, the cotton-usb-host crate itself takes
    /// care of detecting and configuring hubs, and of detecting
    /// devices downstream of hubs. At present, the hubs do themselves
    /// appear as `DeviceEvent`s, but your code doesn't need to do
    /// anything with them.
    ///
    /// If you know for a fact that your hardware setup does not
    /// include any hubs (or if you wish to operate the hubs
    /// yourself), you can use
    /// [`device_events_no_hubs()`](`UsbBus::device_events_no_hubs()`)
    /// instead of `device_events()` and get smaller, simpler code.
    ///
    pub fn device_events(&self) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = self.driver.device_detect();

        enum InternalEvent {
            Root(DeviceStatus),
            Packet(InterruptPacket),
        }

        futures::stream::select(
            root_device.map(InternalEvent::Root),
            MultiInterruptStream::<HC::MultiInterruptPipe> {
                pipe: &self.hub_pipes,
            }
            .map(InternalEvent::Packet),
        )
        .then(move |ev| async move {
            match ev {
                InternalEvent::Root(status) => {
                    if let DeviceStatus::Present(speed) = status {
                        let info = match self.new_device(speed).await {
                            Ok(info) => info,
                            Err(e) => {
                                return DeviceEvent::EnumerationError(0, 1, e)
                            }
                        };
                        let is_hub = info.class == HUB_CLASSCODE;
                        let address = self
                            .hub_state
                            .borrow_mut()
                            .topology
                            .device_connect(0, 1, is_hub)
                            .expect("Root connect should always succeed");
                        let device = match self
                            .set_address(address, &info)
                            .await
                        {
                            Ok(device) => device,
                            Err(e) => {
                                return DeviceEvent::EnumerationError(0, 1, e);
                            }
                        };
                        if is_hub {
                            debug::println!("It's a hub");
                            match self.new_hub(&device, &info).await {
                                Ok(()) => (),
                                Err(e) => {
                                    return DeviceEvent::EnumerationError(
                                        0, 1, e,
                                    )
                                }
                            };
                        }
                        DeviceEvent::Connect(device, info)
                    } else {
                        self.hub_state
                            .borrow_mut()
                            .topology
                            .device_disconnect(0, 1);
                        DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF))
                    }
                }
                InternalEvent::Packet(packet) => {
                    self.handle_hub_packet(&packet).await.unwrap_or_else(|e| {
                        DeviceEvent::EnumerationError(0, 1, e)
                    })
                }
            }
        })
    }

    /// Obtain a stream of hotplug/hot-unplug events
    ///
    /// This stream is how the USB host stack informs your code that a
    /// USB device is available for use. Once you have [created a
    /// `UsbBus` object](UsbBus::new()), you can call `device_events()` and
    /// get a stream of [`DeviceEvent`] objects:
    ///
    /// ```no_run
    /// # use cotton_usb_host::host_controller::HostController;
    /// # use std::pin::{pin, Pin};
    /// # use std::task::{Context, Poll, Waker};
    /// # use cotton_usb_host::usb_bus::UsbBus;
    /// # use cotton_usb_host::host_controller::{InterruptPipe, MultiInterruptPipe, DataPhase, DeviceStatus};
    /// # use cotton_usb_host::host_controller::InterruptPacket;
    /// # use cotton_usb_host::types::{SetupPacket, UsbError};
    /// # use futures::{Stream, StreamExt};
    /// # struct Driver;
    /// # struct Foo;
    /// # impl Stream for Foo {
    /// # type Item = DeviceStatus;
    /// # fn poll_next(
    /// #       mut self: Pin<&mut Self>,
    /// #       cx: &mut Context<'_>,
    /// #   ) -> Poll<Option<Self::Item>> { todo!() }
    /// # }
    /// # impl<'a> InterruptPipe for &'a Foo {
    /// #     fn set_waker(&self, waker: &core::task::Waker) { todo!() }
    /// #     fn poll(&self) -> Option<InterruptPacket> { todo!() }
    /// # }
    /// # impl InterruptPipe for Foo {
    /// #     fn set_waker(&self, waker: &core::task::Waker) { todo!() }
    /// #     fn poll(&self) -> Option<InterruptPacket> { todo!() }
    /// # }
    /// # impl MultiInterruptPipe for Foo {
    /// # fn try_add(
    /// #  &mut self,
    /// # address: u8,
    /// # endpoint: u8,
    /// #       max_packet_size: u8,
    /// #    interval_ms: u8,
    /// #   ) -> Result<(), UsbError> { todo!() }
    /// # fn remove(&mut self, address: u8) { todo!() }
    /// # }
    /// # impl HostController for Driver {
    /// #     type InterruptPipe<'driver> = &'driver Foo;
    /// #     type MultiInterruptPipe = Foo; type DeviceDetect = Foo;
    /// # fn device_detect(&self) -> Self::DeviceDetect { todo!() }
    /// # fn control_transfer<'a>(&self,
    /// #   address: u8,
    /// #       packet_size: u8,
    /// #       setup: SetupPacket,
    /// #       data_phase: DataPhase<'a>,
    /// #   ) -> impl core::future::Future<Output = Result<usize, UsbError>> {
    /// #  async { todo!() } }
    /// # fn alloc_interrupt_pipe(
    /// # &self,
    /// #  address: u8,
    /// #    endpoint: u8,
    /// #   max_packet_size: u16,
    /// #    interval_ms: u8,
    /// # ) -> impl core::future::Future<Output = Self::InterruptPipe<'_>> {
    /// # async { todo!() } }
    /// #
    /// # fn multi_interrupt_pipe(&self) -> Self::MultiInterruptPipe { todo!() }
    /// # }
    /// # let driver = Driver;
    /// # pollster::block_on(async {
    /// let bus = UsbBus::new(driver);
    /// let mut device_stream = pin!(bus.device_events_no_hubs());
    /// loop {
    ///     let event = device_stream.next().await;
    ///     // ... process the event ...
    /// }
    /// # });
    /// ```
    ///
    /// When using this method, the cotton-usb-host crate deals only with
    /// a single USB device attached directly to the USB host controller,
    /// i.e. that device is not treated specially if it is a hub.
    ///
    /// If you would rather let the cotton-usb-host crate take care of
    /// hubs automatically, you can use
    /// [`device_events()`](`UsbBus::device_events_no_hubs()`) instead
    /// of `device_events_no_hubs()`.
    ///
    pub fn device_events_no_hubs(
        &self,
    ) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = self.driver.device_detect();
        root_device.then(move |status| async move {
            if let DeviceStatus::Present(speed) = status {
                match self.new_device(speed).await {
                    Ok(info) => match self.set_address(1, &info).await {
                        Ok(device) => DeviceEvent::Connect(device, info),
                        Err(e) => DeviceEvent::EnumerationError(0, 1, e),
                    },
                    Err(e) => DeviceEvent::EnumerationError(0, 1, e),
                }
            } else {
                DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF))
            }
        })
    }

    pub async fn configure(
        &self,
        device: &UsbDevice,
        info: &DeviceInfo,
        configuration_value: u8,
    ) -> Result<(), UsbError> {
        self.control_transfer(
            device.address,
            info.packet_size_ep0,
            SetupPacket {
                bmRequestType: HOST_TO_DEVICE,
                bRequest: SET_CONFIGURATION,
                wValue: configuration_value as u16,
                wIndex: 0,
                wLength: 0,
            },
            DataPhase::None,
        )
        .map(|r| r.map(|_| ()))
        .await
    }

    async fn new_device(
        &self,
        speed: UsbSpeed,
    ) -> Result<DeviceInfo, UsbError> {
        // Read prefix of device descriptor
        let mut descriptors = [0u8; 18];
        let sz = self
            .control_transfer(
                0,
                8,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((DEVICE_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 8,
                },
                DataPhase::In(&mut descriptors),
            )
            .await?;
        if sz < 8 {
            return Err(UsbError::ProtocolError);
        }

        let packet_size_ep0 = descriptors[7];

        // Fetch rest of device descriptor
        let sz = self
            .control_transfer(
                0,
                packet_size_ep0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((DEVICE_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 18,
                },
                DataPhase::In(&mut descriptors),
            )
            .await?;
        if sz < 18 {
            return Err(UsbError::ProtocolError);
        }

        let vid = u16::from_le_bytes([descriptors[8], descriptors[9]]);
        let pid = u16::from_le_bytes([descriptors[10], descriptors[11]]);

        Ok(DeviceInfo {
            packet_size_ep0,
            vid,
            pid,
            speed,
            class: descriptors[4],
            subclass: descriptors[5],
        })
    }

    async fn set_address(
        &self,
        address: u8,
        info: &DeviceInfo,
    ) -> Result<UsbDevice, UsbError> {
        self.control_transfer(
            0,
            info.packet_size_ep0,
            SetupPacket {
                bmRequestType: HOST_TO_DEVICE,
                bRequest: SET_ADDRESS,
                wValue: address as u16,
                wIndex: 0,
                wLength: 0,
            },
            DataPhase::None,
        )
        .await?;
        Ok(UsbDevice { address })
    }

    pub async fn control_transfer<'a>(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        data_phase: DataPhase<'a>,
    ) -> Result<usize, UsbError> {
        self.driver
            .control_transfer(address, packet_size, setup, data_phase)
            .await
    }

    pub fn interrupt_endpoint_in(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval: u8,
    ) -> impl Stream<Item = InterruptPacket> + '_ {
        let pipe = self.driver.alloc_interrupt_pipe(
            address,
            endpoint,
            max_packet_size,
            interval,
        );
        async move {
            let pipe = pipe.await;
            InterruptStream::<HC::InterruptPipe<'_>> { pipe }
        }
        .flatten_stream()
    }

    pub async fn get_basic_configuration(
        &self,
        device: &UsbDevice,
        info: &DeviceInfo,
    ) -> Result<BasicConfiguration, UsbError> {
        // TODO: descriptor suites >64 byte (Ella!)
        let mut buf = [0u8; 64];
        let sz = self
            .control_transfer(
                device.address,
                info.packet_size_ep0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((CONFIGURATION_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 64,
                },
                DataPhase::In(&mut buf),
            )
            .await?;
        let mut bd = BasicConfiguration::default();
        crate::types::parse_descriptors(&buf[0..sz], &mut bd);
        if bd.num_configurations == 0 || bd.configuration_value == 0 {
            Err(UsbError::ProtocolError)
        } else {
            Ok(bd)
        }
    }

    async fn new_hub(
        &self,
        device: &UsbDevice,
        info: &DeviceInfo,
    ) -> Result<(), UsbError> {
        debug::println!("gbc!");
        let bc = self.get_basic_configuration(device, info).await?;
        debug::println!("cfg: {:?}", &bc);
        self.configure(device, info, bc.configuration_value).await?;
        self.hub_pipes.borrow_mut().try_add(
            device.address,
            bc.in_endpoints.trailing_zeros() as u8,
            info.packet_size_ep0,
            9,
        )?;

        let mut descriptors = [0u8; 64];
        let sz = self
            .control_transfer(
                device.address,
                info.packet_size_ep0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST | CLASS_REQUEST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: (HUB_DESCRIPTOR as u16) << 8,
                    wIndex: 0,
                    wLength: 64,
                },
                DataPhase::In(&mut descriptors),
            )
            .await?;

        if sz < core::mem::size_of::<HubDescriptor>() {
            return Err(UsbError::ProtocolError);
        }

        let ports = descriptors[2];
        debug::println!("{}-port hub", ports);

        // Ports are numbered from 1..=N (not 0..N)
        for port in 1..=ports {
            self.set_port_feature(device.address, port, PORT_POWER)
                .await?;
        }

        Ok(())
    }

    /// Return a snapshot of the current physical bus layout
    ///
    /// This snapshot includes a representation of all the hubs and
    /// devices currently detected, and how they are linked together.
    ///
    /// This is useful for logging/debugging.
    pub fn topology(&self) -> Topology {
        self.hub_state.borrow().topology.clone()
    }

    async fn get_hub_port_status(
        &self,
        hub_address: u8,
        port: u8,
    ) -> Result<(u16, u16), UsbError> {
        let mut data = [0u8; 4];
        self.control_transfer(
            hub_address,
            8,
            SetupPacket {
                bmRequestType: DEVICE_TO_HOST
                    | CLASS_REQUEST
                    | RECIPIENT_OTHER,
                bRequest: GET_STATUS,
                wValue: 0,
                wIndex: port as u16,
                wLength: 4,
            },
            DataPhase::In(&mut data),
        )
        .await?;

        Ok((
            u16::from_le_bytes([data[0], data[1]]),
            u16::from_le_bytes([data[2], data[3]]),
        ))
    }

    /// Clear C_PORT_CONNECTION (or similar status-change bit); see
    /// USB 2.0 s11.24.2.7.2
    async fn clear_port_feature(
        &self,
        hub_address: u8,
        port: u8,
        feature: u16,
    ) -> Result<(), UsbError> {
        self.control_transfer(
            hub_address,
            8,
            SetupPacket {
                bmRequestType: HOST_TO_DEVICE
                    | CLASS_REQUEST
                    | RECIPIENT_OTHER,
                bRequest: CLEAR_FEATURE,
                wValue: feature,
                wIndex: port as u16,
                wLength: 0,
            },
            DataPhase::None,
        )
        .await?;
        Ok(())
    }

    async fn set_port_feature(
        &self,
        hub_address: u8,
        port: u8,
        feature: u16,
    ) -> Result<(), UsbError> {
        self.control_transfer(
            hub_address,
            8,
            SetupPacket {
                bmRequestType: HOST_TO_DEVICE
                    | CLASS_REQUEST
                    | RECIPIENT_OTHER,
                bRequest: SET_FEATURE,
                wValue: feature,
                wIndex: port as u16,
                wLength: 0,
            },
            DataPhase::None,
        )
        .await?;
        Ok(())
    }

    async fn handle_hub_packet(
        &self,
        packet: &InterruptPacket,
    ) -> Result<DeviceEvent, UsbError> {
        // Hub state machine: each hub must have each port powered,
        // then reset. But only one hub port on the whole *bus* can be
        // in reset at any one time, because it becomes sensitive to
        // address zero. So there needs to be a bus-wide hub state
        // machine.
        //
        // Idea: hubs get given addresses 1-15, so a bitmap of hubs
        // fits in u16. Newly found hubs get their interrupt EP added
        // to the waker, and all their ports powered. Ports that see a
        // C_PORT_CONNECTION get the parent hub added to a bitmap of
        // "hubs needing resetting". When no port is currently in
        // reset, a hub is removed from the bitmap, and a port counter
        // (1..=N) is set up; each port in turn has GetStatus called,
        // and on returning CONNECTION but not ENABLE, the port is
        // reset. (Not connected or already enabled means progress to
        // the next port, then the next hub.)
        //
        // On a reset complete, give the new device an address (1-15
        // if it, too, is a hub) and then again proceed to the next
        // port, then the next hub.

        debug::println!(
            "Hub int {} [{}; {}]",
            packet.address,
            packet.data[0],
            packet.size
        );

        if packet.size == 0 {
            return Err(UsbError::ProtocolError);
        }

        let mut port_bitmap = packet.data[0] as u32;
        if packet.size > 1 {
            port_bitmap |= (packet.data[1] as u32) << 8;
        }
        let port_bitmap = BitIterator::new(port_bitmap);
        for port in port_bitmap {
            debug::println!("I'm told to investigate port {}", port);

            let (state, changes) =
                self.get_hub_port_status(packet.address, port).await?;
            debug::println!(
                "  port {} status3 {:x} {:x}",
                port,
                state,
                changes
            );

            if changes != 0 {
                let bit = changes.trailing_zeros(); // i.e., least_set_bit

                if bit < 5 {
                    // "+16" to clear the change version C_xx rather than the
                    // feature itself, see USB 2.0 table 11-17
                    self.clear_port_feature(
                        packet.address,
                        port,
                        (bit + 16) as u16,
                    )
                    .await?;
                }
                if bit == 0 {
                    // C_PORT_CONNECTION
                    if (state & 1) == 0 {
                        // now disconnected
                        let mask = self
                            .hub_state
                            .borrow_mut()
                            .topology
                            .device_disconnect(packet.address, port);
                        return Ok(DeviceEvent::Disconnect(BitSet(mask)));
                    }

                    // now connected
                    if self.hub_state.borrow().currently_resetting.is_none() {
                        self.set_port_feature(
                            packet.address,
                            port,
                            PORT_RESET,
                        )
                        .await?;
                        self.hub_state.borrow_mut().currently_resetting =
                            Some((packet.address, port));

                        // wait for the reset to complete (we'll be called
                        // again with C_PORT_RESET)
                        return Ok(DeviceEvent::None);
                    } else {
                        // TODO: queue hub for future investigation
                    }
                }
                if bit == 4 {
                    // C_PORT_RESET -- "This bit is set when the port
                    // transitions [...] to the Enabled state"

                    // USB 2.0 table 11-21
                    let speed = match state & 0x600 {
                        0 => UsbSpeed::Full12,
                        0x400 => UsbSpeed::High480,
                        _ => UsbSpeed::Low1_5,
                    };

                    let info = self.new_device(speed).await?;
                    let is_hub = info.class == HUB_CLASSCODE;
                    let address = {
                        let mut state = self.hub_state.borrow_mut();
                        state.currently_resetting = None;
                        state
                            .topology
                            .device_connect(packet.address, port, is_hub)
                            .ok_or(UsbError::TooManyDevices)?
                    };
                    let device = self.set_address(address, &info).await?;
                    if is_hub {
                        debug::println!("It's a hub");
                        self.new_hub(&device, &info).await?;
                    }
                    return Ok(DeviceEvent::Connect(device, info));
                }
            }
        }
        // TODO: if we get here, does some other port need resetting?
        Ok(DeviceEvent::None)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::host_controller::tests::{
        MockDeviceDetect, MockHostController, MockInterruptPipe,
        MockMultiInterruptPipe,
    };
    use crate::types::{
        EndpointDescriptor, InterfaceDescriptor, ENDPOINT_DESCRIPTOR,
        INTERFACE_DESCRIPTOR,
    };
    use futures::{future, Future};
    use std::pin::{pin, Pin};
    use std::sync::Arc;
    use std::task::{Poll, Wake, Waker};
    extern crate alloc;

    struct NoOpWaker;

    impl Wake for NoOpWaker {
        fn wake(self: Arc<Self>) {}
    }

    #[test]
    fn test_wake_does_nothing() {
        let w = Arc::new(NoOpWaker);
        w.wake();
    }

    const ELLA: &[u8] = &[
        9, 2, 180, 1, 5, 1, 0, 128, 250, 9, 4, 0, 0, 4, 255, 0, 3, 0, 12, 95,
        1, 0, 10, 0, 4, 4, 1, 0, 4, 0, 7, 5, 2, 2, 0, 2, 0, 7, 5, 8, 2, 0, 2,
        0, 7, 5, 132, 2, 0, 2, 0, 7, 5, 133, 3, 8, 0, 8, 9, 4, 1, 0, 0, 254,
        1, 1, 0, 9, 33, 1, 200, 0, 0, 4, 1, 1, 16, 64, 8, 8, 11, 1, 1, 3, 69,
        108, 108, 97, 68, 111, 99, 107, 8, 11, 2, 3, 1, 0, 32, 5, 9, 4, 2, 0,
        1, 1, 1, 32, 5, 9, 36, 1, 0, 2, 11, 0, 1, 0, 12, 36, 3, 4, 2, 6, 0,
        14, 11, 4, 0, 0, 8, 36, 10, 10, 1, 7, 0, 0, 8, 36, 10, 11, 1, 7, 0, 0,
        9, 36, 11, 12, 2, 10, 11, 3, 0, 17, 36, 2, 13, 1, 1, 0, 10, 6, 63, 0,
        0, 0, 0, 0, 0, 4, 34, 36, 6, 14, 13, 0, 0, 0, 0, 15, 0, 0, 0, 15, 0,
        0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 15, 0, 0, 0, 0, 64, 36,
        9, 0, 0, 0, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 36, 9, 0, 0, 0,
        49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 31, 36, 9, 0, 0, 0, 16, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 5,
        131, 3, 6, 0, 8, 9, 4, 3, 0, 0, 1, 2, 32, 5, 9, 4, 3, 1, 1, 1, 2, 32,
        5, 16, 36, 1, 13, 0, 1, 1, 0, 0, 0, 6, 63, 0, 0, 0, 0, 6, 36, 2, 1, 2,
        16, 7, 5, 9, 13, 64, 2, 4, 8, 37, 1, 0, 0, 1, 0, 0, 9, 4, 4, 0, 0, 1,
        2, 32, 5,
    ];

    fn example_config_descriptor(buf: &mut [u8]) {
        let total_length = (core::mem::size_of::<ConfigurationDescriptor>()
            + core::mem::size_of::<InterfaceDescriptor>()
            + core::mem::size_of::<EndpointDescriptor>())
            as u16;

        let c = ConfigurationDescriptor {
            bLength: core::mem::size_of::<ConfigurationDescriptor>() as u8,
            bDescriptorType: CONFIGURATION_DESCRIPTOR,
            wTotalLength: total_length.to_le_bytes(),
            bNumInterfaces: 1,
            bConfigurationValue: 1,
            iConfiguration: 0,
            bmAttributes: 0,
            bMaxPower: 0,
        };

        buf[0..9].copy_from_slice(bytemuck::bytes_of(&c));

        let i = InterfaceDescriptor {
            bLength: core::mem::size_of::<InterfaceDescriptor>() as u8,
            bDescriptorType: INTERFACE_DESCRIPTOR,
            bInterfaceNumber: 1,
            bAlternateSetting: 0,
            bNumEndpoints: 1,
            bInterfaceClass: 0,
            bInterfaceSubClass: 0,
            bInterfaceProtocol: 0,
            iInterface: 0,
        };

        buf[9..18].copy_from_slice(bytemuck::bytes_of(&i));

        let e = EndpointDescriptor {
            bLength: core::mem::size_of::<EndpointDescriptor>() as u8,
            bDescriptorType: ENDPOINT_DESCRIPTOR,
            bEndpointAddress: 1,
            bmAttributes: 0,
            wMaxPacketSize: 64u16.to_le_bytes(),
            bInterval: 0,
        };

        buf[18..25].copy_from_slice(bytemuck::bytes_of(&e));
    }

    const EXAMPLE_DEVICE: UsbDevice = UsbDevice { address: 5 };
    const EXAMPLE_INFO: DeviceInfo = DeviceInfo {
        vid: 1,
        pid: 2,
        class: 3,
        subclass: 4,
        speed: UsbSpeed::Full12,
        packet_size_ep0: 8,
    };

    // Not sure why this isn't in the standard library
    fn unwrap_poll<T>(p: Poll<T>) -> Option<T> {
        match p {
            Poll::Ready(t) => Some(t),
            _ => None,
        }
    }

    #[test]
    fn unwrap_good_poll() {
        let p = Poll::Ready(1);
        assert!(unwrap_poll(p).is_some());
    }

    #[test]
    fn unwrap_bad_poll() {
        let p = Poll::<u32>::Pending;
        assert!(unwrap_poll(p).is_none());
    }

    #[test]
    fn basic_configuration() {
        let mut bc = BasicConfiguration::default();
        crate::types::parse_descriptors(ELLA, &mut bc);

        assert_eq!(bc.configuration_value, 1);
        assert_eq!(bc.num_configurations, 1);
        assert_eq!(bc.in_endpoints, 0b111000);
        assert_eq!(bc.out_endpoints, 0b1100000100);
    }

    #[test]
    fn new_bus() {
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());
        let _bus = UsbBus::new(hc);
    }

    fn is_set_configuration<const ADDR: u8, const N: u16>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == ADDR
            && *p == 8
            && s.bmRequestType == HOST_TO_DEVICE
            && s.bRequest == SET_CONFIGURATION
            && s.wValue == N
            && s.wIndex == 0
            && s.wLength == 0
            && d.is_none()
    }

    fn control_transfer_ok<const N: usize>(
        _: u8,
        _: u8,
        _: SetupPacket,
        _: DataPhase,
    ) -> Pin<Box<dyn Future<Output = Result<usize, UsbError>>>> {
        Box::pin(future::ready(Ok(N)))
    }

    // This is by some margin the most insane function signature I have yet
    // written in Rust -- but it does make its call sites neater!
    #[rustfmt::skip]
    fn control_transfer_ok_with<F: FnMut(&mut [u8]) -> usize>(
        mut f: F,
    ) -> impl FnMut(
        u8,
        u8,
        SetupPacket,
        DataPhase,
    ) -> Pin<Box<dyn Future<Output = Result<usize, UsbError>>>> {
        move |_, _, _, mut d| {
            let mut n = 0;
            d.in_with(|bytes| n = f(bytes));
            Box::pin(future::ready(Ok(n)))
        }
    }

    fn control_transfer_pending(
        _: u8,
        _: u8,
        _: SetupPacket,
        _: DataPhase,
    ) -> Pin<Box<dyn Future<Output = Result<usize, UsbError>>>> {
        Box::pin(future::pending())
    }

    fn control_transfer_timeout(
        _: u8,
        _: u8,
        _: SetupPacket,
        _: DataPhase,
    ) -> Pin<Box<dyn Future<Output = Result<usize, UsbError>>>> {
        Box::pin(future::ready(Err(UsbError::Timeout)))
    }

    #[test]
    fn configure() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 6>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.configure(&EXAMPLE_DEVICE, &EXAMPLE_INFO, 6));
        let rr = r.poll(&mut c);
        assert_eq!(rr, Poll::Ready(Ok(())));
    }

    #[test]
    fn configure_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 6>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut r = pin!(bus.configure(&EXAMPLE_DEVICE, &EXAMPLE_INFO, 6));
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Pending);
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Pending);
    }

    #[test]
    fn configure_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 6>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.configure(&EXAMPLE_DEVICE, &EXAMPLE_INFO, 6));
        let rr = r.poll(&mut c);
        assert_eq!(rr, Poll::Ready(Err(UsbError::Timeout)));
    }

    fn is_get_configuration_descriptor<const ADDR: u8>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == ADDR
            && *p == 8
            && s.bmRequestType == DEVICE_TO_HOST
            && s.bRequest == GET_DESCRIPTOR
            && s.wValue == 0x200
            && s.wIndex == 0
            && s.wLength > 0
            && d.is_in()
    }

    #[test]
    fn get_basic_configuration() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(example_config_descriptor);
                Box::pin(future::ready(Ok(25)))
            });

        let bus = UsbBus::new(hc);

        let r =
            pin!(bus.get_basic_configuration(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert!(rc.is_ok());
    }

    #[test]
    fn get_basic_configuration_bad_descriptors() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(control_transfer_ok::<25>);

        let bus = UsbBus::new(hc);

        let r =
            pin!(bus.get_basic_configuration(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        assert_eq!(rr, Poll::Ready(Err(UsbError::ProtocolError)));
    }

    #[test]
    fn get_basic_configuration_bad_configuration_value() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(control_transfer_ok_with(|bytes| {
                example_config_descriptor(bytes);
                bytes[5] = 0; // nobble bConfigurationValue
                25
            }));

        let bus = UsbBus::new(hc);

        let r =
            pin!(bus.get_basic_configuration(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        assert_eq!(rr, Poll::Ready(Err(UsbError::ProtocolError)));
    }

    #[test]
    fn get_basic_configuration_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut r =
            pin!(bus.get_basic_configuration(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.as_mut().poll(&mut c);
        assert!(rr.is_pending());
        let rr = r.as_mut().poll(&mut c);
        assert!(rr.is_pending());
    }

    #[test]
    fn get_basic_configuration_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let mut r =
            pin!(bus.get_basic_configuration(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Ready(Err(UsbError::Timeout)));
    }

    fn is_set_address<const N: u8>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == 0
            && *p == 8
            && s.bmRequestType == HOST_TO_DEVICE
            && s.bRequest == SET_ADDRESS
            && s.wValue == N as u16
            && s.wIndex == 0
            && s.wLength == 0
            && d.is_none()
    }

    #[test]
    fn set_address() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<5>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.set_address(5, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        assert!(rr == Poll::Ready(Ok(UsbDevice { address: 5 })));
    }

    #[test]
    fn set_address_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<5>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut r = pin!(bus.set_address(5, &EXAMPLE_INFO));
        let rr = r.as_mut().poll(&mut c);
        assert!(rr.is_pending());
        let rr = r.as_mut().poll(&mut c);
        assert!(rr.is_pending());
    }

    #[test]
    fn set_address_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<5>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.set_address(5, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        assert!(rr.is_ready());
        assert!(rr == Poll::Ready(Err(UsbError::Timeout)));
    }

    #[test]
    fn interrupt_endpoint_in() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());
        hc.inner
            .expect_alloc_interrupt_pipe()
            .withf(|a, e, m, i| *a == 5 && *e == 2 && *m == 8 && *i == 10)
            .returning(|_, _, _, _| {
                Box::pin(future::ready({
                    let mut ip = MockInterruptPipe::new();
                    ip.expect_set_waker().return_const(());
                    ip.expect_poll()
                        .returning(|| Some(InterruptPacket::default()));
                    ip
                }))
            });
        let bus = UsbBus::new(hc);

        let r = pin!(bus.interrupt_endpoint_in(5, 2, 8, 10));
        let rr = r.poll_next(&mut c);
        assert!(rr.is_ready());
    }

    #[test]
    fn interrupt_endpoint_in_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());
        hc.inner
            .expect_alloc_interrupt_pipe()
            .withf(|a, e, m, i| *a == 5 && *e == 2 && *m == 8 && *i == 10)
            .returning(|_, _, _, _| Box::pin(future::pending()));
        let bus = UsbBus::new(hc);

        let mut r = pin!(bus.interrupt_endpoint_in(5, 2, 8, 10));
        let rr = r.as_mut().poll_next(&mut c);
        assert!(rr.is_pending());
        let rr = r.as_mut().poll_next(&mut c);
        assert!(rr.is_pending());
    }

    fn is_get_device_descriptor<const N: u16>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == 0
            && *p == 8
            && s.bmRequestType == DEVICE_TO_HOST
            && s.bRequest == GET_DESCRIPTOR
            && s.wValue == 0x100
            && s.wIndex == 0
            && s.wLength == N
            && d.is_in()
    }

    fn device_descriptor_prefix(bytes: &mut [u8]) -> usize {
        bytes[0] = 18;
        bytes[1] = DEVICE_DESCRIPTOR;
        bytes[7] = 8;
        8
    }

    fn device_descriptor(bytes: &mut [u8]) -> usize {
        device_descriptor_prefix(bytes);
        bytes[8] = 0x34;
        bytes[9] = 0x12;
        bytes[10] = 0x78;
        bytes[11] = 0x56;
        18
    }

    #[test]
    fn new_device() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        // First call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_device(UsbSpeed::Full12));
        let rr = r.poll(&mut c);
        let di = unwrap_poll(rr).unwrap().unwrap();
        assert_eq!(di.vid, 0x1234);
        assert_eq!(di.pid, 0x5678);
    }

    #[test]
    fn new_device_first_call_errors() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        // First call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_timeout);

        // No second call!

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_device(UsbSpeed::Full12));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc.unwrap_err(), UsbError::Timeout);
    }

    #[test]
    fn new_device_first_call_short() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        // First call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok::<7>);

        // No second call!

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_device(UsbSpeed::Full12));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc.unwrap_err(), UsbError::ProtocolError);
    }

    #[test]
    fn new_device_second_call_errors() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        // First call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_device(UsbSpeed::Full12));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc.unwrap_err(), UsbError::Timeout);
    }

    #[test]
    fn new_device_second_call_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        // First call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut r = pin!(bus.new_device(UsbSpeed::Full12));
        let rr = r.as_mut().poll(&mut c);
        assert!(rr.is_pending());
        let rr = r.as_mut().poll(&mut c);
        assert!(rr.is_pending());
    }

    #[test]
    fn new_device_second_call_short() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(|| MockMultiInterruptPipe::new());

        // First call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok::<17>);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_device(UsbSpeed::Full12));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc.unwrap_err(), UsbError::ProtocolError);
    }

    fn is_get_hub_descriptor<const ADDR: u8>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == ADDR
            && *p == 8
            && s.bmRequestType == DEVICE_TO_HOST | CLASS_REQUEST
            && s.bRequest == GET_DESCRIPTOR
            && s.wValue == 0x2900
            && s.wIndex == 0
            && s.wLength >= 9
            && d.is_in()
    }

    fn hub_descriptor(bytes: &mut [u8]) -> usize {
        bytes[0] = 9;
        bytes[1] = HUB_DESCRIPTOR;
        bytes[2] = 2; // 2-port hub
        9
    }

    fn giant_hub_descriptor(bytes: &mut [u8]) -> usize {
        bytes[0] = 9;
        bytes[1] = HUB_DESCRIPTOR;
        bytes[2] = 15; // 15-port hub
        11 // NB bigger than normal
    }

    fn is_set_port_power<const ADDR: u8, const N: u8>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == ADDR
            && *p == 8
            && s.bmRequestType
                == HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER
            && s.bRequest == SET_FEATURE
            && s.wValue == PORT_POWER
            && s.wIndex == N.into()
            && s.wLength == 0
            && d.is_none()
    }

    #[test]
    fn new_hub() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_ok::<0>);

        // Get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<5>)
            .returning(control_transfer_ok_with(hub_descriptor));

        // Set port power
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<5, 1>)
            .returning(control_transfer_ok::<0>);
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<5, 2>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert!(rc.is_ok());
    }

    #[test]
    fn new_hub_giant() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_ok::<0>);

        // Get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<5>)
            .returning(control_transfer_ok_with(giant_hub_descriptor));

        // Set port power x15
        hc.inner
            .expect_control_transfer()
            .times(15)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert!(rc.is_ok());
    }

    #[test]
    fn new_hub_get_configuration_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc, Err(UsbError::Timeout));
    }

    #[test]
    fn new_hub_configure_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(||
            MockMultiInterruptPipe::new()
        );

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc, Err(UsbError::Timeout));
    }

    #[test]
    fn new_hub_configure_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(||
            MockMultiInterruptPipe::new()
        );

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Pending);
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Pending);
    }

    #[test]
    fn new_hub_try_add_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Err(UsbError::TooManyDevices));
            mip
        });

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc, Err(UsbError::TooManyDevices));
    }

    #[test]
    fn new_hub_get_descriptor_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_ok::<0>);

        // Get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<5>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc, Err(UsbError::Timeout));
    }

    #[test]
    fn new_hub_get_descriptor_short() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_ok::<0>);

        // Get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<5>)
            .returning(control_transfer_ok::<8>);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc, Err(UsbError::ProtocolError));
    }

    #[test]
    fn new_hub_get_descriptor_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_ok::<0>);

        // Get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<5>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Pending);
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Pending);
    }

    #[test]
    fn new_hub_set_port_power_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_ok::<0>);

        // Get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<5>)
            .returning(control_transfer_ok_with(hub_descriptor));

        // Set port power
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<5, 1>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.poll(&mut c);
        let rc = unwrap_poll(rr).unwrap();
        assert_eq!(rc, Err(UsbError::Timeout));
    }

    #[test]
    fn new_hub_set_port_power_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<5>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<5, 1>)
            .returning(control_transfer_ok::<0>);

        // Get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<5>)
            .returning(control_transfer_ok_with(hub_descriptor));

        // Set port power
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<5, 1>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut r = pin!(bus.new_hub(&EXAMPLE_DEVICE, &EXAMPLE_INFO));
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Pending);
        let rr = r.as_mut().poll(&mut c);
        assert_eq!(rr, Poll::Pending);
    }

    #[test]
    fn handle_hub_packet_empty() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.size = 1;
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Ok(DeviceEvent::None));
    }

    fn is_get_port_status<const N: u8>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == 5
            && *p == 8
            && s.bmRequestType
                == DEVICE_TO_HOST | CLASS_REQUEST | RECIPIENT_OTHER
            && s.bRequest == GET_STATUS
            && s.wValue == 0
            && s.wIndex == N as u16
            && s.wLength == 4
            && d.is_in()
    }

    fn port_status<const STATE: u16, const CHANGES: u16>(
        bytes: &mut [u8],
    ) -> usize {
        bytes[0..2].copy_from_slice(&STATE.to_le_bytes());
        bytes[2..4].copy_from_slice(&CHANGES.to_le_bytes());
        4
    }

    fn is_clear_port_feature<const PORT: u8, const FEATURE: u16>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == 5
            && *p == 8
            && s.bmRequestType
                == HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER
            && s.bRequest == 1
            && s.wValue == FEATURE
            && s.wIndex == PORT as u16
            && s.wLength == 0
            && d.is_none()
    }

    fn is_set_port_feature<const PORT: u8, const FEATURE: u16>(
        a: &u8,
        p: &u8,
        s: &SetupPacket,
        d: &DataPhase,
    ) -> bool {
        *a == 5
            && *p == 8
            && s.bmRequestType
                == HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER
            && s.bRequest == 3
            && s.wValue == FEATURE
            && s.wIndex == PORT as u16
            && s.wLength == 0
            && d.is_none()
    }

    #[test]
    fn handle_hub_packet_connection() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<1, 1>));

        // Clear C_PORT_CONNECTION
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 16>)
            .returning(control_transfer_ok::<0>);

        // Set PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_feature::<1, 4>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Ok(DeviceEvent::None));

        assert_eq!(bus.hub_state.borrow().currently_resetting, Some((5, 1)));
    }

    #[test]
    fn handle_hub_packet_no_changes() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<8>)
            .returning(control_transfer_ok_with(port_status::<0, 0>));

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 2;
        p.data[0] = 0;
        p.data[1] = 1; // bit 8 set => port 8 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Ok(DeviceEvent::None));
    }

    #[test]
    fn handle_hub_packet_crazy_changes() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<8>)
            .returning(control_transfer_ok_with(port_status::<0, 0x20>));

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 2;
        p.data[0] = 0;
        p.data[1] = 1; // bit 8 set => port 8 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Ok(DeviceEvent::None));
    }

    #[test]
    fn handle_hub_packet_connection_status_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Err(UsbError::Timeout));
    }

    #[test]
    fn handle_hub_packet_connection_status_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let mut fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn handle_hub_packet_connection_clear_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<1, 1>));

        // Clear C_PORT_CONNECTION
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 16>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Err(UsbError::Timeout));
    }

    #[test]
    fn handle_hub_packet_connection_clear_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<1, 1>));

        // Clear C_PORT_CONNECTION
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 16>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let mut fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn handle_hub_packet_connection_set_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<1, 1>));

        // Clear C_PORT_CONNECTION
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 16>)
            .returning(control_transfer_ok::<0>);

        // Set PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_feature::<1, 4>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Err(UsbError::Timeout));

        assert_eq!(bus.hub_state.borrow().currently_resetting, None);
    }

    #[test]
    fn handle_hub_packet_connection_set_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<1, 1>));

        // Clear C_PORT_CONNECTION
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 16>)
            .returning(control_transfer_ok::<0>);

        // Set PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_feature::<1, 4>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let mut fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn handle_hub_packet_connection_queued() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<1, 1>));

        // Clear C_PORT_CONNECTION
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 16>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        bus.hub_state.borrow_mut().currently_resetting = Some((4, 4));

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Ok(DeviceEvent::None));

        assert_eq!(bus.hub_state.borrow().currently_resetting, Some((4, 4)));
    }

    #[test]
    fn handle_hub_packet_disconnection() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0, 1>));

        // Clear C_PORT_CONNECTION
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 16>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        {
            // Set up topology so there's a device (31) on hub 5 port 1
            let mut b = bus.hub_state.borrow_mut();
            b.topology.device_connect(0, 1, true); // 1
            b.topology.device_connect(1, 1, true); // 2
            b.topology.device_connect(1, 2, true); // 3
            b.topology.device_connect(1, 3, true); // 4
            b.topology.device_connect(1, 4, true); // 5
            b.topology.device_connect(5, 1, false); // 31
        }

        assert_eq!(format!("{:?}", bus.topology()), "0:(1:(2 3 4 5:(31)))");

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Ok(DeviceEvent::Disconnect(BitSet(0x8000_0000))));
    }

    #[test]
    fn handle_hub_packet_enabled() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (31)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<31>)
            .returning(control_transfer_ok::<0>);

        // The new device is NOT a hub so we're now done

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Ok(DeviceEvent::Connect(
                UsbDevice { address: 31 },
                DeviceInfo {
                    vid: 0x1234,
                    pid: 0x5678,
                    class: 0,
                    subclass: 0,
                    speed: UsbSpeed::Full12,
                    packet_size_ep0: 8
                }
            ))
        );

        assert_eq!(bus.hub_state.borrow().currently_resetting, None);
    }

    // A bit unlikely as we only have FS hardware, but the protocol
    // allows for it
    #[test]
    fn handle_hub_packet_enabled_high_speed() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x411, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (31)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<31>)
            .returning(control_transfer_ok::<0>);

        // The new device is NOT a hub so we're now done

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Ok(DeviceEvent::Connect(
                UsbDevice { address: 31 },
                DeviceInfo {
                    vid: 0x1234,
                    pid: 0x5678,
                    class: 0,
                    subclass: 0,
                    speed: UsbSpeed::High480,
                    packet_size_ep0: 8
                }
            ))
        );

        assert_eq!(bus.hub_state.borrow().currently_resetting, None);
    }

    #[test]
    fn handle_hub_packet_enabled_low_speed() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x211, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (31)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<31>)
            .returning(control_transfer_ok::<0>);

        // The new device is NOT a hub so we're now done

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Ok(DeviceEvent::Connect(
                UsbDevice { address: 31 },
                DeviceInfo {
                    vid: 0x1234,
                    pid: 0x5678,
                    class: 0,
                    subclass: 0,
                    speed: UsbSpeed::Low1_5,
                    packet_size_ep0: 8
                }
            ))
        );

        assert_eq!(bus.hub_state.borrow().currently_resetting, None);
    }

    #[test]
    fn handle_hub_packet_enabled_port_reset_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Err(UsbError::Timeout));

        assert_eq!(bus.hub_state.borrow().currently_resetting, None);
    }

    #[test]
    fn handle_hub_packet_enabled_port_reset_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let mut fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());

        assert_eq!(bus.hub_state.borrow().currently_resetting, None);
    }

    #[test]
    fn handle_hub_packet_enabled_new_device_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Err(UsbError::Timeout));

        assert_eq!(bus.hub_state.borrow().currently_resetting, None);
    }

    #[test]
    fn handle_hub_packet_enabled_new_device_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let mut fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn handle_hub_packet_enabled_set_address_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (31)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<31>)
            .returning(control_transfer_timeout);

        // The new device is NOT a hub so we're now done

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Err(UsbError::Timeout));
    }

    #[test]
    fn handle_hub_packet_enabled_set_address_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (31)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<31>)
            .returning(control_transfer_pending);

        // The new device is NOT a hub so we're now done

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let mut fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
    }

    fn device_descriptor_prefix_hub(bytes: &mut [u8]) -> usize {
        bytes[0] = 18;
        bytes[1] = DEVICE_DESCRIPTOR;
        bytes[4] = HUB_CLASSCODE;
        bytes[7] = 8;
        8
    }

    fn device_descriptor_hub(bytes: &mut [u8]) -> usize {
        device_descriptor_prefix(bytes);
        bytes[8] = 0x34;
        bytes[9] = 0x12;
        bytes[10] = 0x78;
        bytes[11] = 0x56;
        18
    }

    #[test]
    fn handle_hub_packet_enabled_hub() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix_hub));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor_hub));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_ok::<0>);

        // new_hub(): get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<1>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // new_hub(): configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<1, 1>)
            .returning(control_transfer_ok::<0>);

        // new_hub(): get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<1>)
            .returning(control_transfer_ok_with(hub_descriptor));

        // new_hub(): set port power
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<1, 1>)
            .returning(control_transfer_ok::<0>);
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<1, 2>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Ok(DeviceEvent::Connect(
                UsbDevice { address: 1 },
                DeviceInfo {
                    vid: 0x1234,
                    pid: 0x5678,
                    class: 9,
                    subclass: 0,
                    speed: UsbSpeed::Full12,
                    packet_size_ep0: 8
                }
            ))
        );

        assert_eq!(bus.hub_state.borrow().currently_resetting, None);
    }

    #[test]
    fn handle_hub_packet_enabled_hub_new_hub_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(||
            MockMultiInterruptPipe::new()
        );

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix_hub));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor_hub));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_ok::<0>);

        // new_hub(): get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<1>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Err(UsbError::Timeout));
    }

    #[test]
    fn handle_hub_packet_enabled_hub_new_hub_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(||
            MockMultiInterruptPipe::new()
        );

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix_hub));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor_hub));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_ok::<0>);

        // new_hub(): get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<1>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let mut fut = pin!(bus.handle_hub_packet(&p));

        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
        let poll = fut.as_mut().poll(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn handle_hub_packet_enabled_too_many_devices() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(||
            MockMultiInterruptPipe::new()
        );

        // Get port status
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_ok_with(port_status::<0x11, 0x10>));

        // Clear C_PORT_RESET
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_clear_port_feature::<1, 20>)
            .returning(control_transfer_ok::<0>);

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix_hub));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor_hub));

        let bus = UsbBus::new(hc);

        {
            let mut state = bus.hub_state.borrow_mut();
            for i in 1..16 {
                state.topology.device_connect(0, i, true);
            }
        }

        let mut p = InterruptPacket::new();
        p.address = 5;
        p.size = 1;
        p.data[0] = 0b10; // bit 1 set => port 1 needs attention
        let fut = pin!(bus.handle_hub_packet(&p));
        let poll = fut.poll(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Err(UsbError::TooManyDevices));
    }

    #[test]
    fn device_events_nh() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events_no_hubs());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Some(DeviceEvent::Connect(
                UsbDevice { address: 1 },
                DeviceInfo {
                    vid: 0x1234,
                    pid: 0x5678,
                    class: 0,
                    subclass: 0,
                    speed: UsbSpeed::Full12,
                    packet_size_ep0: 8
                }
            ))
        );
    }

    #[test]
    fn device_events_nh_new_device_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events_no_hubs());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
        );
    }

    #[test]
    fn device_events_nh_new_device_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut stream = pin!(bus.device_events_no_hubs());

        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn device_events_nh_set_address_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events_no_hubs());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
        );
    }

    #[test]
    fn device_events_nh_set_address_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut stream = pin!(bus.device_events_no_hubs());

        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn device_events_nh_disconnect() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next()
                .returning(|_| Poll::Ready(Some(DeviceStatus::Absent)));
            mdd
        });

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events_no_hubs());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Some(DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF))));
    }

    #[test]
    fn device_events_root_connect() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (31)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<31>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Some(DeviceEvent::Connect(
                UsbDevice { address: 31 },
                DeviceInfo {
                    vid: 0x1234,
                    pid: 0x5678,
                    class: 0,
                    subclass: 0,
                    speed: UsbSpeed::Low1_5,
                    packet_size_ep0: 8
                }
            ))
        );
    }

    #[test]
    fn device_events_new_device_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
        );
    }

    #[test]
    fn device_events_new_device_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut stream = pin!(bus.device_events());

        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn device_events_set_address_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (31)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<31>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
        );
    }

    #[test]
    fn device_events_set_address_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Full12)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor));

        // Set address (31)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<31>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut stream = pin!(bus.device_events());

        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn device_events_root_disconnect() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner
            .expect_multi_interrupt_pipe()
            .returning(MockMultiInterruptPipe::new);
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next()
                .returning(|_| Poll::Ready(Some(DeviceStatus::Absent)));
            mdd
        });

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Some(DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF))));
    }

    #[test]
    fn device_events_root_connect_is_hub() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_try_add().returning(|_, _, _, _| Ok(()));
            mip
        });
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix_hub));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor_hub));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_ok::<0>);

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<1>)
            .returning(|_, _, _, mut d| {
                d.in_with(|bytes| {
                    example_config_descriptor(bytes);
                });
                Box::pin(future::ready(Ok(25)))
            });

        // Call to configure
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_configuration::<1, 1>)
            .returning(control_transfer_ok::<0>);

        // Get hub descriptor
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_hub_descriptor::<1>)
            .returning(control_transfer_ok_with(hub_descriptor));

        // Set port power
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<1, 1>)
            .returning(control_transfer_ok::<0>);
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_port_power::<1, 2>)
            .returning(control_transfer_ok::<0>);

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Some(DeviceEvent::Connect(
                UsbDevice { address: 1 },
                DeviceInfo {
                    vid: 0x1234,
                    pid: 0x5678,
                    class: 9,
                    subclass: 0,
                    speed: UsbSpeed::Low1_5,
                    packet_size_ep0: 8
                }
            ))
        );
    }

    #[test]
    fn device_events_root_connect_new_hub_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(||
            MockMultiInterruptPipe::new()
        );
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix_hub));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor_hub));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_ok::<0>);

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<1>)
            .returning(control_transfer_timeout);

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(
            result,
            Some(DeviceEvent::EnumerationError(0, 1, UsbError::Timeout))
        );
    }

    #[test]
    fn device_events_root_connect_new_hub_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(||
            MockMultiInterruptPipe::new()
        );
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Ready(Some(DeviceStatus::Present(UsbSpeed::Low1_5)))
            });
            mdd
        });

        // new_device(): first call (wLength == 8)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<8>)
            .returning(control_transfer_ok_with(device_descriptor_prefix_hub));

        // new_device(): Second call (wLength == 18)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_device_descriptor::<18>)
            .returning(control_transfer_ok_with(device_descriptor_hub));

        // Set address (1)
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_set_address::<1>)
            .returning(control_transfer_ok::<0>);

        // Call to get_basic_configuration
        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_configuration_descriptor::<1>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut stream = pin!(bus.device_events());

        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
    }

    #[test]
    fn device_events_hub_packet() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_set_waker().return_const(());
            mip.expect_poll()
                .returning(|| {
                    let mut ip = InterruptPacket::new();
                    ip.size = 1;
                    Some(ip)
                });
            mip
        });
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Pending
            });
            mdd
        });

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result, Some(DeviceEvent::None));
    }

    #[test]
    fn device_events_hub_packet_fails() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_set_waker().return_const(());
            mip.expect_poll()
                .returning(|| {
                    Some(InterruptPacket::new()) // a 0-length packet
                });
            mip
        });
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Pending
            });
            mdd
        });

        let bus = UsbBus::new(hc);

        let stream = pin!(bus.device_events());

        let poll = stream.poll_next(&mut c);
        let result = unwrap_poll(poll).unwrap();
        assert_eq!(result,Some(DeviceEvent::EnumerationError(0, 1, UsbError::ProtocolError)));
    }

    #[test]
    fn device_events_hub_packet_pends() {
        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);
        let mut hc = MockHostController::default();
        hc.inner.expect_multi_interrupt_pipe().returning(|| {
            let mut mip = MockMultiInterruptPipe::new();
            mip.expect_set_waker().return_const(());
            mip.expect_poll()
                .returning(|| {
                    let mut ip = InterruptPacket::new();
                    ip.size = 1;
                    ip.address = 5;
                    ip.data[0] = 2;
                    Some(ip)
                });
            mip
        });
        hc.inner.expect_device_detect().returning(|| {
            let mut mdd = MockDeviceDetect::new();
            mdd.expect_poll_next().returning(|_| {
                Poll::Pending
            });
            mdd
        });

        hc.inner
            .expect_control_transfer()
            .times(1)
            .withf(is_get_port_status::<1>)
            .returning(control_transfer_pending);

        let bus = UsbBus::new(hc);

        let mut stream = pin!(bus.device_events());

        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
        let poll = stream.as_mut().poll_next(&mut c);
        assert!(poll.is_pending());
    }
}
