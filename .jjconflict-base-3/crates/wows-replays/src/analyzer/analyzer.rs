pub trait Analyzer {
    fn process(&mut self, packet: &crate::packet2::Packet<'_, '_>);
    fn finish(&mut self);
}
