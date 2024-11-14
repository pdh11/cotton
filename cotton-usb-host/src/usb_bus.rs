use crate::bitset::{BitIterator, BitSet};
use crate::debug;
use crate::interrupt::InterruptStream;
use crate::topology::Topology;
use crate::wire::{
    ConfigurationDescriptor, DescriptorVisitor, EndpointDescriptor,
    HubDescriptor, SetupPacket, CLASS_REQUEST, CLEAR_FEATURE,
    CONFIGURATION_DESCRIPTOR, DEVICE_DESCRIPTOR, DEVICE_TO_HOST,
    GET_DESCRIPTOR, GET_STATUS, HOST_TO_DEVICE, HUB_CLASSCODE, HUB_DESCRIPTOR,
    PORT_POWER, PORT_RESET, RECIPIENT_OTHER, SET_ADDRESS, SET_CONFIGURATION,
    SET_FEATURE,
};
use core::cell::RefCell;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::future::FutureExt;
use futures::{Future, Stream, StreamExt};

pub use crate::host_controller::{
    DataPhase, DeviceStatus, HostController, InterruptPacket, InterruptPipe,
    UsbError, UsbSpeed,
};

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub class: u8,
    pub subclass: u8,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
struct UnaddressedDevice {
    usb_speed: UsbSpeed,
    packet_size_ep0: u8,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(PartialEq, Eq)]
pub struct UnconfiguredDevice {
    usb_address: u8,
    usb_speed: UsbSpeed,
    packet_size_ep0: u8,
}

impl UnconfiguredDevice {
    pub fn address(&self) -> u8 {
        self.usb_address
    }
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(PartialEq, Eq)]
pub struct UsbDevice {
    usb_address: u8,
    usb_speed: UsbSpeed,
    packet_size_ep0: u8,
    in_endpoints_bitmap: u16,
    out_endpoints_bitmap: u16,
}

impl UsbDevice {
    pub fn address(&self) -> u8 {
        self.usb_address
    }

    pub fn in_endpoints(&self) -> BitSet {
        BitSet(self.in_endpoints_bitmap as u32)
    }

    pub fn out_endpoints(&self) -> BitSet {
        BitSet(self.out_endpoints_bitmap as u32)
    }
}

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
    Connect(UnconfiguredDevice, DeviceInfo),

    /// A new hub has been connected and configured (when using
    /// [`UsbBus::device_events()`] and not
    /// [`UsbBus::device_events_no_hubs()`]).
    ///
    /// This event can be ignored unless you want to take special
    /// actions e.g. powering-down particular ports. Normal
    /// powering-up and enumerating of hub ports is done by this crate
    /// in the [`UsbBus::device_events`] call.
    HubConnect(UsbDevice),

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

struct SpecificConfiguration {
    configuration_value: u8,
    ok: bool,
    in_endpoints: u16,
    out_endpoints: u16,
}

impl SpecificConfiguration {
    const fn new(configuration_value: u8) -> Self {
        Self {
            configuration_value,
            ok: false,
            in_endpoints: 0,
            out_endpoints: 0,
        }
    }
}

impl DescriptorVisitor for SpecificConfiguration {
    fn on_configuration(&mut self, c: &ConfigurationDescriptor) {
        self.ok = c.bConfigurationValue == self.configuration_value;
    }
    fn on_endpoint(&mut self, i: &EndpointDescriptor) {
        if self.ok {
            if (i.bEndpointAddress & 0x80) == 0x80 {
                self.in_endpoints |= 1 << (i.bEndpointAddress & 15);
            } else {
                self.out_endpoints |= 1 << (i.bEndpointAddress & 15);
            }
        }
    }
}

pub struct HubState<HC: HostController> {
    topology: RefCell<Topology>,
    pipes: RefCell<[Option<HC::InterruptPipe>; 15]>,
}

impl<HC: HostController> Default for HubState<HC> {
    fn default() -> Self {
        Self {
            topology: Default::default(),
            pipes: Default::default(),
        }
    }
}

impl<HC: HostController> HubState<HC> {
    /// Return a snapshot of the current physical bus layout
    ///
    /// This snapshot includes a representation of all the hubs and
    /// devices currently detected, and how they are linked together.
    ///
    /// This is useful for logging/debugging.
    pub fn topology(&self) -> Topology {
        self.topology.borrow().clone()
    }

