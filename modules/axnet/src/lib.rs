//! [ArceOS](https://github.com/rcore-os/arceos) network module.
//!
//! It provides unified networking primitives for TCP/UDP communication
//! using various underlying network stacks. Currently, only [smoltcp] is
//! supported.
//!
//! # Organization
//!
//! - [`TcpSocket`]: A TCP socket that provides POSIX-like APIs.
//! - [`UdpSocket`]: A UDP socket that provides POSIX-like APIs.
//! - [`dns_query`]: Function for DNS query.
//!
//! [smoltcp]: https://github.com/smoltcp-rs/smoltcp

#![no_std]
#![feature(ip_from)]

#[macro_use]
extern crate log;
extern crate alloc;

mod consts;
mod device;
mod general;
mod listen_table;
pub mod options;
mod router;
mod service;
mod socket;
mod tcp;
mod udp;
mod wrapper;

use alloc::{borrow::ToOwned, boxed::Box};

use axdriver::{AxDeviceContainer, prelude::*};
use axsync::Mutex;
use lazyinit::LazyInit;
use smoltcp::wire::{EthernetAddress, Ipv4Address, Ipv4Cidr};
pub use socket::*;
pub use tcp::*;
pub use udp::*;

use crate::{
    consts::{GATEWAY, IP, IP_PREFIX},
    device::{EthernetDevice, LoopbackDevice},
    listen_table::ListenTable,
    router::{Router, Rule},
    service::Service,
    wrapper::SocketSetWrapper,
};

#[doc(hidden)]
pub mod __priv {
    pub use axerrno::LinuxError;
}

static LISTEN_TABLE: LazyInit<ListenTable> = LazyInit::new();
static SOCKET_SET: LazyInit<SocketSetWrapper> = LazyInit::new();

static SERVICE: LazyInit<Mutex<Service>> = LazyInit::new();

/// Initializes the network subsystem by NIC devices.
pub fn init_network(mut net_devs: AxDeviceContainer<AxNetDevice>) {
    info!("Initialize network subsystem...");

    let dev = net_devs.take_one().expect("No NIC device found!");
    info!("  use NIC 0: {:?}", dev.device_name());

    let eth0_address = EthernetAddress(dev.mac_address().0);

    let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
    let eth0_ip = Ipv4Cidr::new(IP.parse().expect("Invalid IPv4 address"), IP_PREFIX);

    let mut router = Router::new();
    let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
    let eth0_dev = router.add_device(Box::new(EthernetDevice::new(
        "eth0".to_owned(),
        dev,
        eth0_ip,
    )));

    router.add_rule(Rule::new(
        lo_ip.into(),
        None,
        lo_dev,
        lo_ip.address().into(),
    ));
    router.add_rule(Rule::new(
        Ipv4Cidr::new(Ipv4Address::UNSPECIFIED, 0).into(),
        Some(GATEWAY.parse().expect("Invalid gateway address")),
        eth0_dev,
        eth0_ip.address().into(),
    ));

    let mut service = Service::new(router);
    service.iface.update_ip_addrs(|ip_addrs| {
        ip_addrs.push(lo_ip.into()).unwrap();
        ip_addrs.push(eth0_ip.into()).unwrap();
    });
    SERVICE.init_once(Mutex::new(service));

    info!("eth0:");
    info!("  mac:  {}", eth0_address);
    info!("  ip:   {}", eth0_ip);

    SOCKET_SET.init_once(SocketSetWrapper::new());
    LISTEN_TABLE.init_once(ListenTable::new());
}

pub fn poll_interfaces() {
    SERVICE.lock().poll(&mut SOCKET_SET.0.lock());
}
