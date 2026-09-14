use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value, value::RawValue};

const MAX_ATTRIBUTE_DEPTH: usize = 6;
const MAX_ATTRIBUTE_STRING_BYTES: usize = 16_384;
const MAX_ATTRIBUTE_ARRAY_ITEMS: usize = 128;
const MAX_ATTRIBUTE_OBJECT_ITEMS: usize = 64;

/// Parsed attribute object. Used when callers construct or mutate attributes.
pub type AttributeMap = Map<String, Value>;

/// Ingest attributes stored as raw JSON until a caller needs structured access.
/// Empty objects stay `None` and never allocate a map.
#[derive(Clone, Debug, Default)]
pub struct Attributes(Option<Box<RawValue>>);

impl Attributes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_raw_json(json: String) -> Result<Self, serde_json::Error> {
        let starts_object = json.as_bytes().first() == Some(&b'{');
        let empty = json.as_bytes() == b"{}" || json.as_bytes() == b"{ }";
        if empty {
            return Ok(Self::new());
        }
        if !starts_object {
            return Err(de::Error::custom("attributes must be a JSON object"));
        }
        RawValue::from_string(json).map(|value| Self(Some(value)))
    }

    pub fn from_map(map: AttributeMap) -> Result<Self, serde_json::Error> {
        if map.is_empty() {
            return Ok(Self::new());
        }
        Self::from_raw_json(serde_json::to_string(&map)?)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_ref().map_or("{}", |value| value.get())
    }

    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|value| {
            let json = value.get().as_bytes();
            json == b"{}" || json == b"{ }"
        })
    }

    pub fn decoded(&self) -> Result<AttributeMap, serde_json::Error> {
        if self.is_empty() {
            return Ok(AttributeMap::new());
        }
        serde_json::from_str(self.as_str())
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.is_empty() {
            return Ok(());
        }
        let mut deserializer = serde_json::Deserializer::from_str(self.as_str());
        AttributeObjectLimits {
            depth: 0,
            max_keys: MAX_ATTRIBUTE_OBJECT_ITEMS,
        }
        .deserialize(&mut deserializer)
        .map_err(map_attribute_error)
    }
}

impl Serialize for Attributes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.0 {
            Some(value) => value.serialize(serializer),
            None => serializer.collect_map(std::iter::empty::<((), ())>()),
        }
    }
}

impl<'de> Deserialize<'de> for Attributes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        match raw.get().as_bytes() {
            b"{}" | b"{ }" => Ok(Self::new()),
            bytes if bytes.first() == Some(&b'{') => Ok(Self(Some(raw))),
            _ => Err(de::Error::custom("attributes must be a JSON object")),
        }
    }
}

fn map_attribute_error(error: serde_json::Error) -> &'static str {
    let message = error.to_string();
    if message.contains("at most 64 attributes") {
        "at most 64 attributes are allowed"
    } else if message.contains("attribute keys must be") {
        "attribute keys must be 1..64 bytes"
    } else if message.contains("JSON object") {
        "attributes must be a JSON object"
    } else {
        "attribute values exceed the allowed size or nesting limits"
    }
}

struct AttributeObjectLimits {
    depth: usize,
    max_keys: usize,
}

impl<'de> DeserializeSeed<'de> for AttributeObjectLimits {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for AttributeObjectLimits {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut count = 0_usize;
        while let Some(key) = map.next_key::<&str>()? {
            count += 1;
            if count > self.max_keys {
                return Err(de::Error::custom(if self.depth == 0 {
                    "at most 64 attributes are allowed"
                } else {
                    "attribute values exceed the allowed size or nesting limits"
                }));
            }
            if key.is_empty() || key.len() > 64 {
                return Err(de::Error::custom("attribute keys must be 1..64 bytes"));
            }
            map.next_value_seed(AttributeValueLimits { depth: self.depth })?;
        }
        Ok(())
    }
}

struct AttributeValueLimits {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for AttributeValueLimits {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        if self.depth > MAX_ATTRIBUTE_DEPTH {
            return Err(de::Error::custom(
                "attribute values exceed the allowed size or nesting limits",
            ));
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for AttributeValueLimits {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value.len() > MAX_ATTRIBUTE_STRING_BYTES {
            return Err(de::Error::custom(
                "attribute values exceed the allowed size or nesting limits",
            ));
        }
        Ok(())
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut count = 0_usize;
        while seq
            .next_element_seed(AttributeValueLimits {
                depth: self.depth + 1,
            })?
            .is_some()
        {
            count += 1;
            if count > MAX_ATTRIBUTE_ARRAY_ITEMS {
                return Err(de::Error::custom(
                    "attribute values exceed the allowed size or nesting limits",
                ));
            }
        }
        Ok(())
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        AttributeObjectLimits {
            depth: self.depth + 1,
            max_keys: MAX_ATTRIBUTE_OBJECT_ITEMS,
        }
        .visit_map(map)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{AttributeMap, Attributes};
    use crate::domain::telemetry::{EventInput, ValidateTelemetry};

    #[test]
    fn empty_attributes_deserialize_from_object() {
        let event: EventInput =
            serde_json::from_str(r#"{"name":"app_startup","attributes":{}}"#).unwrap();
        assert!(event.attributes.is_empty());
        assert_eq!(event.attributes.as_str(), "{}");
        assert_eq!(event.attributes.decoded().unwrap().len(), 0);
    }

    #[test]
    fn attributes_are_kept_as_raw_json() {
        let event: EventInput = serde_json::from_str(
            r#"{"name":"app_startup","attributes":{"channel":"stable","build":1}}"#,
        )
        .unwrap();
        assert!(!event.attributes.is_empty());
        let decoded = event.attributes.decoded().unwrap();
        assert_eq!(
            decoded.get("channel").and_then(serde_json::Value::as_str),
            Some("stable")
        );
        assert_eq!(
            decoded.get("build").and_then(serde_json::Value::as_i64),
            Some(1)
        );
        assert!(event.validate().is_ok());
    }

    #[test]
    fn attributes_array_is_rejected() {
        assert!(
            serde_json::from_str::<EventInput>(r#"{"name":"app_startup","attributes":[]}"#)
                .is_err()
        );
    }

    #[test]
    fn oversized_attribute_object_is_rejected() {
        let mut map = AttributeMap::new();
        for index in 0..65_u8 {
            map.insert(format!("k{index}"), serde_json::Value::from(index as i64));
        }
        let mut event: EventInput = serde_json::from_str(r#"{"name":"app_startup"}"#).unwrap();
        event.attributes = Attributes::from_map(map).unwrap();
        assert_eq!(
            event.validate().unwrap_err(),
            "at most 64 attributes are allowed"
        );
    }
}
