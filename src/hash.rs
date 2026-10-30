use sha2::{Digest, Sha512};

pub(crate) const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Paynow's signature: the uppercase hex SHA-512 of every value followed by the integration key.
pub(crate) fn sign<'a>(values: impl IntoIterator<Item = &'a str>, key: &str) -> String {
    let mut hasher = Sha512::new();
    for value in values {
        hasher.update(value.as_bytes());
    }
    hasher.update(key.as_bytes());

    let mut out = String::with_capacity(128);
    for byte in hasher.finalize() {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0xf)] as char);
    }
    out
}

pub(crate) fn verify<'a>(
    candidate: &str,
    values: impl IntoIterator<Item = &'a str>,
    key: &str,
) -> bool {
    let expected = sign(values, key);
    // Constant time, so a forged status update cannot probe the expected hash.
    candidate.len() == expected.len()
        && candidate
            .bytes()
            .zip(expected.bytes())
            .fold(0, |diff, (a, b)| diff | (a.to_ascii_uppercase() ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_hashes_the_values_then_the_key() {
        // The SHA-512 of "abc", a standard test vector.
        assert_eq!(
            sign(["a", "b"], "c"),
            "DDAF35A193617ABACC417349AE20413112E6FA4E89A97EA20A9EEEE64B55D39A\
             2192992A274FC1A836BA3C23A3FEEBBD454D4423643CE80E2A9AC94FA54CA49F"
        );
    }

    #[test]
    fn sign_is_uppercase_and_order_sensitive() {
        let signature = sign(["a", "b"], "k");
        assert_eq!(signature.len(), 128);
        assert_eq!(signature, signature.to_ascii_uppercase());
        assert_ne!(signature, sign(["b", "a"], "k"));
    }

    #[test]
    fn verify_ignores_case() {
        let signature = sign(["1", "2"], "key");
        assert!(verify(&signature, ["1", "2"], "key"));
        assert!(verify(&signature.to_ascii_lowercase(), ["1", "2"], "key"));
        assert!(!verify("nope", ["1", "2"], "key"));
    }
}
