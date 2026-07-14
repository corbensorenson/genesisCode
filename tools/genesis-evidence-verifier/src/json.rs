use serde::Deserialize;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueValueVisitor)
    }
}

struct UniqueValueVisitor;

impl<'de> Visitor<'de> for UniqueValueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON with unique object keys and integer-only numbers")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Err(E::custom("floating-point JSON is forbidden"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        UniqueValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueValue>()? {
            values.push(value.0);
        }
        Ok(UniqueValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
            }
            let value = map.next_value_seed(UniqueValueSeed)?;
            values.insert(key, value.0);
        }
        Ok(UniqueValue(Value::Object(values)))
    }
}

struct UniqueValueSeed;

impl<'de> DeserializeSeed<'de> for UniqueValueSeed {
    type Value = UniqueValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        UniqueValue::deserialize(deserializer)
    }
}

pub(crate) fn parse_unique(bytes: &[u8], label: &str) -> Result<Value, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value =
        UniqueValue::deserialize(&mut deserializer).map_err(|error| format!("{label}: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("{label}: trailing JSON data: {error}"))?;
    Ok(value.0)
}

pub(crate) fn canonical_bytes(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| format!("canonical JSON encoding failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{canonical_bytes, parse_unique};

    #[test]
    fn rejects_duplicate_keys_and_floats() {
        assert!(parse_unique(br#"{"a":1,"a":2}"#, "fixture").is_err());
        assert!(parse_unique(br#"{"a":1.5}"#, "fixture").is_err());
    }

    #[test]
    fn canonicalizes_object_keys() {
        let value = parse_unique(br#"{"z":1,"a":2}"#, "fixture").expect("valid fixture");
        let bytes = canonical_bytes(&value).expect("canonical fixture");
        assert_eq!(bytes, br#"{"a":2,"z":1}"#);
    }
}
