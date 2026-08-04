//! Optional serde support: [`Url`] as an href string.

use alloc::string::String;
use core::fmt;

use serde::de::{Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{ParseError, Url};

impl Serialize for Url<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Url<'static> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(UrlVisitor)
    }
}

struct UrlVisitor;

impl Visitor<'_> for UrlVisitor {
    type Value = Url<'static>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a URL string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Url::parse(v).map(Url::into_owned).map_err(|e| match e {
            ParseError::Failure => E::invalid_value(Unexpected::Str(v), &self),
            ParseError::InputTooLong => E::custom("URL input exceeds u32 index range"),
        })
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_href() {
        let url = Url::parse("https://example.com/a?b=1#c")
            .unwrap()
            .into_owned();
        let json = serde_json::to_string(&url).unwrap();
        assert_eq!(json, "\"https://example.com/a?b=1#c\"");
        let back: Url<'static> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.as_str(), url.as_str());
    }

    #[test]
    fn serde_invalid_url() {
        let err = serde_json::from_str::<Url<'static>>("\"not a url\"");
        assert!(err.is_err());
    }
}
