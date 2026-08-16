//! The shared secret for two-factor enrolment.
//!
//! This client never computes a one-time code - the authenticator app does
//! that, which is the whole point of the arrangement. All that is needed here
//! is a secret to hand over, so there is no HMAC and no clock arithmetic.
//!
//! The secret is a credential, but unlike a password it exists to be shown:
//! enrolment cannot happen unless it reaches the authenticator app. So it is
//! deliberately *not* a `Secret` - masking it would make the feature
//! impossible - and the panels display it only while enrolment is open.

use rand::RngCore;
use rand::rngs::OsRng;

/// RFC 4648 base32, which is what authenticator apps read.
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// 20 bytes, the length RFC 4226 recommends for an HMAC-SHA1 secret and what
/// every authenticator app expects.
const SECRET_BYTES: usize = 20;

/// A freshly generated enrolment secret.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TotpSecret(String);

impl TotpSecret {
    /// Generate one from the operating system's randomness.
    ///
    /// `OsRng` rather than a thread RNG: this is key material, and a
    /// deterministic or seedable source would make every enrolment guessable.
    pub fn generate() -> Self {
        let mut bytes = [0_u8; SECRET_BYTES];
        OsRng.fill_bytes(&mut bytes);
        Self(base32_encode(&bytes))
    }

    /// The base32 secret, for typing into an authenticator app by hand.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The same secret in groups of four, which is how it is read aloud and
    /// typed. Ungrouped, a 32-character string is easy to lose your place in.
    pub fn grouped(&self) -> String {
        self.0
            .as_bytes()
            .chunks(4)
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The `otpauth://` URI an authenticator app scans.
    ///
    /// The account name is percent-encoded: Discord usernames may contain
    /// characters that would otherwise end the path or start the query, and a
    /// URI broken that way produces an enrolment that silently does not match.
    pub fn otpauth_uri(&self, account: &str) -> String {
        format!(
            "otpauth://totp/Discord:{}?secret={}&issuer=Discord",
            percent_encode(account),
            self.0
        )
    }

    /// Rebuild from a secret already generated, for a panel that is redrawn
    /// while enrolment is open.
    pub fn from_base32(value: &str) -> Self {
        Self(value.to_owned())
    }
}

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    // Five bytes make exactly eight base32 characters, so whole chunks need no
    // padding. Twenty bytes is four such chunks, which is why no padding case
    // appears here - and the assertion in the tests keeps that true.
    for chunk in bytes.chunks(5) {
        let mut buffer = [0_u8; 5];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let value = u64::from(buffer[0]) << 32
            | u64::from(buffer[1]) << 24
            | u64::from(buffer[2]) << 16
            | u64::from(buffer[3]) << 8
            | u64::from(buffer[4]);

        // Ceiling division: a partial chunk still needs a character for its
        // last few bits, and dropping it would lose them.
        let characters = (chunk.len() * 8).div_ceil(5);
        for index in 0..characters {
            let shift = 35 - index * 5;
            let digit = ((value >> shift) & 0x1f) as usize;
            out.push(ALPHABET[digit] as char);
        }
    }
    out
}

/// Percent-encode everything that is not unreserved.
///
/// Deliberately strict: the allow-list is RFC 3986's unreserved set, so a
/// character nobody thought about is encoded rather than passed through.
fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_the_length_authenticator_apps_expect() {
        // 20 bytes is 32 base32 characters with nothing left over, which is
        // why the encoder needs no padding case.
        let secret = TotpSecret::generate();

        assert_eq!(secret.as_str().len(), 32);
        assert_eq!(SECRET_BYTES % 5, 0, "a partial chunk would need padding");
    }

    #[test]
    fn a_secret_uses_only_the_base32_alphabet() {
        // A character outside it is silently rejected by authenticator apps,
        // producing an enrolment that fails with no explanation.
        for _ in 0..32 {
            let secret = TotpSecret::generate();
            assert!(
                secret.as_str().bytes().all(|byte| ALPHABET.contains(&byte)),
                "{} is not base32",
                secret.as_str()
            );
        }
    }

    #[test]
    fn two_secrets_differ() {
        // The failure this guards against is a fixed or zeroed secret, which
        // would look completely normal and be identical for every account.
        let first = TotpSecret::generate();
        let second = TotpSecret::generate();

        assert_ne!(first, second);
        assert!(
            first.as_str().bytes().any(|byte| byte != b'A'),
            "an all-zero secret encodes as all A"
        );
    }

    #[test]
    fn base32_matches_the_rfc_test_vectors() {
        // Hand-rolled encoders are exactly the thing to check against someone
        // else's answers rather than against themselves.
        assert_eq!(base32_encode(b"f"), "MY");
        assert_eq!(base32_encode(b"fo"), "MZXQ");
        assert_eq!(base32_encode(b"foo"), "MZXW6");
        assert_eq!(base32_encode(b"foob"), "MZXW6YQ");
        assert_eq!(base32_encode(b"fooba"), "MZXW6YTB");
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn an_account_name_cannot_break_out_of_the_uri() {
        // A username containing ? or # would otherwise end the path and start
        // a query, producing an enrolment that silently does not match.
        let secret = TotpSecret::from_base32("ABCD");
        let uri = secret.otpauth_uri("some?one#two");

        assert!(uri.contains("some%3Fone%23two"));
        assert_eq!(uri.matches('?').count(), 1, "the query was broken");
        assert!(!uri.contains("#two"));
        assert!(uri.ends_with("&issuer=Discord"));
    }

    #[test]
    fn an_ordinary_name_is_left_readable() {
        // Encoding everything would make the QR label unreadable for the
        // common case, which is a plain username.
        let uri = TotpSecret::from_base32("ABCD").otpauth_uri("someone");
        assert!(uri.contains("Discord:someone?"));
    }

    #[test]
    fn grouping_does_not_change_the_secret() {
        let secret = TotpSecret::generate();
        assert_eq!(secret.grouped().replace(' ', ""), secret.as_str());
        assert!(secret.grouped().contains(' '), "not grouped at all");
    }
}
