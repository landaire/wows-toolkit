use super::analyzer::Analyzer;
use crate::analyzer::decoder::PacketDecoder;
use crate::analyzer::*;
use crate::packet2::Packet;
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

pub struct SurveyStats {
    pub total_packets: usize,
    pub invalid_packets: usize,
    pub audits: Vec<String>,
    pub date_time: String,
}

impl Default for SurveyStats {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyStats {
    pub fn new() -> Self {
        Self { total_packets: 0, invalid_packets: 0, audits: vec![], date_time: "".to_string() }
    }
}

pub struct SurveyBuilder {
    stats: Rc<RefCell<SurveyStats>>,
    skip_decoder: bool,
}

impl SurveyBuilder {
    pub fn new(stats: Rc<RefCell<SurveyStats>>, skip_decoder: bool) -> Self {
        Self { stats, skip_decoder }
    }

    pub fn build(self, meta: &crate::ReplayMeta) -> Box<dyn Analyzer> {
        let version = wowsunpack::data::Version::from_client_exe(&meta.clientVersionFromExe);
        {
            let mut stats: RefMut<_> = self.stats.borrow_mut();
            stats.date_time = meta.dateTime.clone();
        }
        Box::new(Survey {
            skip_decoder: self.skip_decoder,
            decoder: decoder::DecoderBuilder::new(true, true, None).build(meta),
            stats: self.stats.clone(),
            packet_decoder: PacketDecoder::builder().version(version).audit(true).build(),
        })
    }
}

struct Survey {
    skip_decoder: bool,
    decoder: Box<dyn Analyzer>,
    stats: Rc<RefCell<SurveyStats>>,
    packet_decoder: PacketDecoder<'static>,
}

impl Analyzer for Survey {
    fn finish(&mut self) {
        self.decoder.finish();
    }

    fn process(&mut self, packet: &Packet<'_, '_>) {
        // Do stuff and such
        let mut stats: RefMut<_> = self.stats.borrow_mut();
        if !self.skip_decoder {
            let decoded = self.packet_decoder.decode(packet);
            if let crate::analyzer::decoder::DecodedPacketPayload::Audit(s) = &decoded.payload {
                stats.audits.push(s.to_string());
            }
        }

        if let crate::packet2::PacketType::Invalid(_) = &packet.payload {
            stats.invalid_packets += 1;
        }
        stats.total_packets += 1;
    }
}
