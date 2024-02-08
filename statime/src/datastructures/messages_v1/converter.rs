
use arrayvec::ArrayVec;
use messages_v2::SdoId;

use crate::config::ClockIdentity;
use crate::config::ClockQuality;
use crate::datastructures::common::PortIdentity;
use crate::datastructures::common::TimeInterval;

use crate::datastructures::common::TLV_SET_EMPTY;

use crate::datastructures::messages::PtpVersion;
use crate::datastructures::messages_v1;
use crate::datastructures::messages as messages_v2;
use crate::datastructures::messages_v1::v2_clock_identity_to_v1_clock_uuid;
use crate::datastructures::messages_v1::SyncOrDelayReqMessage;
use crate::datastructures::WireFormatError;


#[derive(Debug)]
pub enum MessageConversionError{
    DeserializeErrorV1(WireFormatError),
    DeserializeErrorV2(WireFormatError),
}

pub const MAX_MESSAGE_LENGTH: usize = 128;
pub type MessageContainer<T> = ArrayVec<T, 2>;
pub type MessageBytes = ArrayVec<u8, MAX_MESSAGE_LENGTH>;

pub(crate) fn container_from_single<T>(elem: T) -> MessageContainer<T> {
    let mut container = MessageContainer::new();
    container.push(elem);
    container
}

/// PTPv2<->v1 converter
#[derive(Debug)]
pub struct MessageConverter {
    out_v1_seq_id: u16,
    out_v2_seq_id: u16,
    //last_v1_in_sync_seq_id: Option<u16>,
    last_v1_in_sync: Option<(messages_v1::Header, SyncOrDelayReqMessage)>,
    last_v2_out_sync_seq_id: Option<u16>,
    last_v2_in_delay_req_seq_id: Option<u16>,
}

