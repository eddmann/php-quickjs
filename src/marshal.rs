//! Value marshaling between three worlds via a neutral [`MiddleValue`]:
//!
//! ```text
//!   JS Value  <->  MiddleValue  <->  PHP Zval
//!                       |
//!                  msgpack bytes  (the `__host` wire format)
//! ```
//!
//! `MiddleValue` (de)serializes to **native** msgpack types (nil/bool/int/
//! float/str/bin/array/map) — not serde's tagged-enum form — so a JS-side
//! msgpack codec interoperates with it byte-for-byte.

use crate::bridge::BridgeState;
use crate::callback::JsCallback;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::types::{ArrayKey, ZendClassObject, ZendHashTable, Zval};
use rquickjs::{Array, Ctx, Function, Object, TypedArray, Value};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::fmt;

/// Reserved msgpack-map keys tagging a function reference across the wire.
const JSFN_TAG: &str = "$__jsfn";
const PHPFN_TAG: &str = "$__phpfn";

/// The neutral, self-describing value that bridges JS, PHP and the wire.
#[derive(Debug, Clone, PartialEq)]
pub enum MiddleValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<MiddleValue>),
    /// Insertion-ordered string-keyed map (matches JS object + PHP assoc array).
    Map(Vec<(String, MiddleValue)>),
    /// A PHP callable handed to JS (id into the host-side registry).
    PhpFn(u64),
    /// A JS function handed to PHP (id into the JS-side registry).
    JsFn(u64),
}

impl MiddleValue {
    /// Encode to a msgpack byte payload (the `__host` wire form).
    pub fn to_msgpack(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec(self)
    }

    /// Decode a msgpack byte payload.
    pub fn from_msgpack(bytes: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(bytes)
    }
}

/// Map an `f64` to an int when it is integral and fits an `i64`, else keep it
/// a float. QuickJS already stores small integral numbers as int32, so this
/// only ever promotes the larger integral doubles that JS cannot tag as int.
fn int_or_float(f: f64) -> MiddleValue {
    if f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
        MiddleValue::Int(f as i64)
    } else {
        MiddleValue::Float(f)
    }
}

// ---------------------------------------------------------------------------
// native-msgpack serde
// ---------------------------------------------------------------------------

impl Serialize for MiddleValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            MiddleValue::Null => s.serialize_unit(),
            MiddleValue::Bool(b) => s.serialize_bool(*b),
            MiddleValue::Int(i) => s.serialize_i64(*i),
            MiddleValue::Float(f) => s.serialize_f64(*f),
            MiddleValue::Str(v) => s.serialize_str(v),
            MiddleValue::Bytes(b) => s.serialize_bytes(b),
            MiddleValue::Array(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for it in items {
                    seq.serialize_element(it)?;
                }
                seq.end()
            }
            MiddleValue::Map(entries) => {
                let mut map = s.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
            // Function refs travel as single-entry tagged maps.
            MiddleValue::PhpFn(id) => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry(PHPFN_TAG, &(*id as i64))?;
                map.end()
            }
            MiddleValue::JsFn(id) => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry(JSFN_TAG, &(*id as i64))?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for MiddleValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = MiddleValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a msgpack value")
            }
            fn visit_unit<E>(self) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Null)
            }
            fn visit_none<E>(self) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Null)
            }
            fn visit_bool<E>(self, v: bool) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Bool(v))
            }
            fn visit_i64<E>(self, v: i64) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Int(v))
            }
            fn visit_u64<E>(self, v: u64) -> Result<MiddleValue, E> {
                Ok(i64::try_from(v).map_or(MiddleValue::Float(v as f64), MiddleValue::Int))
            }
            fn visit_f64<E>(self, v: f64) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Float(v))
            }
            fn visit_str<E>(self, v: &str) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Str(v.to_owned()))
            }
            fn visit_string<E>(self, v: String) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Str(v))
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Bytes(v.to_owned()))
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<MiddleValue, E> {
                Ok(MiddleValue::Bytes(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<MiddleValue, A::Error> {
                let mut out = Vec::new();
                while let Some(it) = seq.next_element()? {
                    out.push(it);
                }
                Ok(MiddleValue::Array(out))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<MiddleValue, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = map.next_entry::<MapKey, MiddleValue>()? {
                    out.push((k.0, v));
                }
                // A single-entry map keyed by a reserved tag is a function ref.
                if out.len() == 1 {
                    if let (key, MiddleValue::Int(id)) = &out[0] {
                        if key == JSFN_TAG {
                            return Ok(MiddleValue::JsFn(*id as u64));
                        }
                        if key == PHPFN_TAG {
                            return Ok(MiddleValue::PhpFn(*id as u64));
                        }
                    }
                }
                Ok(MiddleValue::Map(out))
            }
        }
        d.deserialize_any(V)
    }
}

