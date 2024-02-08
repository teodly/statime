//! PTPv1 network messages

pub(crate) use sync_or_delay_req::*;
pub(crate) use delay_resp::*;
pub(crate) use follow_up::*;
pub use header::*;


use super::messages::EnumConversionError;


/* use self::{
    management::ManagementMessage,
}; */
use super::{
    WireFormatError,
};
use crate::config::ClockIdentity;


mod sync_or_delay_req;
mod control_field;
use control_field::ControlField;
mod delay_resp;
mod follow_up;
mod header;

pub mod converter;


/// Maximum length of a packet
///
/// This can be used to preallocate buffers that can always fit packets send by
/// `statime`.
pub const MAX_DATA_LEN: usize = 255;

/// Type of message, used to differentiate low-delay and other messages.
/// `Event` is low-delay.
/// 
/// To avoid confusion with PTPv2 MessageType which is equivalent to ControlField
/// in PTPv1, PTPv1's messageType is called here PortType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PortType {
    Event = 0x1,
    General = 0x2,
}

impl TryFrom<u8> for PortType {
    type Error = EnumConversionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use PortType::*;

        match value {
            0x1 => Ok(Event),
            0x2 => Ok(General),
            _ => Err(EnumConversionError(value)),
        }
    }
}

#[cfg(feature = "fuzz")]
pub use fuzz::FuzzMessage;