    fn try_add(
        &self,
        hc: &HC,
        address: u8,
        endpoint: u8,
        max_packet_size: u8,
        interval_ms: u8,
    ) -> Result<(), UsbError> {
        for p in self.pipes.borrow_mut().iter_mut() {
            if p.is_none() {
                *p = Some(hc.try_alloc_interrupt_pipe(
                    address,
                    endpoint,
                    max_packet_size as u16,
                    interval_ms,
                )?);
                return Ok(());
            }
        }
        Err(UsbError::TooManyDevices)
    }
}

struct HubStateStream<'a, HC: HostController> {
    state: &'a HubState<HC>,
}

impl<HC: HostController> Stream for HubStateStream<'_, HC> {
    type Item = InterruptPacket;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Option<Self::Item>> {
        for pipe in self.state.pipes.borrow().iter().flatten() {
            pipe.set_waker(cx.waker());
        }

        for pipe in self.state.pipes.borrow().iter().flatten() {
            if let Some(packet) = pipe.poll() {
                return Poll::Ready(Some(packet));
            }
        }
        Poll::Pending
    }
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
}

impl<HC: HostController> UsbBus<HC> {
    /// Create a new USB host bus from a host-controller driver
    pub fn new(driver: HC) -> Self {
        Self { driver }
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
    /// # use cotton_usb_host::usb_bus::{HubState, UsbBus};
    /// # use futures::{future, Future, Stream, StreamExt};
    /// # fn delay_ms(_ms: usize) -> impl Future<Output = ()> {
    /// #  future::ready(())
    /// # }
    /// # async fn foo<D: HostController>(driver: D) -> () {
    /// let hub_state = HubState::default();
    /// let bus = UsbBus::new(driver);
    /// let mut device_stream = pin!(bus.device_events(&hub_state, delay_ms));
    /// loop {
    ///     let event = device_stream.next().await;
    ///     // ... process the event ...
    /// }
    /// # }
    /// ```
    ///
    /// You need to supply an implementation of the "delay" function which,
    /// given a parameter in milliseconds, returns a Future that waits for
    /// that long before coming ready. See the examples for how to implement
    /// that (simple!) function for RTIC2 and for Embassy; other executors
    /// will require their own implementations.
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
    pub fn device_events<
        'a,
        D: Future<Output = ()>,
        F: Fn(usize) -> D + 'static + Clone,
    >(
        &'a self,
        hub_state: &'a HubState<HC>,
        delay_ms_in: F,
    ) -> impl Stream<Item = DeviceEvent> + 'a {
        let root_device = self.driver.device_detect();

        enum InternalEvent {
            Root(DeviceStatus),
            Packet(InterruptPacket),
        }

        futures::stream::select(
            root_device.map(InternalEvent::Root),
            HubStateStream { state: hub_state }
                /*
                MultiInterruptStream::<HC::MultiInterruptPipe> {
                    pipe: &hub_state.pipes,
                } */
                .map(InternalEvent::Packet),
        )
        .then(move |ev| {
            let delay_ms = delay_ms_in.clone();
            async move {
                match ev {
                    InternalEvent::Root(status) => {
                        if let DeviceStatus::Present(speed) = status {
                            self.driver.reset_root_port(true);
                            delay_ms(50).await;
                            self.driver.reset_root_port(false);
                            delay_ms(10).await;
                            let (device, info) =
                                match self.new_device(speed).await {
                                    Ok((device, info)) => (device, info),
                                    Err(e) => {
                                        return DeviceEvent::EnumerationError(
                                            0, 1, e,
                                        )
                                    }
                                };
                            let is_hub = info.class == HUB_CLASSCODE;
                            let address = hub_state
                                .topology
                                .borrow_mut()
                                .device_connect(0, 1, is_hub)
                                .expect("Root connect should always succeed");
                            let device = match self
                                .set_address(device, address)
                                .await
                            {
                                Ok(device) => device,
                                Err(e) => {
                                    return DeviceEvent::EnumerationError(
                                        0, 1, e,
                                    );
                                }
                            };
                            if is_hub {
                                debug::println!("It's a hub");
                                match self.new_hub(hub_state, device).await {
                                    Ok(device) => {
                                        return DeviceEvent::HubConnect(device)
                                    }
                                    Err(e) => {
                                        return DeviceEvent::EnumerationError(
                                            0, 1, e,
                                        )
                                    }
                                };
                            }
                            DeviceEvent::Connect(device, info)
                        } else {
                            hub_state
                                .topology
                                .borrow_mut()
                                .device_disconnect(0, 1);
                            DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF))
                        }
                    }
                    InternalEvent::Packet(packet) => self
                        .handle_hub_packet(hub_state, &packet, delay_ms)
                        .await
                        .unwrap_or_else(|e| {
                            DeviceEvent::EnumerationError(0, 1, e)
                        }),
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
    /// # use cotton_usb_host::usb_bus::UsbBus;
    /// # use futures::{future, Future, Stream, StreamExt};
    /// # fn delay_ms(_ms: usize) -> impl Future<Output = ()> {
    /// #  future::ready(())
    /// # }
    /// # async fn foo<D: HostController>(driver: D) -> () {
    /// let bus = UsbBus::new(driver);
    /// let mut device_stream = pin!(bus.device_events_no_hubs(delay_ms));
    /// loop {
    ///     let event = device_stream.next().await;
    ///     // ... process the event ...
    /// }
    /// # }
    /// ```
    ///
    /// You need to supply an implementation of the "delay" function which,
    /// given a parameter in milliseconds, returns a Future that waits for
    /// that long before coming ready. See the examples for how to implement
    /// that (simple!) function for RTIC2 and for Embassy; other executors
    /// will require their own implementations.
    ///
    /// When using this method, the cotton-usb-host crate deals only with
    /// a single USB device attached directly to the USB host controller,
    /// i.e. that device is not treated specially if it is a hub.
    ///
    /// If you would rather let the cotton-usb-host crate take care of
    /// hubs automatically, you can use
    /// [`device_events()`](`UsbBus::device_events()`) instead
    /// of `device_events_no_hubs()`.
    ///
    pub fn device_events_no_hubs<
        D: Future<Output = ()>,
        F: Fn(usize) -> D + 'static + Clone,
    >(
        &self,
        delay_ms_in: F,
    ) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = self.driver.device_detect();
        root_device.then(move |status| {
            let delay_ms = delay_ms_in.clone();
            async move {
                if let DeviceStatus::Present(speed) = status {
                    self.driver.reset_root_port(true);
                    delay_ms(50).await;
                    self.driver.reset_root_port(false);
                    delay_ms(10).await;
                    match self.new_device(speed).await {
                        Ok((device, info)) => match self
                            .set_address(device, 1)
                            .await
                        {
                            Ok(device) => DeviceEvent::Connect(device, info),
                            Err(e) => DeviceEvent::EnumerationError(0, 1, e),
                        },
                        Err(e) => DeviceEvent::EnumerationError(0, 1, e),
                    }
                } else {
                    DeviceEvent::Disconnect(BitSet(0xFFFF_FFFF))
                }
            }
        })
    }

    pub async fn configure(
        &self,
        device: UnconfiguredDevice,
        configuration_value: u8,
    ) -> Result<UsbDevice, UsbError> {
        self.driver
            .control_transfer(
                device.address(),
                device.packet_size_ep0,
                SetupPacket {
                    bmRequestType: HOST_TO_DEVICE,
                    bRequest: SET_CONFIGURATION,
                    wValue: configuration_value as u16,
                    wIndex: 0,
                    wLength: 0,
                },
                DataPhase::None,
            )
            .await?;
        let mut endpoints = SpecificConfiguration::new(configuration_value);
        self.get_configuration(&device, &mut endpoints).await?;
        Ok(UsbDevice {
            usb_address: device.usb_address,
            usb_speed: device.usb_speed,
            packet_size_ep0: device.packet_size_ep0,
            in_endpoints_bitmap: endpoints.in_endpoints,
            out_endpoints_bitmap: endpoints.out_endpoints,
        })
    }

    async fn new_device(
        &self,
        speed: UsbSpeed,
    ) -> Result<(UnaddressedDevice, DeviceInfo), UsbError> {
        // Read prefix of device descriptor
        let mut descriptors = [0u8; 18];
        let sz = self
            .driver
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
            .driver
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

        Ok((
            UnaddressedDevice {
                usb_speed: speed,
                packet_size_ep0,
            },
            DeviceInfo {
                vid,
                pid,
                class: descriptors[4],
                subclass: descriptors[5],
            },
        ))
    }

    async fn set_address(
        &self,
        device: UnaddressedDevice,
        address: u8,
    ) -> Result<UnconfiguredDevice, UsbError> {
        self.driver
            .control_transfer(
                0,
                device.packet_size_ep0,
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
        Ok(UnconfiguredDevice {
            usb_address: address,
            usb_speed: device.usb_speed,
            packet_size_ep0: device.packet_size_ep0,
        })
    }

    pub async fn control_transfer(
        &self,
        device: &UsbDevice,
        setup: SetupPacket,
        data_phase: DataPhase<'_>,
    ) -> Result<usize, UsbError> {
        self.driver
            .control_transfer(
                device.usb_address,
                device.packet_size_ep0,
                setup,
                data_phase,
            )
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
            InterruptStream::<HC::InterruptPipe> { pipe }
        }
        .flatten_stream()
    }

    async fn get_configuration(
        &self,
        device: &UnconfiguredDevice,
        visitor: &mut impl DescriptorVisitor,
    ) -> Result<(), UsbError> {
        // TODO: descriptor suites >64 byte (Ella!)
        let mut buf = [0u8; 64];
        let sz = self
            .driver
            .control_transfer(
                device.address(),
                device.packet_size_ep0,
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
        crate::wire::parse_descriptors(&buf[0..sz], visitor);
        Ok(())
    }

    pub async fn get_basic_configuration(
        &self,
        device: &UnconfiguredDevice,
    ) -> Result<BasicConfiguration, UsbError> {
        let mut bd = BasicConfiguration::default();
        self.get_configuration(device, &mut bd).await?;
        if bd.num_configurations == 0 || bd.configuration_value == 0 {
            Err(UsbError::ProtocolError)
        } else {
            Ok(bd)
        }
    }

    async fn new_hub(
        &self,
        hub_state: &HubState<HC>,
        device: UnconfiguredDevice,
    ) -> Result<UsbDevice, UsbError> {
        debug::println!("gbc!");
        let bc = self.get_basic_configuration(&device).await?;
        debug::println!("cfg: {:?}", &bc);
        let device = self.configure(device, bc.configuration_value).await?;
        hub_state.try_add(
            &self.driver,
            device.address(),
            bc.in_endpoints.trailing_zeros() as u8,
            device.packet_size_ep0,
            9,
        )?;

        let mut descriptors = [0u8; 64];
        let sz = self
            .driver
            .control_transfer(
                device.address(),
                device.packet_size_ep0,
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
            self.set_port_feature(device.address(), port, PORT_POWER)
                .await?;
        }

        Ok(device)
    }

    async fn get_hub_port_status(
        &self,
        hub_address: u8,
        port: u8,
    ) -> Result<(u16, u16), UsbError> {
        let mut data = [0u8; 4];
        self.driver
            .control_transfer(
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
        self.driver
            .control_transfer(
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
        self.driver
            .control_transfer(
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

    async fn handle_hub_packet<
        D: Future<Output = ()>,
        F: Fn(usize) -> D + 'static + Clone,
    >(
        &self,
        hub_state: &HubState<HC>,
        packet: &InterruptPacket,
        delay_ms: F,
    ) -> Result<DeviceEvent, UsbError> {
        // Hub state machine: each hub must have each port powered,
        // then reset. But only one hub port on the whole *bus* can be
        // in reset at any one time, because it becomes sensitive to
        // address zero. So there needs to be a bus-wide hub state
        // machine.

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
                        let mask = hub_state
                            .topology
                            .borrow_mut()
                            .device_disconnect(packet.address, port);

                        return Ok(DeviceEvent::Disconnect(BitSet(mask)));
                    }

                    // now connected
                    self.set_port_feature(packet.address, port, PORT_RESET)
                        .await?;

                    delay_ms(50).await;

                    let (state, _changes) =
                        self.get_hub_port_status(packet.address, port).await?;

                    if (state & 2) != 0 {
                        // port is now ENABLED i.e. operational

                        // USB 2.0 table 11-21
                        let speed = match state & 0x600 {
                            0 => UsbSpeed::Full12,
                            0x400 => UsbSpeed::High480,
                            _ => UsbSpeed::Low1_5,
                        };

                        let (device, info) = self.new_device(speed).await?;
                        let is_hub = info.class == HUB_CLASSCODE;
                        let address = hub_state
                            .topology
                            .borrow_mut()
                            .device_connect(packet.address, port, is_hub)
                            .ok_or(UsbError::TooManyDevices)?;
                        let device = self.set_address(device, address).await?;
                        if is_hub {
                            debug::println!("It's a hub");
                            return Ok(DeviceEvent::HubConnect(
                                self.new_hub(hub_state, device).await?,
                            ));
                        }

                        return Ok(DeviceEvent::Connect(device, info));
                    }
                }
            }
        }
        Ok(DeviceEvent::None)
    }
}

#[cfg(all(test, feature = "std"))]
#[path = "tests/usb_bus.rs"]
mod tests;
