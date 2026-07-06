//! A std-only JSON reader — the reason this crate can promise zero dependencies. It covers the
//! full JSON grammar with `serde_json`'s strict semantics, because the pipeline was born on serde
//! and must keep behaving identically: escape validation including UTF-16 surrogate pairs,
//! duplicate object keys resolved last-wins, numbers that remember whether their literal was a
//! bare unsigned integer (serde's `as_u64` contract: `2` qualifies, `2.0` does not), rejection of
//! literals that overflow to infinity (`1e999`), and serde's default 128-level nesting cap so a
//! hostile input fails cleanly instead of blowing the stack.

/// The subset of a parsed JSON document this crate inspects: numbers, arrays, and object keys.
/// String *values* and booleans are validated and then discarded — [`crate::inspect`] never reads
/// them — so their variants carry no payload.
#[derive(Debug, PartialEq)]
pub(crate) enum Value {
  Null,
  Bool,
  /// A number: its `f64` reading, plus its `u64` reading when the literal was a bare unsigned
  /// integer in range (mirroring serde's `as_u64`, which is `None` for `2.0`, `-2`, and `2e0`).
  Num {
    value: f64,
    integer: Option<u64>,
  },
  Str,
  /// Array elements, in order.
  Arr(Vec<Value>),
  /// Object fields, in document order. Lookups resolve duplicate keys last-wins, as serde's
  /// insert-into-a-map parsing does.
  Obj(Vec<(String, Value)>),
}

impl Value {
  pub(crate) fn get(&self, key: &str) -> Option<&Value> {
    match self {
      Value::Obj(fields) => fields.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
      _ => None,
    }
  }

  pub(crate) fn as_u64(&self) -> Option<u64> {
    match self {
      Value::Num { integer, .. } => *integer,
      _ => None,
    }
  }

  pub(crate) fn as_f64(&self) -> Option<f64> {
    match self {
      Value::Num { value, .. } => Some(*value),
      _ => None,
    }
  }
}

/// Parse one JSON document, requiring it to span the whole input, like `serde_json::from_str`.
/// `None` is "not valid JSON": the caller maps every failure to one typed error, so the reader
/// does not manufacture error prose nobody would read.
pub(crate) fn parse(input: &str) -> Option<Value> {
  let mut p = Parser { input, pos: 0 };
  p.skip_ws();
  let value = p.value(0)?;
  p.skip_ws();
  if p.pos == p.input.len() {
    Some(value)
  } else {
    None
  }
}

/// serde_json's default recursion limit, adopted verbatim.
const MAX_DEPTH: usize = 128;

struct Parser<'a> {
  input: &'a str,
  pos: usize,
}