#[cfg(feature = "fuzz")]
mod fuzz {
    #![allow(missing_docs)] // These are only used for internal fuzzing
    use super::*;
    use crate::datastructures::{common::Tlv, WireFormatError};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FuzzMessage<'a> {
        inner: Message<'a>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FuzzTlv<'a>(Tlv<'a>);

    impl<'a> FuzzMessage<'a> {
        pub fn deserialize(buffer: &'a [u8]) -> Result<Self, impl std::error::Error> {
            Ok::<FuzzMessage, WireFormatError>(FuzzMessage {
                inner: Message::deserialize(buffer)?,
            })
        }

        pub fn serialize(&self, buffer: &mut [u8]) -> Result<usize, impl std::error::Error> {
            self.inner.serialize(buffer)
        }

        pub fn tlv(&self) -> impl Iterator<Item = FuzzTlv<'_>> + '_ {
            self.inner.suffix.tlv().map(FuzzTlv)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) header: Header,
    pub(crate) body: MessageBody,
}

impl Message {
    pub(crate) fn is_event(&self) -> bool {
        use MessageBody::*;
        match self.body {
            Sync(_) | DelayReq(_) => true,
            FollowUp(_) | DelayResp(_) /* | Management(_) */ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MessageBody {
    Sync(SyncMessage),
    DelayReq(DelayReqMessage),
    FollowUp(FollowUpMessage),
    DelayResp(DelayRespMessage),
    //Management(ManagementMessage), // TODO
}

impl MessageBody {
    fn wire_size(&self) -> usize {
        match &self {
            MessageBody::Sync(m) => m.content_size(),
            MessageBody::DelayReq(m) => m.content_size(),
            MessageBody::FollowUp(m) => m.content_size(),
            MessageBody::DelayResp(m) => m.content_size(),
            /* MessageBody::Management(m) => m.content_size(), */
        }
    }

    fn content_type(&self) -> ControlField {
        match self {
            MessageBody::Sync(_) => ControlField::Sync,
            MessageBody::DelayReq(_) => ControlField::DelayReq,
            MessageBody::FollowUp(_) => ControlField::FollowUp,
            MessageBody::DelayResp(_) => ControlField::DelayResp,
            /* MessageBody::Management(_) => MessageType::Management, */
        }
    }

    pub(crate) fn serialize(&self, buffer: &mut [u8]) -> Result<usize, super::WireFormatError> {
        match &self {
            MessageBody::Sync(m) => m.serialize_content(buffer)?,
            MessageBody::DelayReq(m) => m.serialize_content(buffer)?,
            MessageBody::FollowUp(m) => m.serialize_content(buffer)?,
            MessageBody::DelayResp(m) => m.serialize_content(buffer)?,
            /* MessageBody::Management(m) => m.serialize_content(buffer)?, */
        }

        Ok(self.wire_size())
    }

    pub(crate) fn deserialize(
        message_type: ControlField,
        _header: &Header,
        buffer: &[u8],
    ) -> Result<Self, super::WireFormatError> {
        let body = match message_type {
            ControlField::Sync => MessageBody::Sync(SyncMessage::deserialize_content(buffer)?),
            ControlField::DelayReq => {
                MessageBody::DelayReq(DelayReqMessage::deserialize_content(buffer)?)
            }
            ControlField::FollowUp => {
                MessageBody::FollowUp(FollowUpMessage::deserialize_content(buffer)?)
            }
            ControlField::DelayResp => {
                MessageBody::DelayResp(DelayRespMessage::deserialize_content(buffer)?)
            }
            /* ControlField::Management => {
                MessageBody::Management(ManagementMessage::deserialize_content(buffer)?)
            } */
        };

        Ok(body)
    }
}

fn v2_clock_identity_to_v1_clock_uuid(identity: &ClockIdentity) -> [u8; 6] {
    identity.0[0..6].try_into().unwrap()
    // XXX: this is correct only if clock identity is synthesized by right-padding mac address with zeros as in statime-linux
}
/* 
fn grandmaster_properties_from_parent_ds(ds: &ParentDS) -> GrandmasterPropertiesV1 {
    GrandmasterPropertiesV1 {
        communication_technology: 1,
        clock_uuid: v2_clock_identity_to_v1_clock_uuid(&ds.grandmaster_identity),
        port_id: 0, // FIXME
        sequence_id: 0, // FIXME
        clock_stratum: ds.grandmaster_clock_quality.clock_class,
        clock_identifier: [b'D', b'F', b'L', b'T'], // FIXME
        clock_variance: 0, // ??? FIXME
        preferred: ds.grandmaster_priority_1 > 128,
        is_boundary_clock: true // ??? FIXME
    }
}

fn base_header(default_ds: &DefaultDS, port_identity: PortIdentity, sequence_id: u16) -> Header {
    Header {
        // TODO subdomain
        source_uuid: v2_clock_identity_to_v1_clock_uuid(&port_identity.clock_identity),
        source_port_id: port_identity.port_number,
        sequence_id,
        ..Default::default()
    }
}

fn sync_or_delay_req(global: &PtpInstanceState) -> SyncOrDelayReqMessage {
    let time_properties_ds = &global.time_properties_ds;
    SyncOrDelayReqMessage {
        origin_timestamp: Default::default(),
        epoch_number: 0,
        current_utc_offset: time_properties_ds.current_utc_offset.unwrap_or(0),
        grandmaster: grandmaster_properties_from_parent_ds(&global.parent_ds),
        sync_interval: -2,
        local_clock_variance: global.default_ds.clock_quality.offset_scaled_log_variance as i16, // ???
        local_steps_removed: global.current_ds.steps_removed,
        local_clock_stratum: global.default_ds.clock_quality.clock_class,
        local_clock_identifier: [b'D', b'F', b'L', b'T'], // TODO
        parent_communication_technology: 1,
        parent_uuid: v2_clock_identity_to_v1_clock_uuid(&global.parent_ds.parent_port_identity.clock_identity),
        parent_port_field: global.parent_ds.parent_port_identity.port_number,
        estimated_master_variance: global.parent_ds.observed_parent_offset_scaled_log_variance as i16, // ???
        estimated_master_drift: global.parent_ds.observed_parent_clock_phase_change_rate as i32, // ???
        utc_reasonable: time_properties_ds.current_utc_offset.is_some()
    }
}

impl Message {
    pub(crate) fn sync(
        global: &PtpInstanceState,
        port_identity: PortIdentity,
        sequence_id: u16,
    ) -> Self {
        let time_properties_ds = &global.time_properties_ds;

        let header = Header {
            leap59: time_properties_ds.leap_indicator == LeapIndicator::Leap59,
            leap61: time_properties_ds.leap_indicator == LeapIndicator::Leap61,
            ptp_assist: true,
            ptp_boundary_clock: true, // assume that we're a boundary clock if we're a master, TODO is it correct?
            ..base_header(&global.default_ds, port_identity, sequence_id)
        };

        Message {
            header,
            body: MessageBody::Sync(sync_or_delay_req(&global)),
        }
    }

    pub(crate) fn follow_up(
        global: &PtpInstanceState,
        port_identity: PortIdentity,
        sequence_id: u16,
        timestamp: Time,
    ) -> Self {
        let header = Header {
            ..base_header(&global.default_ds, port_identity, sequence_id)
        };

        Message {
            header,
            body: MessageBody::FollowUp(FollowUpMessage {
                associated_sequence_id: sequence_id,
                precise_origin_timestamp: timestamp.into(),
            }),
        }
    }

/*     pub(crate) fn _announce(
        global: &PtpInstanceState,
        port_identity: PortIdentity,
        sequence_id: u16,
    ) -> Self {
        let time_properties_ds = &global.time_properties_ds;

        let header = Header {
            leap59: time_properties_ds.leap_indicator == LeapIndicator::Leap59,
            leap61: time_properties_ds.leap_indicator == LeapIndicator::Leap61,
            current_utc_offset_valid: time_properties_ds.current_utc_offset.is_some(),
            ptp_timescale: time_properties_ds.ptp_timescale,
            time_tracable: time_properties_ds.time_traceable,
            frequency_tracable: time_properties_ds.frequency_traceable,
            ..base_header(&global.default_ds, port_identity, sequence_id)
        };

        let body = MessageBody::Announce(AnnounceMessage {
            header,
            origin_timestamp: Default::default(),
            current_utc_offset: time_properties_ds.current_utc_offset.unwrap_or_default(),
            grandmaster_priority_1: global.parent_ds.grandmaster_priority_1,
            grandmaster_clock_quality: global.parent_ds.grandmaster_clock_quality,
            grandmaster_priority_2: global.parent_ds.grandmaster_priority_2,
            grandmaster_identity: global.parent_ds.grandmaster_identity,
            steps_removed: global.current_ds.steps_removed,
            time_source: time_properties_ds.time_source,
        });

        Message {
            header,
            body,
        }
    } */

    pub(crate) fn delay_req(
        global: &PtpInstanceState,
        default_ds: &DefaultDS,
        port_identity: PortIdentity,
        sequence_id: u16,
    ) -> Self {
        let header = Header {
            ..base_header(default_ds, port_identity, sequence_id)
        };

        Message {
            header,
            body: MessageBody::DelayReq(sync_or_delay_req(&global)),
        }
    }

    pub(crate) fn delay_resp(
        global: &PtpInstanceState,
        request_header: Header,
        request: DelayReqMessage,
        port_identity: PortIdentity,
        sequence_id: u16,
        min_delay_req_interval: Interval,
        timestamp: Time,
    ) -> Self {
        let time_properties_ds = &global.time_properties_ds;
        // TODO is it really correct that we don't use any of the data?
        let _ = request;

        let header = Header {
            leap59: time_properties_ds.leap_indicator == LeapIndicator::Leap59,
            leap61: time_properties_ds.leap_indicator == LeapIndicator::Leap61,
            ptp_assist: true,
            ptp_boundary_clock: true, // assume that we're a boundary clock if we're a master, TODO is it correct?
            ..base_header(&global.default_ds, port_identity, sequence_id)
        };

        let body = MessageBody::DelayResp(DelayRespMessage {
            receive_timestamp: timestamp.into(),
            requesting_source_communication_technology: 1,
            requesting_source_uuid: request_header.source_uuid,
            requesting_source_port_id: request_header.source_port_id,
            requesting_source_sequence_id: request_header.sequence_id
        });

        Message {
            header,
            body,
        }
    }
}
 */

impl Message {
    pub(crate) fn header(&self) -> &Header {
        &self.header
    }

    /// The byte size on the wire of this message
    pub(crate) fn wire_size(&self) -> usize {
        self.header.wire_size() + self.body.wire_size()
    }

    /// Serializes the object into the PTP wire format.
    ///
    /// Returns the used buffer size that contains the message or an error.
    pub(crate) fn serialize(&self, buffer: &mut [u8]) -> Result<usize, super::WireFormatError> {
        let (header, rest) = buffer.split_at_mut(40);
        let (body, _tlv) = rest.split_at_mut(self.body.wire_size());

        self.header
            .serialize_header(
                self.body.content_type(),
                self.body.wire_size(),
                header,
            )
            .unwrap();

        self.body.serialize(body).unwrap();

        Ok(self.wire_size())
    }

    /// Deserializes a message from the PTP wire format.
    ///
    /// Returns the message or an error.
    pub(crate) fn deserialize(buffer: &[u8]) -> Result<Self, super::WireFormatError> {
        let header_data = Header::deserialize_header(buffer)?;

        /* if header_data.message_length < 34 {
            return Err(WireFormatError::Invalid);
        } */

        // Ensure we have the entire message and ignore potential padding
        // Skip the header bytes and only keep the content
        let content_buffer = buffer
            .get(40..buffer.len())
            .ok_or(WireFormatError::BufferTooShort)?;

        let body = MessageBody::deserialize(
            header_data.control,
            &header_data.header,
            content_buffer,
        )?;

        Ok(Message {
            header: header_data.header,
            body,
        })
    }
}
