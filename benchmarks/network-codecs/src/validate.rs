//! Bounded strict binary structural validation; codecs alone accept unknown/duplicate fields.
use crate::Codec;
#[derive(Clone, Copy)]
enum Field {
    Unsigned,
    Signed,
    Bool,
    Text,
    Message(usize),
    Repeated(usize),
}
use Field::*;
const SCHEMAS: &[&[Field]] = &[
    &[
        Unsigned,
        Message(1),
        Message(2),
        Message(4),
        Message(5),
        Message(8),
        Text,
    ],
    &[Text, Text],
    &[Unsigned, Text, Text, Signed, Signed, Message(3)],
    &[Unsigned, Text, Signed, Bool],
    &[Unsigned, Text, Text, Signed, Message(5), Bool],
    &[Unsigned, Text, Text, Signed, Message(6)],
    &[Repeated(7)],
    &[Signed, Text, Bool],
    &[Unsigned, Text, Text, Signed, Signed, Unsigned],
];
pub fn check(codec: Codec, bytes: &[u8]) -> Result<(), String> {
    match codec {
        Codec::Cbor => {
            let mut decoder = minicbor::Decoder::new(bytes);
            cbor(&mut decoder, 0, 0)?;
            if decoder.position() != bytes.len() {
                return Err("trailing data".into());
            }
        }
        Codec::Protobuf => proto(bytes, 0, 0)?,
        Codec::Json => {}
    }
    Ok(())
}
fn cbor(d: &mut minicbor::Decoder<'_>, schema: usize, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err("nesting limit".into());
    }
    let fields = SCHEMAS[schema];
    let count = d
        .map()
        .map_err(|e| e.to_string())?
        .ok_or("indefinite map")?;
    if count > fields.len() as u64 {
        return Err("too many fields".into());
    }
    let mut seen = 0u32;
    for _ in 0..count {
        let tag = d.u32().map_err(|e| e.to_string())?;
        if tag == 0 || tag > fields.len() as u32 || seen & (1 << tag) != 0 {
            return Err("duplicate or unknown field".into());
        }
        seen |= 1 << tag;
        if d.datatype().map_err(|e| e.to_string())? == minicbor::data::Type::Null {
            d.null().map_err(|e| e.to_string())?;
            continue;
        }
        match fields[tag as usize - 1] {
            Unsigned => {
                d.u32().map_err(|e| e.to_string())?;
            }
            Signed => {
                d.i64().map_err(|e| e.to_string())?;
            }
            Bool => {
                d.bool().map_err(|e| e.to_string())?;
            }
            Text => {
                d.str().map_err(|e| e.to_string())?;
            }
            Message(child) => cbor(d, child, depth + 1)?,
            Repeated(child) => {
                let count = d
                    .array()
                    .map_err(|e| e.to_string())?
                    .ok_or("indefinite array")?;
                if count > 4096 {
                    return Err("item limit".into());
                }
                for _ in 0..count {
                    cbor(d, child, depth + 1)?;
                }
            }
        }
    }
    Ok(())
}
fn proto(bytes: &[u8], schema: usize, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err("nesting limit".into());
    }
    let fields = SCHEMAS[schema];
    let mut position = 0;
    let mut seen = 0u32;
    let mut items = 0;
    while position < bytes.len() {
        let key = varint(bytes, &mut position)?;
        let tag = key >> 3;
        if tag == 0 || tag > fields.len() as u64 {
            return Err("unknown field".into());
        }
        let field = fields[tag as usize - 1];
        let wire = key & 7;
        if !matches!(field, Repeated(_)) && seen & (1 << tag) != 0 {
            return Err("duplicate field".into());
        }
        seen |= 1 << tag;
        match field {
            Unsigned | Signed | Bool => {
                if wire != 0 {
                    return Err("wrong wire type".into());
                }
                let value = varint(bytes, &mut position)?;
                if matches!(field, Unsigned) && value > u32::MAX.into()
                    || matches!(field, Bool) && value > 1
                {
                    return Err("scalar overflow".into());
                }
            }
            Text | Message(_) | Repeated(_) => {
                if wire != 2 {
                    return Err("wrong wire type".into());
                }
                let length = usize::try_from(varint(bytes, &mut position)?)
                    .map_err(|_| "length overflow")?;
                let end = position
                    .checked_add(length)
                    .filter(|end| *end <= bytes.len())
                    .ok_or("truncated field")?;
                match field {
                    Message(child) => proto(&bytes[position..end], child, depth + 1)?,
                    Repeated(child) => {
                        items += 1;
                        if items > 4096 {
                            return Err("item limit".into());
                        }
                        proto(&bytes[position..end], child, depth + 1)?;
                    }
                    _ => {}
                }
                position = end;
            }
        }
    }
    Ok(())
}
fn varint(bytes: &[u8], position: &mut usize) -> Result<u64, String> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let b = *bytes.get(*position).ok_or("truncated varint")?;
        *position += 1;
        if shift == 63 && b > 1 {
            return Err("varint overflow".into());
        }
        value |= u64::from(b & 127) << shift;
        if b & 128 == 0 {
            return Ok(value);
        }
    }
    Err("varint overflow".into())
}