/// A map key coerced to a string (msgpack maps may key by non-string scalars).
struct MapKey(String);
impl<'de> Deserialize<'de> for MapKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct K;
        impl<'de> Visitor<'de> for K {
            type Value = MapKey;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map key")
            }
            fn visit_str<E>(self, v: &str) -> Result<MapKey, E> {
                Ok(MapKey(v.to_owned()))
            }
            fn visit_string<E>(self, v: String) -> Result<MapKey, E> {
                Ok(MapKey(v))
            }
            fn visit_i64<E>(self, v: i64) -> Result<MapKey, E> {
                Ok(MapKey(v.to_string()))
            }
            fn visit_u64<E>(self, v: u64) -> Result<MapKey, E> {
                Ok(MapKey(v.to_string()))
            }
        }
        d.deserialize_any(K)
    }
}

// ---------------------------------------------------------------------------
// JS <-> MiddleValue
// ---------------------------------------------------------------------------

/// Convert a JS value into the neutral representation. Functions are registered
/// in the JS-side registry and travel as a [`MiddleValue::JsFn`] ref.
pub fn js_to_middle<'js>(
    ctx: &Ctx<'js>,
    value: Value<'js>,
    _state: &BridgeState,
) -> rquickjs::Result<MiddleValue> {
    if value.is_null() || value.is_undefined() {
        return Ok(MiddleValue::Null);
    }
    if let Some(b) = value.as_bool() {
        return Ok(MiddleValue::Bool(b));
    }
    if value.is_int() {
        return Ok(MiddleValue::Int(value.as_int().unwrap() as i64));
    }
    if value.is_float() {
        return Ok(int_or_float(value.as_float().unwrap()));
    }
    if let Some(s) = value.as_string() {
        return Ok(MiddleValue::Str(s.to_string()?));
    }
    if value.is_function() {
        // Register the function JS-side; PHP receives an opaque id.
        let register: Function = ctx.globals().get("__registerJsFn")?;
        let id: f64 = register.call((value.clone(),))?;
        return Ok(MiddleValue::JsFn(id as u64));
    }
    // Uint8Array -> Bytes (checked before the generic object branch).
    if value.is_object() {
        if let Ok(ta) = TypedArray::<u8>::from_value(value.clone()) {
            if let Some(bytes) = ta.as_bytes() {
                return Ok(MiddleValue::Bytes(bytes.to_vec()));
            }
        }
    }
    if value.is_array() {
        let arr = value.into_array().unwrap();
        let mut out = Vec::with_capacity(arr.len());
        for i in 0..arr.len() {
            out.push(js_to_middle(ctx, arr.get(i)?, _state)?);
        }
        return Ok(MiddleValue::Array(out));
    }
    if value.is_object() {
        let obj = value.into_object().unwrap();
        let mut out = Vec::new();
        for entry in obj.props::<String, Value>() {
            let (k, v) = entry?;
            out.push((k, js_to_middle(ctx, v, _state)?));
        }
        return Ok(MiddleValue::Map(out));
    }
    Err(rquickjs::Exception::throw_type(
        ctx,
        "unsupported JS value type",
    ))
}

