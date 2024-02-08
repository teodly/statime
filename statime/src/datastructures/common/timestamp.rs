

use crate::{
    datastructures::{WireFormat, WireFormatError},
    time::Time,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct WireTimestamp {
    /// The seconds field of the timestamp.
    /// 48-bit, must be less than 281474976710656
    pub seconds: u64,
    /// The nanoseconds field of the timestamp.
    /// Must be less than 10^9
    pub nanos: u32,
}

impl WireFormat for WireTimestamp {
    fn wire_size(&self) -> usize {
        10
    }

    fn serialize(&self, buffer: &mut [u8]) -> Result<(), WireFormatError> {
        buffer[0..6].copy_from_slice(&self.seconds.to_be_bytes()[2..8]);
        buffer[6..10].copy_from_slice(&self.nanos.to_be_bytes());
        Ok(())
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, WireFormatError> {
        let mut seconds_buffer = [0; 8];
        seconds_buffer[2..8].copy_from_slice(&buffer[0..6]);

        Ok(Self {
            seconds: u64::from_be_bytes(seconds_buffer),
            nanos: u32::from_be_bytes(buffer[6..10].try_into().unwrap()),
        })
    }
}

impl WireTimestamp {
    // XXX not used because not all PTPv1 messages have epochNumber field
    fn serialize_v1(&self, buffer: &mut [u8], epoch_buffer: &mut [u8]) -> Result<(), WireFormatError> {
        epoch_buffer[0..2].copy_from_slice(&self.seconds.to_be_bytes()[2..4]);
        buffer[0..4].copy_from_slice(&self.seconds.to_be_bytes()[4..8]);
        buffer[4..8].copy_from_slice(&self.nanos.to_be_bytes());
        Ok(())
    }

    fn deserialize_v1(buffer: &[u8], epoch_buffer: &[u8]) -> Result<Self, WireFormatError> {
        let mut seconds_buffer = [0; 8];
        seconds_buffer[2..4].copy_from_slice(&epoch_buffer[0..2]);
        seconds_buffer[4..8].copy_from_slice(&buffer[0..4]);
        Ok(Self {
            seconds: u64::from_be_bytes(seconds_buffer),
            nanos: u32::from_be_bytes(buffer[6..10].try_into().unwrap()),
        })
    }

    pub fn epoch_number_v1(&self) -> u16 {
        (self.seconds >> 32) as u16
    }

    pub fn from_v1_with_epoch_number(v1: WireTimestampV1, epoch_number: u16) -> Self {
        Self { seconds: (v1.seconds as u64) | ((epoch_number as u64) << 32), nanos: v1.nanos }
    }
}

impl From<Time> for WireTimestamp {
    fn from(instant: Time) -> Self {
        WireTimestamp {
            seconds: instant.secs(),
            nanos: instant.subsec_nanos(),
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct WireTimestampV1 {
    /// The seconds field of the timestamp.
    /// May wrap around (Y38K problem)
    pub seconds: u32,
    /// The nanoseconds field of the timestamp.
    /// Must be less than 10^9
    pub nanos: u32,
}

impl WireFormat for WireTimestampV1 {
    fn wire_size(&self) -> usize {
        8
    }

    fn serialize(&self, buffer: &mut [u8]) -> Result<(), WireFormatError> {
        buffer[0..4].copy_from_slice(&self.seconds.to_be_bytes());
        buffer[4..8].copy_from_slice(&self.nanos.to_be_bytes());
        Ok(())
    }

    fn deserialize(buffer: &[u8]) -> Result<Self, WireFormatError> {
        Ok(Self {
            seconds: u32::from_be_bytes(buffer[0..4].try_into().unwrap()),
            nanos: u32::from_be_bytes(buffer[4..8].try_into().unwrap()),
        })
    }
}

impl From<Time> for WireTimestampV1 {
    fn from(instant: Time) -> Self {
        WireTimestampV1 {
            seconds: instant.secs() as u32, // TODO: will wrap-around in 2038
            // TODO we need to account for this overflow in other parts of the code
            nanos: instant.subsec_nanos(),
        }
    }
}

impl From<WireTimestampV1> for WireTimestamp {
    fn from(v1: WireTimestampV1) -> Self {
        Self { seconds: v1.seconds as u64, nanos: v1.nanos }
    }
}

impl From<WireTimestamp> for WireTimestampV1 {
    fn from(v2: WireTimestamp) -> Self {
        Self { seconds: v2.seconds as u32, nanos: v2.nanos }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_wireformat() {
        let representations = [
            (
                [0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01u8],
                WireTimestamp {
                    seconds: 0x0000_0000_0002,
                    nanos: 0x0000_0001,
                },
            ),
            (
                [0x10, 0x00, 0x00, 0x00, 0x00, 0x02, 0x10, 0x00, 0x00, 0x01u8],
                WireTimestamp {
                    seconds: 0x1000_0000_0002,
                    nanos: 0x1000_0001,
                },
            ),
        ];

        for (byte_representation, object_representation) in representations {
            // Test the serialization output
            let mut serialization_buffer = [0; 10];
            object_representation
                .serialize(&mut serialization_buffer)
                .unwrap();
            assert_eq!(serialization_buffer, byte_representation);

            // Test the deserialization output
            let deserialized_data = WireTimestamp::deserialize(&byte_representation).unwrap();
            assert_eq!(deserialized_data, object_representation);
        }
    }
}
