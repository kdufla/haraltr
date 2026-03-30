use std::{mem::size_of, ptr};

pub const IPC_SOCKET_PATH: &str = "/run/haraltr/query.sock";

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueryKind {
    IsDeviceNear = 0x01,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProximityStatus {
    Near = 0x01,
    Far = 0x02,
    Disconnected = 0x03,
    Unknown = 0xFF,
}

impl ProximityStatus {
    pub fn is_near(self) -> bool {
        matches!(self, ProximityStatus::Near)
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct QueryResponse {
    pub status: u8,
    pub rpl: f32,
    pub timestamp_secs: u64,
}

const _: () = assert!(size_of::<QueryResponse>() == 13);

impl QueryResponse {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) }
    }

    pub fn from_bytes(bytes: &[u8; size_of::<QueryResponse>()]) -> Self {
        unsafe { ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let original = QueryResponse {
            status: ProximityStatus::Near as u8,
            rpl: 12.5,
            timestamp_secs: 1234567890,
        };

        let bytes: [u8; size_of::<QueryResponse>()] = original.as_bytes().try_into().unwrap();
        let restored = QueryResponse::from_bytes(&bytes);

        assert_eq!(original.status, restored.status);
        assert_eq!({ original.rpl }, { restored.rpl });
        assert_eq!({ original.timestamp_secs }, { restored.timestamp_secs });
    }

    #[test]
    fn status_values() {
        assert_eq!(ProximityStatus::Near as u8, 0x01);
        assert_eq!(ProximityStatus::Far as u8, 0x02);
        assert_eq!(ProximityStatus::Disconnected as u8, 0x03);
        assert_eq!(ProximityStatus::Unknown as u8, 0xFF);
    }

    #[test]
    fn query_kind_value() {
        assert_eq!(QueryKind::IsDeviceNear as u8, 0x01);
    }

    #[test]
    fn is_near() {
        assert!(ProximityStatus::Near.is_near());
        assert!(!ProximityStatus::Far.is_near());
        assert!(!ProximityStatus::Disconnected.is_near());
        assert!(!ProximityStatus::Unknown.is_near());
    }
}