/// Convert the neutral representation into a JS value.
pub fn middle_to_js<'js>(
    ctx: &Ctx<'js>,
    value: &MiddleValue,
    _state: &BridgeState,
) -> rquickjs::Result<Value<'js>> {
    Ok(match value {
        MiddleValue::Null => Value::new_null(ctx.clone()),
        MiddleValue::Bool(b) => Value::new_bool(ctx.clone(), *b),
        MiddleValue::Int(i) => {
            if let Ok(i32v) = i32::try_from(*i) {
                Value::new_int(ctx.clone(), i32v)
            } else {
                // Beyond i32: represent as a JS number (exact up to 2^53).
                Value::new_float(ctx.clone(), *i as f64)
            }
        }
        MiddleValue::Float(f) => Value::new_float(ctx.clone(), *f),
        MiddleValue::Str(s) => rquickjs::String::from_str(ctx.clone(), s)?.into_value(),
        MiddleValue::Bytes(b) => TypedArray::new(ctx.clone(), b.clone())?.into_value(),
        MiddleValue::Array(items) => {
            let arr = Array::new(ctx.clone())?;
            for (i, it) in items.iter().enumerate() {
                arr.set(i, middle_to_js(ctx, it, _state)?)?;
            }
            arr.into_value()
        }
        MiddleValue::Map(entries) => {
            let obj = Object::new(ctx.clone())?;
            for (k, v) in entries {
                obj.set(k.as_str(), middle_to_js(ctx, v, _state)?)?;
            }
            obj.into_value()
        }
        // Reconstruct callables from the JS-side helpers.
        MiddleValue::JsFn(id) => {
            let get: Function = ctx.globals().get("__getJsFn")?;
            get.call((*id as f64,))?
        }
        MiddleValue::PhpFn(id) => {
            let make: Function = ctx.globals().get("__makePhpFn")?;
            make.call((*id as f64,))?
        }
    })
}

// ---------------------------------------------------------------------------
// PHP Zval <-> MiddleValue
// ---------------------------------------------------------------------------

/// Convert a PHP value into the neutral representation. A `Js\Callback` becomes
/// a [`MiddleValue::JsFn`] ref; any other PHP callable is registered host-side
/// as a [`MiddleValue::PhpFn`].
pub fn zval_to_middle(zv: &Zval, state: &BridgeState) -> Result<MiddleValue, String> {
    if zv.is_null() {
        return Ok(MiddleValue::Null);
    }
    if zv.is_bool() {
        return Ok(MiddleValue::Bool(zv.bool().unwrap_or(false)));
    }
    if zv.is_long() {
        return Ok(MiddleValue::Int(zv.long().unwrap()));
    }
    if zv.is_double() {
        return Ok(MiddleValue::Float(zv.double().unwrap()));
    }
    if zv.is_string() {
        // PHP strings are byte strings. Preserve valid UTF-8 as a string;
        // anything else (binary data) crosses as bytes -> JS Uint8Array.
        let bytes = zv.zend_str().map(|zs| zs.as_bytes()).unwrap_or(&[]);
        return Ok(match std::str::from_utf8(bytes) {
            Ok(s) => MiddleValue::Str(s.to_owned()),
            Err(_) => MiddleValue::Bytes(bytes.to_owned()),
        });
    }
    if zv.is_array() {
        let ht = zv.array().unwrap();
        return hashtable_to_middle(ht, state);
    }
    // A returned Js\Callback maps back to its original JS function.
    if zv.is_object() {
        if let Some(cb) = zv.extract::<&ZendClassObject<JsCallback>>() {
            return Ok(MiddleValue::JsFn(cb.id));
        }
    }
    // Any other callable (closure, [obj, 'method'] is caught above as array) is
    // registered host-side and handed to JS as a callable wrapper.
    if zv.is_callable() {
        return Ok(MiddleValue::PhpFn(state.register_php_fn(zv)));
    }
    Err("unsupported PHP value type for marshaling".to_owned())
}

/// A PHP array becomes an [`MiddleValue::Array`] when its keys are the
/// sequential `0..n`, otherwise an insertion-ordered [`MiddleValue::Map`].
fn hashtable_to_middle(ht: &ZendHashTable, state: &BridgeState) -> Result<MiddleValue, String> {
    if ht.has_sequential_keys() {
        let mut out = Vec::with_capacity(ht.len());
        for (_, v) in ht.iter() {
            out.push(zval_to_middle(v, state)?);
        }
        Ok(MiddleValue::Array(out))
    } else {
        let mut out = Vec::with_capacity(ht.len());
        for (k, v) in ht.iter() {
            let key = match k {
                ArrayKey::Long(i) => i.to_string(),
                ArrayKey::String(s) => s,
                ArrayKey::Str(s) => s.to_owned(),
                ArrayKey::ZendString(s) => s.try_into().unwrap_or_default(),
            };
            out.push((key, zval_to_middle(v, state)?));
        }
        Ok(MiddleValue::Map(out))
    }
}

