use alloc::vec::Vec;
use axerrno::{LinuxError, LinuxResult, ax_err};
use core::net::IpAddr;

use smoltcp::iface::SocketHandle;
use smoltcp::socket::dns::{self, GetQueryResultError, StartQueryError};
use smoltcp::wire::DnsQueryType;

use super::addr::into_core_ipaddr;
use super::{SOCKET_SET, SocketSetWrapper};

/// A DNS socket.
struct DnsSocket {
    handle: Option<SocketHandle>,
}

impl DnsSocket {
    #[allow(clippy::new_without_default)]
    /// Creates a new DNS socket.
    pub fn new() -> Self {
        let socket = SocketSetWrapper::new_dns_socket();
        let handle = Some(SOCKET_SET.add(socket));
        Self { handle }
    }

    #[allow(dead_code)]
    /// Update the list of DNS servers, will replace all existing servers.
    pub fn update_servers(self, servers: &[smoltcp::wire::IpAddress]) {
        SOCKET_SET.with_socket_mut::<dns::Socket, _, _>(self.handle.unwrap(), |socket| {
            socket.update_servers(servers)
        });
    }

    /// Query a address with given DNS query type.
    pub fn query(&self, name: &str, query_type: DnsQueryType) -> LinuxResult<Vec<IpAddr>> {
        // let local_addr = self.local_addr.unwrap_or_else(f);
        let handle = self.handle.ok_or_else(|| LinuxError::EINVAL)?;

        let iface = &super::ETH0.iface;
        let query_handle = SOCKET_SET
            .with_socket_mut::<dns::Socket, _, _>(handle, |socket| {
                socket.start_query(iface.lock().context(), name, query_type)
            })
            .map_err(|e| match e {
                StartQueryError::NoFreeSlot => ax_err!(EBUSY, "no free slot"),
                StartQueryError::InvalidName => ax_err!(EINVAL, "invalid name"),
                StartQueryError::NameTooLong => ax_err!(EINVAL, "name too long"),
            })?;
        loop {
            SOCKET_SET.poll_interfaces();
            match SOCKET_SET.with_socket_mut::<dns::Socket, _, _>(handle, |socket| {
                socket.get_query_result(query_handle).map_err(|e| match e {
                    GetQueryResultError::Pending => LinuxError::EAGAIN,
                    GetQueryResultError::Failed => ax_err!(ECONNREFUSED, "DNS query failed"),
                })
            }) {
                Ok(n) => {
                    let mut res = Vec::with_capacity(n.capacity());
                    for ip in n {
                        res.push(into_core_ipaddr(ip))
                    }
                    return Ok(res);
                }
                Err(LinuxError::EAGAIN) => axtask::yield_now(),
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for DnsSocket {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            SOCKET_SET.remove(handle);
        }
    }
}

/// Public function for DNS query.
pub fn dns_query(name: &str) -> LinuxResult<Vec<IpAddr>> {
    let socket = DnsSocket::new();
    socket.query(name, DnsQueryType::A)
}