impl MessageConverter {
    /// Create the converter state
    pub fn new() -> Self {
        Self {
            out_v1_seq_id: 0,
            out_v2_seq_id: 0,
            //last_v1_in_sync_seq_id: None,
            last_v1_in_sync: None,
            last_v2_out_sync_seq_id: None,
            last_v2_in_delay_req_seq_id: None,
        }
    }
    fn v1_to_v2(&mut self, msgv1: messages_v1::Message) -> Result<MessageContainer<messages_v2::Message<'static>>, MessageConversionError> {
        let mut messages_v2 = MessageContainer::new();
        if let messages_v1::MessageBody::Sync(body_v1) = msgv1.body {
            self.last_v1_in_sync = Some((msgv1.header.clone(), body_v1.clone()));
        } else {
            let mut master_changed = false;
            if let Some((last_hdr_v1, _last_body_v1)) = self.last_v1_in_sync {
                use messages_v1::MessageBody;
                match msgv1.body {
                    MessageBody::Sync(_) | MessageBody::FollowUp(_) | MessageBody::DelayResp(_) => {
                        // this is message from Master
                        if last_hdr_v1.source_uuid != msgv1.header.source_uuid || last_hdr_v1.source_port_id != msgv1.header.source_port_id {
                            // Master has changed in the meantime, the previous Sync is no longer valid
                            master_changed = true;
                        }
                    },
                    _ => {}
                }
            }
            if master_changed {
                self.last_v1_in_sync = None;
            }
        }
        let header_v2 = messages_v2::Header {
            sdo_id: SdoId::try_from(0u16).unwrap(),
            version: PtpVersion::new(2, 0).unwrap(),
            domain_number: 0 /*FIXME*/,
            alternate_master_flag: false/*FIXME*/,
            two_step_flag: msgv1.header.ptp_assist,
            unicast_flag: false,
            ptp_profile_specific_1: false,
            ptp_profile_specific_2: false,
            leap61: msgv1.header.leap61,
            leap59: msgv1.header.leap59,
            current_utc_offset_valid: match self.last_v1_in_sync {
                Some((_hdr, sync_v1)) => sync_v1.utc_reasonable,
                None => false
            },
            ptp_timescale: match self.last_v1_in_sync {
                Some((_hdr, sync_v1)) => [*b"ATOM", *b"GPS\0", *b"NTP\0", *b"HAND"].contains(&sync_v1.grandmaster.clock_identifier),
                _ => false
            },
            time_tracable: false/*FIXME*/,
            frequency_tracable: false/*FIXME*/,
            synchronization_uncertain: false /* really? */,
            correction_field: TimeInterval(0.into()),
            source_port_identity: PortIdentity {
                clock_identity: ClockIdentity::from_mac_address(msgv1.header.source_uuid.clone()),
                port_number: msgv1.header.source_port_id,
            },
            sequence_id: self.out_v2_seq_id,
            log_message_interval: 0 /*FIXME*/,
        };
        match msgv1.body {
            messages_v1::MessageBody::Sync(body_v1) => {
                //let origin_timestamp = WireTimestamp::from_v1_with_epoch_number(body_v1.origin_timestamp, body_v1.epoch_number); // TODO implement epoch number in FollowUp handler, for now let's just ignore epoch numbers
                let origin_timestamp = body_v1.origin_timestamp.into();
                self.out_v2_seq_id = self.out_v2_seq_id.wrapping_add(1);
                messages_v2.push(messages_v2::Message{
                    header: header_v2.clone(),
                    body: messages_v2::MessageBody::Announce(messages_v2::AnnounceMessage {
                        header: header_v2.clone(),
                        origin_timestamp,
                        current_utc_offset: body_v1.current_utc_offset,
                        grandmaster_priority_1: if body_v1.grandmaster.preferred { 64 } else { 128 },
                        grandmaster_clock_quality: ClockQuality {
                            clock_class: body_v1.grandmaster.clock_stratum,
                            clock_accuracy: crate::config::ClockAccuracy::Unknown,
                            offset_scaled_log_variance: body_v1.grandmaster.clock_variance as u16 /*FIXME*/
                        },
                        grandmaster_priority_2: 128,
                        grandmaster_identity: ClockIdentity::from_mac_address(body_v1.grandmaster.clock_uuid.clone()),
                        steps_removed: body_v1.local_steps_removed,
                        time_source: crate::config::TimeSource::InternalOscillator /*FIXME*/
                    }),
                    suffix: TLV_SET_EMPTY,
                });
                let header2_v2 = messages_v2::Header{ sequence_id: self.out_v2_seq_id, ..header_v2 };
                self.last_v2_out_sync_seq_id = Some(self.out_v2_seq_id);
                self.out_v2_seq_id = self.out_v2_seq_id.wrapping_add(1);
                messages_v2.push(messages_v2::Message {
                    header: header2_v2,
                    body: messages_v2::MessageBody::Sync(messages_v2::SyncMessage { origin_timestamp }),
                    suffix: TLV_SET_EMPTY,
                });
            },
            messages_v1::MessageBody::FollowUp(body_v1) => {
                match (self.last_v1_in_sync, self.last_v2_out_sync_seq_id) {
                    (Some((hdr_v1, _sync_v1)), Some(last_v2_out_sync_seq_id)) if hdr_v1.sequence_id==body_v1.associated_sequence_id => {
                        messages_v2.push(messages_v2::Message {
                            header: messages_v2::Header { sequence_id: last_v2_out_sync_seq_id, ..header_v2 },
                            body: messages_v2::MessageBody::FollowUp(messages_v2::FollowUpMessage {
                                precise_origin_timestamp: body_v1.precise_origin_timestamp.into()
                            }),
                            suffix: TLV_SET_EMPTY,
                        });
                    },
                    _ => {
                        log::warn!("lost Sync associated with incoming v1 Follow-Up");
                    }
                }
            },
            messages_v1::MessageBody::DelayReq(_body_v1) => {
                log::warn!("TODO: got Delay Request but master operation not supported yet");
            },
            messages_v1::MessageBody::DelayResp(body_v1) => {
                if let Some(sequence_id) = self.last_v2_in_delay_req_seq_id {
                    messages_v2.push(messages_v2::Message {
                        header: messages_v2::Header { sequence_id, ..header_v2 },
                        body: messages_v2::MessageBody::DelayResp(messages_v2::DelayRespMessage {
                            receive_timestamp: body_v1.receive_timestamp.into(),
                            requesting_port_identity: PortIdentity {
                                clock_identity: ClockIdentity::from_mac_address(body_v1.requesting_source_uuid),
                                port_number: body_v1.requesting_source_port_id
                            }
                        }),
                        suffix: TLV_SET_EMPTY,
                    });
                }
            }
        }
        Ok(messages_v2)
    }

    fn v2_to_v1(&mut self, msgv2: messages_v2::Message) -> Result<MessageContainer<messages_v1::Message>, MessageConversionError> {
        let mut messages_v1 = MessageContainer::new();
        let header_v1 = messages_v1::Header {
            version_ptp: 1,
            version_network: 1,
            subdomain: *b"_DFLT\0\0\0\0\0\0\0\0\0\0\0" /*FIXME*/,
            source_communication_technology: 1,
            leap61: msgv2.header.leap61,
            leap59: msgv2.header.leap59,
            source_uuid: v2_clock_identity_to_v1_clock_uuid(&msgv2.header.source_port_identity.clock_identity),
            source_port_id: msgv2.header.source_port_identity.port_number,
            sequence_id: msgv2.header.sequence_id /* really? */,
            ptp_boundary_clock: false /*FIXME*/,
            ptp_assist: true /* really? */,
            ptp_ext_sync: false /* TODO set to true if one of our ports is Master */,
            parent_stats: false /* TODO */,
            ptp_sync_burst: false
        };
        match msgv2.body {
            messages_v2::MessageBody::DelayReq(body_v2) => {
                if let Some((last_hdr_v1, last_sync_v1)) = self.last_v1_in_sync {
                    messages_v1.push(messages_v1::Message{
                        header: header_v1,

                        body: messages_v1::MessageBody::DelayReq(SyncOrDelayReqMessage {
                            origin_timestamp: body_v2.origin_timestamp.into(),
                            epoch_number: 0 /* TODO epoch handling logic */,
                            local_clock_variance: -4000 /* FIXME */,
                            local_steps_removed: last_sync_v1.local_steps_removed + 1,
                            local_clock_stratum: 254 /* FIXME */,
                            parent_uuid: last_hdr_v1.source_uuid.clone(),
                            parent_port_field: last_hdr_v1.source_port_id,
                            estimated_master_variance: 0,
                            estimated_master_drift: 0,
                            utc_reasonable: false /* really? */,
                            ..last_sync_v1
                        })
                    });
                    self.last_v2_in_delay_req_seq_id = Some(msgv2.header.sequence_id);
                }
            },
            _ => {
                log::warn!("TODO: got v2 message from master, not supported yet");
            }
        }
        Ok(messages_v1)
    }

    // FIXME DRY
    // FIXME change unwraps to something more sane

    /// Convert PTPv1 to PTPv2 payload bytes
    pub fn v1_to_v2_bytes(&mut self, buffer_v1: &[u8]) -> Result<MessageContainer<MessageBytes>, MessageConversionError> {
        if buffer_v1.len()<4 || buffer_v1[0..4] != [0, 1, 0, 1] {
            // not PTPv1 message, ignore
            return Ok(MessageContainer::<MessageBytes>::new());
        }
        let msgsv2 = self.v1_to_v2(messages_v1::Message::deserialize(buffer_v1).map_err(|e|MessageConversionError::DeserializeErrorV1(e))?)?;
        Ok(MessageContainer::from_iter(msgsv2.iter().map(|msgv2| {
            let mut outbuf = [0u8; MAX_MESSAGE_LENGTH];
            let len = messages_v2::Message::serialize(msgv2, &mut outbuf).unwrap();
            let mut bytes = MessageBytes::from_iter(outbuf);
            bytes.truncate(len);
            bytes
        })))
    }

    /// Convert PTPv2 to PTPv1 payload bytes
    pub fn v2_to_v1_bytes(&mut self, buffer_v2: &[u8]) -> Result<MessageContainer<MessageBytes>, MessageConversionError> {
        if buffer_v2.len()<2 || (buffer_v2[1] & 0xf) != 2 {
            // not PTPv2 message, ignore
            return Ok(MessageContainer::<MessageBytes>::new());
        }
        let msgsv1 = self.v2_to_v1(messages_v2::Message::deserialize(buffer_v2).map_err(|e|MessageConversionError::DeserializeErrorV2(e))?)?;
        Ok(MessageContainer::from_iter(msgsv1.iter().map(|msgv1| {
            let mut outbuf = [0u8; MAX_MESSAGE_LENGTH];
            let len = messages_v1::Message::serialize(msgv1, &mut outbuf).unwrap();
            let mut bytes = MessageBytes::from_iter(outbuf);
            bytes.truncate(len);
            bytes
        })))
    }
}
