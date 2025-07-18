use alloc::vec;

use smoltcp::{
    storage::{PacketBuffer, PacketMetadata},
    time::Instant,
    wire::IpAddress,
};

use crate::{
    consts::{SOCKET_BUFFER_SIZE, STANDARD_MTU},
    device::Device,
};

pub struct LoopbackDevice {
    buffer: PacketBuffer<'static, ()>,
}
impl LoopbackDevice {
    pub fn new() -> Self {
        let buffer = PacketBuffer::new(
            vec![PacketMetadata::EMPTY; SOCKET_BUFFER_SIZE],
            vec![0u8; STANDARD_MTU * SOCKET_BUFFER_SIZE],
        );
        Self { buffer }
    }
}

impl Device for LoopbackDevice {
    fn name(&self) -> &str {
        "lo"
    }

    fn recv(&mut self, buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> bool {
        self.buffer.dequeue().ok().is_some_and(|(_, rx_buf)| {
            buffer
                .enqueue(rx_buf.len(), ())
                .unwrap()
                .copy_from_slice(rx_buf);
            true
        })
    }

    fn send(&mut self, next_hop: IpAddress, packet: &[u8], _timestamp: Instant) {
        match self.buffer.enqueue(packet.len(), ()) {
            Ok(tx_buf) => tx_buf.copy_from_slice(packet),
            Err(_) => {
                warn!(
                    "Loopback device buffer is full, dropping packet to {}",
                    next_hop
                );
            }
        }
    }
}
