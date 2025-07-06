use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};

pub const fn from_core_ipaddr(ip: IpAddr) -> IpAddress {
    match ip {
        IpAddr::V4(ipv4) => IpAddress::Ipv4(Ipv4Address(ipv4.octets())),
        IpAddr::V6(ipv6) => IpAddress::Ipv6(Ipv6Address(ipv6.octets())),
    }
}

pub const fn into_core_ipaddr(ip: IpAddress) -> IpAddr {
    match ip {
        IpAddress::Ipv4(ipv4) => IpAddr::V4(Ipv4Addr::from_octets(ipv4.0)),
        IpAddress::Ipv6(ipv6) => IpAddr::V6(Ipv6Addr::from_octets(ipv6.0)),
    }
}

/// Convert from `std::net::SocketAddr` to `smoltcp::wire::IpEndpoint`.
pub const fn from_core_sockaddr(addr: SocketAddr) -> IpEndpoint {
    IpEndpoint {
        addr: from_core_ipaddr(addr.ip()),
        port: addr.port(),
    }
}

/// Convert from `smoltcp::wire::IpEndpoint` to `std::net::SocketAddr`.
pub const fn into_core_sockaddr(addr: IpEndpoint) -> SocketAddr {
    SocketAddr::new(into_core_ipaddr(addr.addr), addr.port)
}

pub const UNSPECIFIED_IP_V4: IpAddress = IpAddress::v4(0, 0, 0, 0);
pub const UNSPECIFIED_ENDPOINT_V4: IpEndpoint = IpEndpoint::new(UNSPECIFIED_IP_V4, 0);
