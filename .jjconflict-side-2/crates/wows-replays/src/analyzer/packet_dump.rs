use crate::analyzer::Analyzer;
use crate::packet2::Packet;

pub struct PacketDumpBuilder {}

impl Default for PacketDumpBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketDumpBuilder {
    pub fn new() -> Self {
        Self {}
    }

    pub fn build(self, _: &crate::ReplayMeta) -> Box<dyn Analyzer> {
        Box::new(PacketDump {})
    }
}

struct PacketDump {}

impl Analyzer for PacketDump {
    fn finish(&mut self) {}

    fn process(&mut self, packet: &Packet<'_, '_>) {
        println!("{}", serde_json::to_string(packet).unwrap());
    }
}