impl Parser<'_> {
  fn peek(&self) -> Option<u8> {
    self.input.as_bytes().get(self.pos).copied()
  }

  fn eat(&mut self, byte: u8) -> bool {
    let hit = self.peek() == Some(byte);
    if hit {
      self.pos += 1;
    }
    hit
  }

  fn literal(&mut self, text: &str) -> Option<()> {
    self.input[self.pos..].starts_with(text).then(|| self.pos += text.len())
  }

  fn skip_ws(&mut self) {
    while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
      self.pos += 1;
    }
  }

  fn value(&mut self, depth: usize) -> Option<Value> {
    if depth >= MAX_DEPTH {
      return None;
    }
    match self.peek()? {
      b'n' => self.literal("null").map(|()| Value::Null),
      b't' => self.literal("true").map(|()| Value::Bool),
      b'f' => self.literal("false").map(|()| Value::Bool),
      b'"' => self.string().map(|_| Value::Str),
      b'[' => self.array(depth),
      b'{' => self.object(depth),
      _ => self.number(),
    }
  }

  fn array(&mut self, depth: usize) -> Option<Value> {
    self.pos += 1; // The opening `[`.
    let mut items = Vec::new();
    self.skip_ws();
    if self.eat(b']') {
      return Some(Value::Arr(items));
    }
    loop {
      items.push(self.value(depth + 1)?);
      self.skip_ws();
      if self.eat(b']') {
        return Some(Value::Arr(items));
      }
      if !self.eat(b',') {
        return None;
      }
      self.skip_ws();
    }
  }

  fn object(&mut self, depth: usize) -> Option<Value> {
    self.pos += 1; // The opening `{`.
    let mut fields = Vec::new();
    self.skip_ws();
    if self.eat(b'}') {
      return Some(Value::Obj(fields));
    }
    loop {
      if self.peek()? != b'"' {
        return None;
      }
      let key = self.string()?;
      self.skip_ws();
      if !self.eat(b':') {
        return None;
      }
      self.skip_ws();
      fields.push((key, self.value(depth + 1)?));
      self.skip_ws();
      if self.eat(b'}') {
        return Some(Value::Obj(fields));
      }
      if !self.eat(b',') {
        return None;
      }
      self.skip_ws();
    }
  }

  /// Decode a string literal (the cursor is on the opening quote). The decoded text is only ever
  /// *used* for object keys, but a bad escape or a raw control character anywhere must still fail
  /// the whole document, exactly as serde's reader does.
  fn string(&mut self) -> Option<String> {
    self.pos += 1; // The opening `"`.
    let mut out = String::new();
    loop {
      let chunk = self.pos;
      // Raw UTF-8 runs verbatim; the stop bytes are all ASCII, so the slice below stays on
      // character boundaries.
      while !matches!(self.peek()?, b'"' | b'\\' | 0x00..=0x1f) {
        self.pos += 1;
      }
      out.push_str(&self.input[chunk..self.pos]);
      match self.peek()? {
        b'"' => {
          self.pos += 1;
          return Some(out);
        }
        b'\\' => {
          self.pos += 1;
          self.escape(&mut out)?;
        }
        _ => return None, // A raw control character, which JSON forbids inside strings.
      }
    }
  }

  /// One escape sequence (the cursor is on the byte after the backslash), including a UTF-16
  /// surrogate pair; a lone surrogate is an error, as it is for serde.
  fn escape(&mut self, out: &mut String) -> Option<()> {
    let b = self.peek()?;
    self.pos += 1;
    match b {
      b'"' => out.push('"'),
      b'\\' => out.push('\\'),
      b'/' => out.push('/'),
      b'b' => out.push('\u{8}'),
      b'f' => out.push('\u{c}'),
      b'n' => out.push('\n'),
      b'r' => out.push('\r'),
      b't' => out.push('\t'),
      b'u' => {
        let code = match self.hex4()? {
          hi @ 0xD800..=0xDBFF => {
            self.literal("\\u")?;
            let lo = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&lo) {
              return None;
            }
            0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
          }
          0xDC00..=0xDFFF => return None, // A lone trailing surrogate.
          scalar => scalar,
        };
        out.push(char::from_u32(code)?);
      }
      _ => return None,
    }
    Some(())
  }

  fn hex4(&mut self) -> Option<u32> {
    let hex = self.input.get(self.pos..self.pos + 4)?;
    // `from_str_radix` alone would also admit a sign (`\u+12f`), which JSON forbids.
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
      return None;
    }
    self.pos += 4;
    u32::from_str_radix(hex, 16).ok()
  }

  fn number(&mut self) -> Option<Value> {
    let start = self.pos;
    self.eat(b'-');
    // The integer part: `0` alone, or a nonzero digit followed by more (JSON forbids `01`).
    match self.peek()? {
      b'0' => self.pos += 1,
      b'1'..=b'9' => self.digits(),
      _ => return None,
    }
    let mut bare_integer = self.input.as_bytes()[start] != b'-';
    if self.eat(b'.') {
      bare_integer = false;
      if !self.peek()?.is_ascii_digit() {
        return None;
      }
      self.digits();
    }
    if matches!(self.peek(), Some(b'e' | b'E')) {
      bare_integer = false;
      self.pos += 1;
      if matches!(self.peek(), Some(b'+' | b'-')) {
        self.pos += 1;
      }
      if !self.peek()?.is_ascii_digit() {
        return None;
      }
      self.digits();
    }
    let text = &self.input[start..self.pos];
    let value: f64 = text.parse().ok()?;
    // serde_json refuses a literal whose value overflows to infinity (`1e999`).
    if !value.is_finite() {
      return None;
    }
    // A bare unsigned integer too large for `u64` falls back to its `f64` reading, like serde's.
    let integer = if bare_integer { text.parse::<u64>().ok() } else { None };
    Some(Value::Num { value, integer })
  }

  fn digits(&mut self) {
    while matches!(self.peek(), Some(b'0'..=b'9')) {
      self.pos += 1;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_the_shapes_the_pipeline_inspects() {
    let v = parse(r#" { "version": 2, "duration": 7.5, "title": "x", "live": true } "#).unwrap();
    assert_eq!(v.get("version").and_then(Value::as_u64), Some(2));
    assert_eq!(v.get("duration").and_then(Value::as_f64), Some(7.5));
    assert_eq!(v.get("title"), Some(&Value::Str));
    assert_eq!(v.get("live"), Some(&Value::Bool));
    assert_eq!(
      parse("[0.5, \"o\", \"a\"]").unwrap(),
      Value::Arr(vec![Value::Num { value: 0.5, integer: None }, Value::Str, Value::Str,])
    );
  }

  /// serde's `as_u64` contract: only a bare in-range unsigned integer literal qualifies.
  #[test]
  fn integer_readings_mirror_serde() {
    let num = |json: &str| parse(json).unwrap();
    assert_eq!(num("2").as_u64(), Some(2));
    assert_eq!(num("2.0").as_u64(), None);
    assert_eq!(num("2e0").as_u64(), None);
    assert_eq!(num("-2").as_u64(), None);
    assert_eq!(num("-2").as_f64(), Some(-2.0));
    assert_eq!(num("18446744073709551615").as_u64(), Some(u64::MAX));
    assert_eq!(num("18446744073709551616").as_u64(), None, "too big for u64: the f64 fallback");
    assert_eq!(num("18446744073709551616").as_f64(), Some(1.8446744073709552e19));
  }

  #[test]
  fn duplicate_keys_resolve_last_wins() {
    let v = parse(r#"{ "t": 1, "t": 2 }"#).unwrap();
    assert_eq!(v.get("t").and_then(Value::as_u64), Some(2));
  }

  #[test]
  fn escapes_are_validated_and_decoded() {
    // Decoded escapes are observable through object keys — the one place the text is read.
    assert!(parse("{\"\\ud83d\\udc1d\":1}").unwrap().get("\u{1f41d}").is_some(), "a surrogate pair decodes");
    assert!(parse("{\"a\\tb\\u00e9\":1}").unwrap().get("a\tb\u{e9}").is_some());
    assert_eq!(parse(r#""\ud83d""#), None, "a lone leading surrogate is an error");
    assert_eq!(parse(r#""\udc1d""#), None, "a lone trailing surrogate is an error");
    assert_eq!(parse(r#""\x41""#), None, "an unknown escape is an error");
    assert_eq!(parse(r#""\u+12f""#), None, "a signed hex escape is an error");
    assert_eq!(parse("\"raw\ncontrol\""), None, "a raw control character is an error");
  }

  #[test]
  fn rejects_what_serde_rejects() {
    for bad in ["", "hello", "{", "[1,]", "{\"a\":}", "01", "1.", "1e", "+1", "1 2", "{\"a\":1} x", "1e999", "nul"] {
      assert_eq!(parse(bad), None, "`{bad}` must not parse");
    }
    for good in ["null", "true", " 0 ", "-0.5e-2", "[]", "{}", "[[1], {\"a\": [2]}]"] {
      assert!(parse(good).is_some(), "`{good}` must parse");
    }
  }

  /// Nesting beyond serde's 128-level default fails cleanly instead of overflowing the stack.
  #[test]
  fn deep_nesting_is_capped() {
    let nest = |depth: usize| format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
    assert!(parse(&nest(100)).is_some());
    assert_eq!(parse(&nest(200)), None);
  }
}
