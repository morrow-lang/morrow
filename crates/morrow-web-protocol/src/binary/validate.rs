//! Allocation-free strict field scan before prost allocates owned messages.
use super::{MAX_IDENTITY_BYTES, MAX_TASKS};
use crate::{Error, MAX_FRAME_BYTES, MAX_LABEL_BYTES};

#[derive(Clone, Copy)]
enum Field {
    Unsigned,
    Signed,
    Bool,
    Text(usize),
    Message(usize),
    Repeated(usize),
}
use Field::*;
pub(super) const ENVELOPE: usize = 0;
pub(super) const COMMAND: usize = 2;
const ID: Field = Text(MAX_IDENTITY_BYTES);
const LABEL: Field = Text(MAX_LABEL_BYTES);
const SCHEMAS: &[&[Field]] = &[
    &[
        Unsigned,
        Message(1),
        Message(2),
        Message(4),
        Message(5),
        Message(8),
        ID,
    ],
    &[ID, ID],
    &[Unsigned, ID, ID, Signed, Signed, Message(3)],
    &[Unsigned, LABEL, Signed, Bool],
    &[Unsigned, ID, ID, Signed, Message(5), Bool],
    &[Unsigned, ID, ID, Signed, Message(6)],
    &[Repeated(7)],
    &[Signed, LABEL, Bool],
    &[Unsigned, ID, ID, Signed, Signed, Unsigned],
];

pub(super) fn check(bytes: &[u8], schema: usize) -> Result<(), Error> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(Error::FrameTooLarge);
    }
    scan(bytes, schema, 0)
}
fn scan(bytes: &[u8], schema: usize, depth: usize) -> Result<(), Error> {
    if depth > 16 {
        return Err(Error::Malformed);
    }
    let fields = SCHEMAS[schema];
    let mut position = 0;
    let mut seen = 0u32;
    let mut items = 0;
    while position < bytes.len() {
        let key = varint(bytes, &mut position)?;
        let tag = key >> 3;
        if tag == 0 || tag > fields.len() as u64 {
            return Err(Error::Malformed);
        }
        let field = fields[tag as usize - 1];
        let wire = key & 7;
        if !matches!(field, Repeated(_)) && seen & (1 << tag) != 0 {
            return Err(Error::Malformed);
        }
        seen |= 1 << tag;
        match field {
            Unsigned | Signed | Bool => {
                if wire != 0 {
                    return Err(Error::Malformed);
                }
                let value = varint(bytes, &mut position)?;
                if matches!(field, Unsigned) && value > u32::MAX.into()
                    || matches!(field, Bool) && value > 1
                {
                    return Err(Error::Malformed);
                }
            }
            Text(_) | Message(_) | Repeated(_) => {
                if wire != 2 {
                    return Err(Error::Malformed);
                }
                let length =
                    usize::try_from(varint(bytes, &mut position)?).map_err(|_| Error::Malformed)?;
                let end = position
                    .checked_add(length)
                    .filter(|end| *end <= bytes.len())
                    .ok_or(Error::Malformed)?;
                match field {
                    Text(limit) => {
                        if length > limit {
                            return Err(Error::Malformed);
                        }
                        // prost performs UTF-8 validation while decoding the bounded string.
                    }
                    Message(child) => scan(&bytes[position..end], child, depth + 1)?,
                    Repeated(child) => {
                        items += 1;
                        if items > MAX_TASKS {
                            return Err(Error::Malformed);
                        }
                        scan(&bytes[position..end], child, depth + 1)?;
                    }
                    _ => unreachable!(),
                }
                position = end;
            }
        }
    }
    Ok(())
}
fn varint(bytes: &[u8], position: &mut usize) -> Result<u64, Error> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*position).ok_or(Error::Malformed)?;
        *position += 1;
        if shift == 63 && byte > 1 {
            return Err(Error::Malformed);
        }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    Err(Error::Malformed)
}
