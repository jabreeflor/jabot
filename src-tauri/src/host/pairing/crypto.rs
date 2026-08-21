//! The primitives the pairing handshake needs, on top of the SHA-256 the
//! OAuth flow already hand-rolled (`host/tools/crypto.rs`).
//!
//! Same reasoning as there: the host's dependency set is deliberately small,
//! HMAC-SHA256 is a fixed and fully specified function, and the tests below
//! pin it to the RFC 4231 vectors — so a wrong MAC fails here rather than as
//! an unpairable phone in someone's kitchen.
//!
//! Two things in this file are load-bearing beyond "it hashes".
//!
//! **Fields are length-prefixed before they are hashed.** A transcript is a
//! list of strings from both sides, and plain concatenation lets an attacker
//! who controls one field move a boundary — `deviceId = "a", nonce = "bc"`
//! and `deviceId = "ab", nonce = "c"` would otherwise hash identically, and
//! the safety number the two humans compare would agree on two different
//! pairings. [`transcript_hash`] therefore frames every field.
//!
//! **Comparisons of secrets are constant-time.** [`ct_eq`] exists so that a
//! wrong MAC — or a guessed typed code — cannot be walked byte by byte from
//! how long the host took to say no.

use super::super::tools::crypto::{base64url, sha256};

const BLOCK: usize = 64;

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut padded = [0u8; BLOCK];
    if key.len() > BLOCK {
        padded[..32].copy_from_slice(&sha256(key));
    } else {
        padded[..key.len()].copy_from_slice(key);
    }

    let mut inner = Vec::with_capacity(BLOCK + message.len());
    inner.extend(padded.iter().map(|byte| byte ^ 0x36));
    inner.extend_from_slice(message);
    let inner_digest = sha256(&inner);

    let mut outer = Vec::with_capacity(BLOCK + inner_digest.len());
    outer.extend(padded.iter().map(|byte| byte ^ 0x5c));
    outer.extend_from_slice(&inner_digest);
    sha256(&outer)
}

/// Compare without leaking where the difference is.
///
/// Length is compared first and in the clear: the length of a MAC or a code is
/// public, and a loop that ran for the shorter of two lengths would leak the
/// contents instead.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Hash a list of fields such that no field can absorb another's bytes.
///
/// Each field is written as a 4-byte big-endian length followed by its bytes,
/// so the encoding is injective over the list.
pub fn transcript_hash(fields: &[&str]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(fields.iter().map(|f| f.len() + 4).sum::<usize>());
    for field in fields {
        let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(field.as_bytes());
    }
    sha256(&buf)
}

/// A public, comparable name for key material that is never itself sent.
///
/// This is a commitment, not a verifying key — there is no signature scheme
/// here (see the module docs on `pairing/mod.rs`). What it buys is that a host
/// or device can publish a stable identifier for its long-term key without
/// publishing the key, that the identifier changes if the key is regenerated
/// (a reinstall, which `pairing-security-mobile.md` says must scream), and
/// that both identifiers can be folded into the safety number.
pub fn fingerprint(domain: &str, key_material: &str) -> String {
    base64url(&transcript_hash(&[domain, key_material]))
}

/// The safety number both humans read out loud: eight decimal digits.
///
/// `pairing-security-mobile.md` asks for a 6–8 digit number, Signal-style.
/// Eight digits is 1 in 100 million that a man in the middle running two
/// separate handshakes gets both humans to see the same string — and unlike a
/// Signal safety number it is per-pairing and lives for the length of one
/// offer, so there is no offline grinding window worth the name.
pub fn sas_digits(mac: &[u8; 32]) -> String {
    let mut value = 0u64;
    for byte in mac.iter().take(8) {
        value = (value << 8) | u64::from(*byte);
    }
    let digits = value % 100_000_000;
    format!("{:04}-{:04}", digits / 10_000, digits % 10_000)
}

