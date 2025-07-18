use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};

mod ethernet;
mod loopback;

pub use ethernet::*;
pub use loopback::*;

pub trait Device: Send + Sync {
    fn name(&self) -> &str;

    fn recv(&mut self, buffer: &mut PacketBuffer<()>, timestamp: Instant) -> bool;
    fn send(&mut self, next_hop: IpAddress, packet: &[u8], timestamp: Instant);
}
