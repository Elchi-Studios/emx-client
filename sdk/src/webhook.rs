//! Checking what a webhook received.
//!
//! Every request from the service carries `X-EMX-Signature:
//! t=<unix seconds>,v1=<hex>`, where `v1` is HMAC-SHA256 with the
//! webhook's secret over `<unix seconds>.<raw body>`. [`verify`] checks it
//! and refuses a timestamp older than five minutes, so a captured request
//! cannot be replayed later. HMAC and SHA-256 are written here so that a
//! program which only receives webhooks needs no more of this crate than
//! this module.

use crate::types::WebhookEvent;

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The header is missing or not in the `t=...,v1=...` form.
    Malformed,
    /// The timestamp is more than `tolerance` seconds from `now`.
    Stale,
    /// The signature does not match the body and the secret.
    BadSignature,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refused::Malformed => "the signature header is malformed",
            Refused::Stale => "the signature is too old",
            Refused::BadSignature => "the signature does not match",
        })
    }
}

impl std::error::Error for Refused {}

/// Checks a request against the secret, with a five-minute tolerance.
pub fn verify(secret: &str, signature_header: &str, body: &[u8]) -> Result<(), Refused> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    verify_at(secret, signature_header, body, now, 300)
}

/// [`verify`] with the clock and the tolerance given, for tests and for
/// programs that replay recorded requests.
pub fn verify_at(
    secret: &str,
    signature_header: &str,
    body: &[u8],
    now: i64,
    tolerance: i64,
) -> Result<(), Refused> {
    let mut ts: Option<i64> = None;
    let mut sig: Option<Vec<u8>> = None;
    for part in signature_header.split(',') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("t=") {
            ts = v.parse().ok();
        } else if let Some(v) = part.strip_prefix("v1=") {
            sig = unhex(v);
        }
    }
    let (ts, sig) = match (ts, sig) {
        (Some(t), Some(s)) if s.len() == 32 => (t, s),
        _ => return Err(Refused::Malformed),
    };
    if (now - ts).abs() > tolerance {
        return Err(Refused::Stale);
    }
    let mut signed = ts.to_string().into_bytes();
    signed.push(b'.');
    signed.extend_from_slice(body);
    let want = hmac_sha256(secret.as_bytes(), &signed);
    if constant_time_eq(&want, &sig) {
        Ok(())
    } else {
        Err(Refused::BadSignature)
    }
}

/// Verifies and decodes in one step.
pub fn receive(
    secret: &str,
    signature_header: &str,
    body: &[u8],
) -> Result<WebhookEvent, Box<dyn std::error::Error>> {
    verify(secret, signature_header, body)?;
    Ok(serde_json::from_slice(body)?)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

/// HMAC-SHA256, RFC 2104.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + msg.len());
    let mut outer = Vec::with_capacity(96);
    for b in &k {
        inner.push(b ^ 0x36);
        outer.push(b ^ 0x5c);
    }
    inner.extend_from_slice(msg);
    outer.extend_from_slice(&sha256(&inner));
    sha256(&outer)
}

/// SHA-256, FIPS 180-4.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha256_known_answers() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            hex(&sha256(long.as_bytes())),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&sha256(&million)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn hmac_known_answers() {
        // RFC 4231, test case 2.
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: a key longer than a block.
        let key = vec![0xaa; 131];
        assert_eq!(
            hex(&hmac_sha256(
                &key,
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn verifies_and_refuses() {
        let secret = "whsec_test";
        let body = br#"{"id":"evt_1","type":"ping","createdAt":"2026-09-24T10:00:00Z","account":{"id":"a"},"data":{}}"#;
        let ts = 1_758_708_000i64;
        let mut signed = format!("{ts}.").into_bytes();
        signed.extend_from_slice(body);
        let sig = hex(&hmac_sha256(secret.as_bytes(), &signed));
        let header = format!("t={ts},v1={sig}");
        assert_eq!(verify_at(secret, &header, body, ts + 10, 300), Ok(()));
        assert_eq!(
            verify_at(secret, &header, body, ts + 600, 300),
            Err(Refused::Stale)
        );
        assert_eq!(
            verify_at("other", &header, body, ts, 300),
            Err(Refused::BadSignature)
        );
        assert_eq!(
            verify_at(secret, &header, b"changed", ts, 300),
            Err(Refused::BadSignature)
        );
        assert_eq!(verify_at(secret, "v1=zz", body, ts, 300), Err(Refused::Malformed));
        assert_eq!(verify_at(secret, "", body, ts, 300), Err(Refused::Malformed));
    }
}
