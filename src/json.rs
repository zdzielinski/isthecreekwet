//! JSON helpers that reproduce the original Python implementation's output.

use std::io;

use serde_json::Value;

/// Python-style truthiness for a JSON value.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Round to 2 decimal places like Python's round(x, 2).
pub fn round2(x: f64) -> f64 {
    // A formatted f64 always parses back, so the fallback is never used.
    format!("{x:.2}").parse().unwrap_or(x)
}

/// Serializes like Python's json.dumps defaults: ", " and ": " separators, non-ASCII escaped.
pub struct PythonJson;

impl serde_json::ser::Formatter for PythonJson {
    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            w.write_all(b", ")
        }
    }
    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            w.write_all(b", ")
        }
    }
    fn begin_object_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }
    fn write_string_fragment<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        for c in fragment.chars() {
            if (' '..='~').contains(&c) {
                w.write_all(&[c as u8])?;
            } else {
                for unit in c.encode_utf16(&mut [0; 2]) {
                    write!(w, "\\u{unit:04x}")?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_truthy_matches_python() {
        assert!(!truthy(None));
        assert!(!truthy(Some(&json!(null))));
        assert!(!truthy(Some(&json!(0))));
        assert!(!truthy(Some(&json!(""))));
        assert!(!truthy(Some(&json!([]))));
        assert!(!truthy(Some(&json!({}))));
        assert!(truthy(Some(&json!(true))));
        assert!(truthy(Some(&json!(0.5))));
        assert!(truthy(Some(&json!("x"))));
    }

    #[test]
    #[allow(clippy::float_cmp)] // round2 must return exactly these decimal values, like Python's round()
    fn test_round2_rounds_to_two_places() {
        assert_eq!(round2(0.126), 0.13);
        assert_eq!(round2(1.0 / 3.0), 0.33);
        assert_eq!(round2(0.0), 0.0);
    }

    #[test]
    fn test_python_json_separators_and_escapes() -> Result<(), serde_json::Error> {
        let mut out = Vec::new();
        let value = json!({"a": [1, 2], "b": "°F"});
        serde::Serialize::serialize(
            &value,
            &mut serde_json::Serializer::with_formatter(&mut out, PythonJson),
        )?;
        assert_eq!(
            String::from_utf8_lossy(&out),
            r#"{"a": [1, 2], "b": "\u00b0F"}"#
        );
        Ok(())
    }
}