/// Convert the neutral representation into a PHP value.
pub fn middle_to_zval(value: &MiddleValue, state: &BridgeState) -> Result<Zval, String> {
    let mut zv = Zval::new();
    match value {
        MiddleValue::Null => zv.set_null(),
        MiddleValue::Bool(b) => zv.set_bool(*b),
        MiddleValue::Int(i) => zv.set_long(*i),
        MiddleValue::Float(f) => zv.set_double(*f),
        MiddleValue::Str(s) => zv
            .set_string(s, false)
            .map_err(|e| format!("string conversion failed: {e}"))?,
        MiddleValue::Bytes(b) => zv.set_binary(b.clone()),
        MiddleValue::Array(items) => {
            let mut ht = ZendHashTable::new();
            for it in items {
                ht.push(middle_to_zval(it, state)?)
                    .map_err(|e| format!("array push failed: {e}"))?;
            }
            zv.set_hashtable(ht);
        }
        MiddleValue::Map(entries) => {
            let mut ht = ZendHashTable::new();
            for (k, v) in entries {
                ht.insert(k.as_str(), middle_to_zval(v, state)?)
                    .map_err(|e| format!("map insert failed: {e}"))?;
            }
            zv.set_hashtable(ht);
        }
        // A PHP callable handed to JS and returned unchanged: original callable.
        MiddleValue::PhpFn(id) => {
            return state
                .get_php_fn(*id)
                .ok_or_else(|| format!("unknown PHP callable id {id}"));
        }
        // A JS function handed to PHP: an invocable Js\Callback object.
        MiddleValue::JsFn(id) => {
            let engine = state
                .engine()
                .ok_or("engine no longer available for JS callback")?;
            let cb = JsCallback::new(*id, engine);
            return ZendClassObject::new(cb)
                .into_zval(false)
                .map_err(|e| format!("failed to build Js\\Callback: {e}"));
        }
    }
    Ok(zv)
}

/// Helper so callers can build a Zval from any `IntoZval` (used by tests).
#[allow(dead_code)]
pub fn into_zval<T: IntoZval>(v: T) -> Result<Zval, String> {
    v.into_zval(false).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: MiddleValue) {
        let bytes = v.to_msgpack().expect("encode");
        let back = MiddleValue::from_msgpack(&bytes).expect("decode");
        assert_eq!(v, back);
    }

    #[test]
    fn msgpack_scalars() {
        roundtrip(MiddleValue::Null);
        roundtrip(MiddleValue::Bool(true));
        roundtrip(MiddleValue::Int(-42));
        roundtrip(MiddleValue::Int(1 << 40));
        roundtrip(MiddleValue::Float(3.5));
        roundtrip(MiddleValue::Str("héllo".to_owned()));
        roundtrip(MiddleValue::Bytes(vec![0, 1, 2, 255]));
    }

    #[test]
    fn msgpack_nested() {
        roundtrip(MiddleValue::Array(vec![
            MiddleValue::Int(1),
            MiddleValue::Str("two".into()),
            MiddleValue::Bool(false),
        ]));
        roundtrip(MiddleValue::Map(vec![
            ("a".into(), MiddleValue::Int(1)),
            (
                "nested".into(),
                MiddleValue::Array(vec![MiddleValue::Null, MiddleValue::Float(2.5)]),
            ),
        ]));
    }

    #[test]
    fn bytes_encode_as_msgpack_bin() {
        // msgpack bin8 marker is 0xc4; ensure bytes do not serialize as an array.
        let bytes = MiddleValue::Bytes(vec![1, 2, 3]).to_msgpack().unwrap();
        assert_eq!(bytes[0], 0xc4);
    }
}
