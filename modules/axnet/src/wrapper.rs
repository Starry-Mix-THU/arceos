use alloc::vec;

use axerrno::{LinuxError, LinuxResult};
use axsync::Mutex;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::{AnySocket, Socket},
    wire::IpAddress,
};

pub(crate) struct SocketSetWrapper<'a>(pub Mutex<SocketSet<'a>>);

impl<'a> SocketSetWrapper<'a> {
    pub fn new() -> Self {
        Self(Mutex::new(SocketSet::new(vec![])))
    }

    pub fn add<T: AnySocket<'a>>(&self, socket: T) -> SocketHandle {
        let handle = self.0.lock().add(socket);
        debug!("socket {}: created", handle);
        handle
    }

    pub fn with_socket<T: AnySocket<'a>, R, F>(&self, handle: SocketHandle, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let set = self.0.lock();
        let socket = set.get(handle);
        f(socket)
    }

    pub fn with_socket_mut<T: AnySocket<'a>, R, F>(&self, handle: SocketHandle, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut set = self.0.lock();
        let socket = set.get_mut(handle);
        f(socket)
    }

    pub fn bind_check(&self, addr: IpAddress, port: u16) -> LinuxResult {
        if port == 0 {
            return Ok(());
        }

        // TODO(mivik): optimize
        let mut sockets = self.0.lock();
        for (_, socket) in sockets.iter_mut() {
            match socket {
                Socket::Tcp(s) => {
                    let local_addr = s.get_bound_endpoint();
                    if local_addr.addr == Some(addr) && local_addr.port == port {
                        return Err(LinuxError::EADDRINUSE);
                    }
                }
                Socket::Udp(s) => {
                    if s.endpoint().addr == Some(addr) && s.endpoint().port == port {
                        return Err(LinuxError::EADDRINUSE);
                    }
                }
                _ => continue,
            };
        }
        Ok(())
    }

    pub fn remove(&self, handle: SocketHandle) {
        self.0.lock().remove(handle);
        debug!("socket {}: destroyed", handle);
    }
}
