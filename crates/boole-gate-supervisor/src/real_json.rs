//! Ordered, duplicate-free scalar JSON for the real contract only.
use anyhow::{ensure, Result};
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::fmt;
#[derive(Debug)]
pub enum Json {
    Null,
    Bool(bool),
    UInt(u64),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}
impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Json;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("strict scalar JSON")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Json, E> {
                Ok(Json::Bool(v))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Json, E> {
                Ok(Json::UInt(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Json, E> {
                u64::try_from(v).map(Json::UInt).map_err(E::custom)
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Json, E> {
                Ok(Json::Text(v.into()))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Json, A::Error> {
                let mut v = vec![];
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(Json::Array(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Json, A::Error> {
                let mut v: Vec<(String, Json)> = vec![];
                let mut seen = std::collections::BTreeSet::new();
                while let Some((k, x)) = a.next_entry::<String, Json>()? {
                    if !seen.insert(k.clone()) {
                        return Err(serde::de::Error::custom("duplicate key"));
                    }
                    v.push((k, x));
                }
                Ok(Json::Object(v))
            }
        }
        d.deserialize_any(V)
    }
}
impl Serialize for Json {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Null => s.serialize_unit(),
            Self::Bool(v) => s.serialize_bool(*v),
            Self::UInt(v) => s.serialize_u64(*v),
            Self::Text(v) => s.serialize_str(v),
            Self::Array(v) => {
                let mut a = s.serialize_seq(Some(v.len()))?;
                for x in v {
                    a.serialize_element(x)?;
                }
                a.end()
            }
            Self::Object(v) => {
                let mut a = s.serialize_map(Some(v.len()))?;
                for (k, x) in v {
                    a.serialize_entry(k, x)?;
                }
                a.end()
            }
        }
    }
}
pub fn parse(raw: &[u8], keys: &[&str]) -> Result<serde_json::Value> {
    let j: Json = serde_json::from_slice(raw)?;
    let Json::Object(fields) = &j else {
        anyhow::bail!("JSON object")
    };
    ensure!(
        fields
            .iter()
            .map(|(k, _)| k.as_str())
            .eq(keys.iter().copied()),
        "ordered fields"
    );
    ensure!(crate::real_descriptor::ascii(&j)? == raw, "canonical JSON");
    Ok(serde_json::from_slice(raw)?)
}
pub fn target(v: &serde_json::Value) -> Result<()> {
    let a = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("structured target"))?;
    ensure!(!a.is_empty() && a.len() <= 256, "target length");
    for p in a {
        let p = p
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("target component"))?;
        ensure!(p.len() == 2, "target component length");
        ensure!(
            (p[0] == "str" && p[1].as_str().is_some_and(|s| !s.is_empty()))
                || (p[0] == "num" && p[1].as_u64().is_some()),
            "target component type"
        );
    }
    Ok(())
}
