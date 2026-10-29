use crate::{Error, Result, hash};

/// Form fields in the order Paynow sent them, which the response hash depends on.
///
/// Lookups ignore case, as Paynow sends both `pollurl` and `PollUrl`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields(Vec<(String, String)>);

impl Fields {
    pub fn parse(body: &str) -> Result<Self> {
        let body = body.trim();
        let body = body.strip_prefix('?').unwrap_or(body);

        body.split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                Ok((decode(key)?, decode(value)?))
            })
            .collect::<Result<_>>()
            .map(Self)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub(crate) fn text(&self, key: &str) -> String {
        self.get(key).unwrap_or_default().to_owned()
    }

    pub(crate) fn require(&self, key: &'static str) -> Result<&str> {
        self.get(key)
            .filter(|value| !value.trim().is_empty())
            .ok_or(Error::InvalidField(key))
    }

    pub(crate) fn status_is(&self, expected: &str) -> bool {
        self.get("status")
            .is_some_and(|status| status.trim().eq_ignore_ascii_case(expected))
    }

    pub(crate) fn verify(&self, key: &str) -> Result<()> {
        let candidate = self.get("hash").ok_or(Error::MissingHash)?;
        let values = self
            .0
            .iter()
            .filter(|(k, _)| !k.eq_ignore_ascii_case("hash"))
            .map(|(_, v)| v.as_str());

        if hash::verify(candidate, values, key) {
            Ok(())
        } else {
            Err(Error::HashMismatch)
        }
    }
}

/// Encodes in order, unlike encoders that sort keys and so break the hash.
pub(crate) fn encode(fields: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(fields.iter().map(|(k, v)| k.len() + v.len() + 2).sum());
    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        escape_into(&mut out, key);
        out.push('=');
        escape_into(&mut out, value);
    }
    out
}

pub(crate) fn escape_into(out: &mut String, s: &str) {
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(hash::HEX[usize::from(byte >> 4)] as char);
                out.push(hash::HEX[usize::from(byte & 0xf)] as char);
            }
        }
    }
}

fn decode(s: &str) -> Result<String> {
    if !s.bytes().any(|b| b == b'%' || b == b'+') {
        return Ok(s.to_owned());
    }

    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(byte) = bytes.next() {
        out.push(match byte {
            b'+' => b' ',
            b'%' => {
                let hi = bytes.next().and_then(hex_value);
                let lo = bytes.next().and_then(hex_value);
                match hi.zip(lo) {
                    Some((hi, lo)) => hi << 4 | lo,
                    None => return Err(Error::MalformedResponse),
                }
            }
            _ => byte,
        });
    }
    String::from_utf8(out).map_err(|_| Error::MalformedResponse)
}

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_in_order() {
        let body = encode(&[("b", "x y&z"), ("a", "£5 ~ok"), ("hash", "")]);
        assert_eq!(body, "b=x+y%26z&a=%C2%A35+~ok&hash=");

        let fields = Fields::parse(&body).unwrap();
        assert_eq!(
            fields.iter().collect::<Vec<_>>(),
            [("b", "x y&z"), ("a", "£5 ~ok"), ("hash", "")]
        );
    }

    #[test]
    fn escapes_like_go() {
        assert_eq!(
            encode(&[("k", "INV £1/~*'()!")]),
            "k=INV+%C2%A31%2F~%2A%27%28%29%21"
        );
    }

    #[test]
    fn lookups_ignore_case() {
        let fields = Fields::parse("Status=Ok&PollUrl=p").unwrap();
        assert_eq!(fields.get("status"), Some("Ok"));
        assert_eq!(fields.get("POLLURL"), Some("p"));
        assert!(fields.status_is("ok"));
        assert!(matches!(
            fields.require("browserurl"),
            Err(Error::InvalidField("browserurl"))
        ));
    }

    #[test]
    fn parse_tolerates_noise() {
        let fields = Fields::parse(" ?status=Ok&&flag\n").unwrap();
        assert_eq!(fields.get("status"), Some("Ok"));
        assert_eq!(fields.get("flag"), Some(""));
        assert!(Fields::parse("").unwrap().iter().next().is_none());
    }

    #[test]
    fn rejects_bad_escapes() {
        assert!(matches!(
            Fields::parse("a=%zz"),
            Err(Error::MalformedResponse)
        ));
        assert!(matches!(
            Fields::parse("a=%4"),
            Err(Error::MalformedResponse)
        ));
        assert!(matches!(
            Fields::parse("a=%ff"),
            Err(Error::MalformedResponse)
        ));
    }
}
