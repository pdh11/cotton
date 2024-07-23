//! Implementing SSDP, the Simple Service Discovery Protocol
//!
//! The cotton-ssdp crate encapsulates a client and server for the
//! Simple Service Discovery Protocol (SSDP), a mechanism for
//! discovering available _resources_ (services) on local networks. A
//! _resource_ might be a streaming-media server, or a router, or a
//! network printer, or anything else that someone might want to
//! search for or enumerate on a network.
//!
//! What is advertised, or discovered, is, for each resource, a unique
//! identifier for that particular resource (Unique Service Name,
//! USN), an identifier for the _type_ of resource (Notification Type,
//! NT), and the _location_ of the resource in the form of a URL.
//!
//! SSDP is mainly used by UPnP (Universal Plug-'n'-Play) systems,
//! such as for media libraries and local streaming of music and video
//! -- but the mechanism is quite generic, and could as easily be used
//! for any type of device or resource that must be discoverable over
//! a network, including in *ad hoc* settings which don't necessarily
//! have expert network administrators close at hand.
//!
//! There is no Internet RFC as such for SSDP -- merely some expired
//! drafts.  The protocol is, instead, documented in the [UPnP Device
//! Architecture](https://openconnectivity.org/developer/specifications/upnp-resources/upnp/archive-of-previously-published-upnp-device-architectures/)
//! documents.
//!
//! This crate provides two different high-level interfaces for
//! working with SSDP, [`Service`] and [`AsyncService`]. Either one can
//! be used both to discover other devices ([`Service::subscribe`])
//! and to advertise resources itself ([`Service::advertise`]).
//!
//! Client code using the MIO crate should use plain [`Service`];
//! client code using the Tokio crate might wish to use
//! [`AsyncService`] instead, which integrates with that
//! system. Client code with a _custom_ polling loop -- neither MIO
//! nor Tokio -- should instead probably aim to build an equivalent to
//! [`Service`] using the lower-level facilities in
//! [`engine::Engine`].
//!
//! Example code is available both for asynchronous Tokio use:
//! [ssdp-search](https://github.com/pdh11/cotton/blob/main/cotton-ssdp/examples/ssdp-search.rs)
//! (on Github) and reactor-style MIO use:
//! [ssdp-search-mio](https://github.com/pdh11/cotton/blob/main/cotton-ssdp/examples/ssdp-search-mio.rs)
//! (on Github).
//!
//! Todo:
//!  - [x] Make mio/tokio features
//!  - [ ] Make advertise/subscribe features
//!  - [ ] `Cow<'static>` for input strings?
//!  - [ ] Hasher instead of `thread_rng`; hash over network interfaces sb unique
//!  - [ ] Vary phase 1,2,3 timings but keep phase 0 timings on round numbers (needs _absolute_ wall time)
//!  - [x] Monotonic time instead of `Instant::now` (lifetime?) *Solved differently*
//!  - [x] `smoltcp`/`no_std`, see <https://github.com/rust-lang/rust/pull/104265>
//!  - [ ] IPv6, see UPnP DA appendix A
//!

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(docsrs, feature(doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

// NB "docsrs" here really means "nightly", but that isn't an available cfg

extern crate alloc;

#[cfg(feature = "async")]
mod async_service;

/// Low-level SSDP API used inside [`Service`] and [`AsyncService`]
pub mod engine;

/// Inbound and outbound SSDP events, high-level
pub mod event;

mod message;

#[cfg(feature = "sync")]
mod service;

/// Traits used to abstract over various UDP socket implementations
pub mod udp;

/// Common code for triggering refreshes of [`Service`] and [`AsyncService`]
pub mod refresh_timer;

#[cfg(feature = "async")]
pub use async_service::AsyncService;

#[cfg(feature = "sync")]
pub use service::Service;

pub use event::Advertisement;
pub use event::Notification;