/// Crockford base32 — no I, L, O or U, so nothing reads as another character
/// over the phone. `pairing-security-mobile.md` picks it for the headless
/// host that prints a code instead of drawing a QR.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// `len` Crockford characters of unguessable text.
///
/// Each source byte selects one character by `% 32`. 256 is exactly eight
/// times 32, so the reduction is uniform rather than merely close.
pub fn crockford(bytes: &[u8], len: usize) -> String {
    bytes
        .iter()
        .cycle()
        .take(len)
        .map(|byte| CROCKFORD[usize::from(*byte) % 32] as char)
        .collect()
}

/// Fold the ways a human types a code back onto the alphabet, or refuse.
///
/// Case, spaces and dashes are noise. `I`/`L` are `1` and `O` is `0`, which is
/// the whole point of Crockford: someone reading a code off a terminal to
/// someone holding a phone should not be able to get it wrong.
pub fn normalize_code(input: &str) -> Option<String> {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch == '-' || ch == ' ' || ch == '\t' {
            continue;
        }
        let upper = ch.to_ascii_uppercase();
        let mapped = match upper {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        };
        if !CROCKFORD.contains(&(mapped as u8)) {
            return None;
        }
        out.push(mapped);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Hex, for MACs on the wire. Shorter than base64url to eyeball in a log and
/// unambiguous to re-parse on a client written in another language.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4231 HMAC-SHA256 vectors. If these pass, this is HMAC.
    #[test]
    fn hmac_matches_the_published_vectors() {
        let cases: [(&[u8], &[u8], &str); 3] = [
            (
                &[0x0b; 20],
                b"Hi There",
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                b"Jefe",
                b"what do ya want for nothing?",
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                &[0xaa; 20],
                &[0xdd; 50],
                "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
            ),
        ];
        for (key, message, expected) in cases {
            assert_eq!(hex(&hmac_sha256(key, message)), expected);
        }
    }

    /// RFC 4231 case 4: a key longer than the 64-byte block, which the
    /// implementation must hash down rather than truncate.
    #[test]
    fn hmac_hashes_an_oversized_key() {
        let mac = hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First",
        );
        assert_eq!(
            hex(&mac),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    /// The reason fields are framed: two different field lists must not be
    /// able to produce one hash by moving a boundary.
    #[test]
    fn transcript_framing_defeats_a_shifted_boundary() {
        assert_ne!(
            transcript_hash(&["a", "bc"]),
            transcript_hash(&["ab", "c"]),
            "concatenation without length prefixes would make these equal"
        );
        // Order still matters, and equal lists still agree.
        assert_ne!(transcript_hash(&["a", "b"]), transcript_hash(&["b", "a"]));
        assert_eq!(transcript_hash(&["a", "b"]), transcript_hash(&["a", "b"]));
    }

    #[test]
    fn ct_eq_is_equality() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn sas_is_eight_digits_and_moves_with_its_input() {
        let a = sas_digits(&hmac_sha256(b"secret", b"one"));
        let b = sas_digits(&hmac_sha256(b"secret", b"two"));
        assert_ne!(a, b);
        for sas in [&a, &b] {
            assert_eq!(sas.len(), 9, "NNNN-NNNN");
            assert_eq!(sas.as_bytes()[4], b'-');
            assert!(sas.chars().filter(char::is_ascii_digit).count() == 8);
        }
    }

    #[test]
    fn codes_read_back_the_way_a_human_says_them() {
        assert_eq!(normalize_code("abc-def12").as_deref(), Some("ABCDEF12"));
        // The characters Crockford removes, mapped rather than rejected.
        assert_eq!(normalize_code("iLo").as_deref(), Some("110"));
        // A character that is not in the alphabet at all is a typo, not a code.
        assert_eq!(normalize_code("ab!c"), None);
        assert_eq!(normalize_code("  "), None);
    }

    #[test]
    fn crockford_stays_in_the_alphabet() {
        let code = crockford(&[0, 1, 31, 32, 255, 128], 6);
        assert_eq!(code.len(), 6);
        assert!(code.bytes().all(|b| CROCKFORD.contains(&b)));
        assert_eq!(&code[..3], "01Z");
    }
}
