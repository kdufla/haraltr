use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mac([u8; 6]);

impl Mac {
    pub fn bytes(&self) -> [u8; 6] {
        self.0
    }
}

impl From<[u8; 6]> for Mac {
    fn from(v: [u8; 6]) -> Self {
        Self(v)
    }
}

impl From<Mac> for bdaddr::Address {
    fn from(v: Mac) -> Self {
        let mut rev = v.0;
        rev.reverse();
        bdaddr::Address::from(rev)
    }
}

impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            b[0], b[1], b[2], b[3], b[4], b[5]
        )
    }
}

impl fmt::Debug for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Mac {
    type Err = ParseMacError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 17 {
            return Err(ParseMacError);
        }
        let mut out = [0u8; 6];
        for (res_byte, chunk) in out.iter_mut().zip(bytes.chunks(3)) {
            let hi = parse_ascii_hex(chunk[0])?;
            let lo = parse_ascii_hex(chunk[1])?;
            *res_byte = (hi << 4) | lo;
            if let Some(&sep) = chunk.get(2)
                && sep != b':'
                && sep != b'-'
            {
                return Err(ParseMacError);
            }
        }
        Ok(Mac(out))
    }
}

fn parse_ascii_hex(digit: u8) -> Result<u8, ParseMacError> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err(ParseMacError),
    }
}

impl Serialize for Mac {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Mac {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseMacError;

impl fmt::Display for ParseMacError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid MAC address: expected six hex pairs separated by ':' or '-'")
    }
}

impl std::error::Error for ParseMacError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colon_form_uppercase() {
        let mac: Mac = "AA:BB:CC:DD:EE:FF".parse().unwrap();
        assert_eq!(mac.bytes(), [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn parses_dash_form_lowercase() {
        let mac: Mac = "aa-bb-cc-dd-ee-ff".parse().unwrap();
        assert_eq!(mac.bytes(), [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn parses_mixed_separators() {
        let mac: Mac = "Aa:Bb-Cc:Dd-Ee:Ff".parse().unwrap();
        assert_eq!(mac.bytes(), [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn display_is_uppercase_colons() {
        let mac = Mac([0x01, 0x23, 0x45, 0x67, 0x89, 0xAB]);
        assert_eq!(mac.to_string(), "01:23:45:67:89:AB");
    }

    #[test]
    fn colon_and_dash_forms_are_equal() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let a: Mac = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        let b: Mac = "AA-BB-CC-DD-EE-FF".parse().unwrap();
        assert_eq!(a, b);
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn rejects_wrong_length() {
        assert!("AA:BB:CC:DD:EE".parse::<Mac>().is_err());
        assert!("AA:BB:CC:DD:EE:FF:00".parse::<Mac>().is_err());
        assert!("".parse::<Mac>().is_err());
    }

    #[test]
    fn rejects_non_hex() {
        assert!("ZZ:BB:CC:DD:EE:FF".parse::<Mac>().is_err());
        assert!("AA:BB:CC:DD:EE:F_".parse::<Mac>().is_err());
    }

    #[test]
    fn rejects_wrong_separator() {
        assert!("AA.BB.CC.DD.EE.FF".parse::<Mac>().is_err());
        assert!("AA,BB,CC,DD,EE,FF".parse::<Mac>().is_err());
        assert!("AA_BB_CC_DD_EE_FF".parse::<Mac>().is_err());
        assert!("AABBCCDDEEFF".parse::<Mac>().is_err());
    }

    #[test]
    fn serde_round_trip() {
        let mac: Mac = "AA:BB:CC:DD:EE:FF".parse().unwrap();
        let json = serde_json::to_string(&mac).unwrap();
        assert_eq!(json, r#""AA:BB:CC:DD:EE:FF""#);
        let back: Mac = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mac);
    }

    #[test]
    fn serde_accepts_dash_form() {
        let back: Mac = serde_json::from_str(r#""aa-bb-cc-dd-ee-ff""#).unwrap();
        assert_eq!(back.bytes(), [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }
}
