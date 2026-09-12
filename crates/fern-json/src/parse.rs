//! Native-compatible byte positions, resource accounting and Unicode decoding.
use super::*;
struct Parser<'s, 'b, 'l> {
    text: &'s [u8],
    at: usize,
    budget: &'b mut Budget<'l>,
}
/// Parse one bounded document with native byte offsets, returning an immutable root or the first failure.
pub fn document(text: &str, limits: &mut Limits) -> Result<Json> {
    document_bytes(text.as_bytes(), limits)
}
/// Parse native bytes without losing malformed-Unicode byte positions.
pub fn document_bytes(text: &[u8], limits: &mut Limits) -> Result<Json> {
    if text.len() > INPUT {
        return Err(error(4, INPUT as i64));
    }
    let mut budget = Budget::new(limits, text.len())?;
    document_bytes_in(text, &mut budget)
}
/// Parse under an existing codec allowance without resetting budgets.
pub fn document_in(text: &str, budget: &mut Budget<'_>) -> Result<Json> {
    document_bytes_in(text.as_bytes(), budget)
}
/// Parse native bytes under an existing codec allowance.
pub fn document_bytes_in(text: &[u8], budget: &mut Budget<'_>) -> Result<Json> {
    if text.len() > INPUT {
        return Err(error(4, INPUT as i64));
    }
    let mut parser = Parser {
        text,
        at: usize::from(text.starts_with(b"\xef\xbb\xbf")) * 3,
        budget,
    };
    let value = parser.value(1)?;
    parser.space();
    if parser.at != text.len() {
        return Err(parser.fail(1));
    }
    Ok(value)
}
/// Validate a builder number token under its supplied budget; reject any unconsumed suffix.
pub fn number(text: &str, mut budget: Budget<'_>) -> Result<Json> {
    let mut parser = Parser {
        text: text.as_bytes(),
        at: 0,
        budget: &mut budget,
    };
    if !matches!(parser.peek(), Some(b'-' | b'0'..=b'9')) {
        return Err(parser.fail(1));
    }
    let value = parser.number()?;
    if parser.at != text.len() {
        return Err(parser.fail(1));
    }
    Ok(value)
}
impl Parser<'_, '_, '_> {
    /// Read the current byte without advancing; None denotes the exact end of input.
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }
    /// Create a syntax/profile failure at the current original-input byte position.
    fn fail(&self, code: u8) -> Error {
        error(code, self.at as i64)
    }
    /// Copy the current input offset into the budget so subsequent resource errors retain native locations.
    fn sync(&mut self) {
        self.budget.at = self.at;
    }
    /// Advance over exactly JSON whitespace bytes; each iteration consumes one bounded input byte.
    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }
    /// Charge a new node at the current input position and return that position for immutable metadata.
    fn node(&mut self) -> Result<usize> {
        self.sync();
        self.budget.node()?;
        Ok(self.at)
    }
    /// Dispatch one value after whitespace, work and depth checks; never return a partial subtree.
    fn value(&mut self, depth: usize) -> Result<Json> {
        self.space();
        self.sync();
        self.budget.work(1)?;
        if depth > DEPTH {
            return Err(self.fail(4));
        }
        match self.peek() {
            Some(b'[' | b'{') => self.container(depth),
            Some(b'"') => self.string(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b'n') => self.literal(b"null", Kind::Null),
            Some(b't') => self.literal(b"true", Kind::Bool(true)),
            Some(b'f') => self.literal(b"false", Kind::Bool(false)),
            _ => Err(self.fail(1)),
        }
    }
    /// Require the complete literal spelling before allocating its supplied semantic kind.
    fn literal(&mut self, text: &[u8], kind: Kind) -> Result<Json> {
        for (i, byte) in text.iter().enumerate() {
            if self.text.get(self.at + i) != Some(byte) {
                return Err(error(1, (self.at + i) as i64));
            }
        }
        let offset = self.node()?;
        self.at += text.len();
        Ok(Rc::new(Node {
            kind,
            offset,
            height: 1,
            nodes: 1,
            encoded: text.len(),
        }))
    }
    /// Consume a nonempty ASCII digit run, otherwise report syntax at the first missing digit.
    fn digits(&mut self) -> Result<()> {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        if self.at == start {
            Err(self.fail(1))
        } else {
            Ok(())
        }
    }
    /// Consume a strict JSON number token, charging copied lexeme storage before allocation.
    fn number(&mut self) -> Result<Json> {
        let offset = self.node()?;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        if self.peek() == Some(b'0') {
            self.at += 1;
        } else {
            self.digits()?;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits()?;
        }
        self.sync();
        self.budget.allocate(self.at - offset + 1)?;
        let text = std::str::from_utf8(&self.text[offset..self.at])
            .map_err(|_| self.fail(1))?
            .to_owned();
        Ok(Rc::new(Node {
            encoded: text.len(),
            kind: Kind::Number(text),
            offset,
            height: 1,
            nodes: 1,
        }))
    }
    /// Decode a quoted string in two bounded passes, allocating only the measured decoded length.
    fn string(&mut self) -> Result<Json> {
        let offset = self.node()?;
        self.at += 1;
        let start = self.at;
        let mut length = 0;
        while self.peek() != Some(b'"') {
            if self.peek().is_none() {
                return Err(self.fail(1));
            }
            length += self.unit()?.len_utf8();
        }
        let end = self.at;
        self.at += 1;
        self.sync();
        self.budget.allocate(length + 1)?;
        let mut text = String::with_capacity(length);
        self.at = start;
        while self.at < end {
            text.push(self.unit()?);
        }
        self.at = end + 1;
        Ok(value::text_node(text, offset))
    }
    /// Decode one string unit with charged work, preserving Unicode or syntax error positions.
    fn unit(&mut self) -> Result<char> {
        self.sync();
        self.budget.work(1)?;
        let at = self.at;
        let byte = self.peek().ok_or_else(|| self.fail(1))?;
        if byte < 32 {
            return Err(self.fail(1));
        }
        if byte == b'\\' {
            self.at += 1;
            return self.escape(at);
        }
        let width = match byte {
            0..=127 => 1,
            194..=223 => 2,
            224..=239 => 3,
            240..=244 => 4,
            _ => 0,
        };
        let bytes = self.text.get(at..at + width).ok_or_else(|| self.fail(2))?;
        let scalar = std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| s.chars().next())
            .ok_or_else(|| self.fail(2))?;
        self.at += width;
        Ok(scalar)
    }
    /// Consume exactly four hexadecimal bytes and return their scalar value or the first syntax failure.
    fn hex(&mut self) -> Result<u32> {
        let mut scalar = 0;
        for _ in 0..4 {
            let byte = self.peek().ok_or_else(|| self.fail(1))?;
            let digit = (byte as char).to_digit(16).ok_or_else(|| self.fail(1))?;
            self.at += 1;
            scalar = scalar * 16 + digit;
        }
        Ok(scalar)
    }
    /// Decode the escape after a backslash at `at`; preserve that position for malformed surrogate errors.
    fn escape(&mut self, at: usize) -> Result<char> {
        let byte = self.peek().ok_or_else(|| self.fail(1))?;
        self.at += 1;
        match byte {
            b'"' | b'\\' | b'/' => Ok(byte as char),
            b'b' => Ok('\u{8}'),
            b'f' => Ok('\u{c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => self.unicode(at),
            _ => Err(error(1, (self.at - 1) as i64)),
        }
    }
    /// Decode one Unicode escape or paired surrogates into a scalar, rejecting unpaired surrogate values.
    fn unicode(&mut self, at: usize) -> Result<char> {
        let mut scalar = self.hex()?;
        if (0xd800..=0xdbff).contains(&scalar) {
            if self.text.len() - self.at < 6 || self.text.get(self.at..self.at + 2) != Some(b"\\u")
            {
                return Err(error(2, at as i64));
            }
            self.at += 2;
            let low = self.hex()?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err(error(2, at as i64));
            }
            scalar = 0x10000 + (scalar - 0xd800) * 1024 + low - 0xdc00;
        }
        char::from_u32(scalar).ok_or_else(|| error(2, at as i64))
    }
    /// Append a child after charging geometric storage/copy growth; `capacity` tracks native logical buffers.
    fn append(&mut self, values: &mut Vec<Json>, capacity: &mut usize, child: Json) -> Result<()> {
        self.sync();
        if values.len() == *capacity {
            let next = if *capacity == 0 { 4 } else { *capacity * 2 };
            self.budget.allocate(next * 8)?;
            self.budget.work(values.len())?;
            values.reserve_exact(next - values.len());
            *capacity = next;
        }
        values.push(child);
        Ok(())
    }
    /// Parse array/object children at bounded depth, then seal metadata and reject malformed separators.
    fn container(&mut self, depth: usize) -> Result<Json> {
        let offset = self.node()?;
        let object = self.peek() == Some(b'{');
        let closing = if object { b'}' } else { b']' };
        self.at += 1;
        self.space();
        let mut children = Vec::new();
        let mut capacity = 0;
        if self.peek() == Some(closing) {
            self.at += 1;
            return value::seal(children, object, offset, self.budget);
        }
        for _ in 0..NODES {
            if object {
                if self.peek() != Some(b'"') {
                    return Err(self.fail(1));
                }
                let key = self.string()?;
                self.append(&mut children, &mut capacity, key)?;
                self.space();
                if self.peek() != Some(b':') {
                    return Err(self.fail(1));
                }
                self.at += 1;
            }
            let child = self.value(depth + 1)?;
            self.append(&mut children, &mut capacity, child)?;
            self.space();
            let separator = self.peek().ok_or_else(|| self.fail(1))?;
            self.at += 1;
            if separator == closing {
                self.sync();
                return value::seal(children, object, offset, self.budget);
            }
            if separator != b',' {
                return Err(error(1, (self.at - 1) as i64));
            }
            self.space();
        }
        Err(self.fail(4))
    }
}
