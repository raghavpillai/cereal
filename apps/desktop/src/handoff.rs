//! Signing in through the browser, and asking a server how people sign in to
//! it. The app opens the server's own sign-in page; the person signs in there
//! by whichever method the server offers; the page hands back a short-lived
//! code, and the code plus the secret that only this attempt holds are
//! exchanged for a session. Everything here is plain values: no window, no
//! network.
use crate::link::{AuthCallback, ServerOrigin};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

/// What a server offers on its sign-in page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInMethods {
    pub password: bool,
    pub google: bool,
    /// The server can finish a sign-in in the browser and hand it to this
    /// app. False on a server from before it said which methods it offers.
    pub browser: bool,
    /// What the server calls itself, when it says.
    pub instance_name: Option<String>,
}

impl SignInMethods {
    /// A server that does not say: email and password, as this app has
    /// always asked for.
    pub const PASSWORD_ONLY: SignInMethods = SignInMethods {
        password: true,
        google: false,
        browser: false,
        instance_name: None,
    };

    /// The browser button's label, or `None` when there is no such button.
    pub fn browser_label(&self) -> Option<&'static str> {
        if !self.browser {
            None
        } else if self.google && !self.password {
            Some("Continue with Google")
        } else {
            Some("Continue in browser")
        }
    }

    /// The answer to `GET /api/v1/auth/methods`.
    pub fn read(status: u16, body: &[u8]) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct Wire {
            password: bool,
            google: bool,
            #[serde(default, rename = "instanceName")]
            instance_name: Option<String>,
        }
        match status {
            200 => serde_json::from_slice::<Wire>(body)
                .map(|wire| SignInMethods {
                    password: wire.password,
                    google: wire.google,
                    browser: true,
                    instance_name: wire.instance_name,
                })
                .map_err(|_| "The server returned an unexpected response.".into()),
            // An older server: it has no such page, and signs in as before.
            404 => Ok(Self::PASSWORD_ONLY),
            503 => Err("The server is temporarily unavailable. Try again shortly.".into()),
            status => Err(format!(
                "The server could not complete the request (HTTP {status})."
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffError {
    /// The answer belongs to another sign-in attempt, or to another server.
    Mismatch,
    Rejected,
    Unsupported,
    TimedOut,
    Random,
}

impl HandoffError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Mismatch => {
                "The sign-in that came back does not match the one this app started. Try again."
            }
            Self::Rejected => {
                "The server did not accept this sign-in. It may have taken too long. Try again."
            }
            Self::Unsupported => {
                "This server cannot sign in through the browser. Use your email and password."
            }
            Self::TimedOut => "Sign-in in the browser took too long and was stopped. Try again.",
            Self::Random => "A sign-in could not be started securely on this Mac.",
        }
    }
}

/// One attempt to sign in through the browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserSignIn {
    /// The server address the person chose, validated, without a trailing slash.
    server: String,
    /// Stays in the app. The server sees only its hash until the exchange.
    verifier: String,
    state: String,
}

impl BrowserSignIn {
    /// A new attempt for `server`: 32 random bytes of verifier and 16 of state.
    pub fn new(server: String) -> Result<Self, HandoffError> {
        Ok(Self::with(server, random(32)?, random(16)?))
    }

    fn with(server: String, verifier: String, state: String) -> Self {
        Self {
            server,
            verifier,
            state,
        }
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    /// `base64url(SHA-256(verifier))`, as in RFC 7636 (S256).
    pub fn challenge(&self) -> String {
        challenge_for(&self.verifier)
    }

    /// The server's sign-in page, told that an app is waiting for the result.
    pub fn login_url(&self) -> String {
        format!(
            "{}/login?app=1&challenge={}&state={}",
            self.server,
            self.challenge(),
            self.state
        )
    }

    /// The code from `cereal://auth?…`, once the answer is known to be for
    /// this attempt. The code is only ever sent to the server the person
    /// chose, whatever the answer names.
    pub fn code<'a>(&self, answer: &'a AuthCallback) -> Result<&'a str, HandoffError> {
        if !same(&answer.state, &self.state) {
            return Err(HandoffError::Mismatch);
        }
        if let Some(origin) = &answer.origin
            && ServerOrigin::of_server(&self.server).as_ref() != Some(origin)
        {
            return Err(HandoffError::Mismatch);
        }
        Ok(&answer.code)
    }

    /// Where the code is traded for a session, and what is sent there.
    pub fn exchange(&self, code: &str) -> (String, serde_json::Value) {
        (
            format!("{}/api/v1/auth/app-handoff/exchange", self.server),
            serde_json::json!({ "code": code, "verifier": self.verifier }),
        )
    }

    /// The session in the answer to the exchange.
    pub fn read_exchange(status: u16, body: &[u8]) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct Wire {
            token: String,
        }
        match status {
            200..=299 => serde_json::from_slice::<Wire>(body)
                .ok()
                .map(|wire| wire.token)
                .filter(|token| {
                    !token.is_empty()
                        && token.len() < 8192
                        && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
                })
                .ok_or_else(|| "The server did not return a signed session".into()),
            400 | 401 => Err(HandoffError::Rejected.message().into()),
            404 => Err(HandoffError::Unsupported.message().into()),
            429 => Err("Too many attempts, try again in a minute".into()),
            503 => Err("The server is temporarily unavailable. Try again shortly.".into()),
            status => Err(format!(
                "The server could not complete the request (HTTP {status})."
            )),
        }
    }
}

fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes()))
}

/// `count` bytes from the system's generator, as base64url without padding.
fn random(count: usize) -> Result<String, HandoffError> {
    use std::io::Read;
    let mut bytes = vec![0u8; count];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|_| HandoffError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Compares every byte, so the time taken says nothing about where two
/// values differ.
fn same(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

/// SHA-256 (FIPS 180-4), fed in pieces: a sign-in hashes its verifier, and an
/// update hashes the archive it downloaded as it reads it from disk.
pub struct Sha256 {
    state: [u32; 8],
    /// Bytes that do not yet make a whole 64-byte block.
    pending: Vec<u8>,
    length: u64,
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                0x1f83d9ab, 0x5be0cd19,
            ],
            pending: Vec::with_capacity(64),
            length: 0,
        }
    }

    pub fn update(&mut self, mut bytes: &[u8]) {
        self.length = self.length.wrapping_add(bytes.len() as u64);
        if !self.pending.is_empty() {
            let take = (64 - self.pending.len()).min(bytes.len());
            self.pending.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.pending.len() < 64 {
                return;
            }
            let block = std::mem::take(&mut self.pending);
            self.compress(&block);
        }
        let mut blocks = bytes.chunks_exact(64);
        for block in &mut blocks {
            self.compress(block);
        }
        self.pending.extend_from_slice(blocks.remainder());
    }

    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        let mut tail = std::mem::take(&mut self.pending);
        tail.push(0x80);
        while tail.len() % 64 != 56 {
            tail.push(0);
        }
        tail.extend_from_slice(&bits.to_be_bytes());
        for block in tail.chunks_exact(64) {
            self.compress(block);
        }
        let mut digest = [0u8; 32];
        for (index, value) in self.state.iter().enumerate() {
            digest[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        digest
    }

    fn compress(&mut self, block: &[u8]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for (index, word) in block.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let first = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let second = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(first);
            d = c;
            c = b;
            b = a;
            a = first.wrapping_add(second);
        }
        for (value, add) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *value = value.wrapping_add(add);
        }
    }
}

fn sha256(message: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(message);
    hash.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn sha256_fed_in_pieces_matches_the_whole() {
        let message: Vec<u8> = (0..10_000u32).map(|value| (value % 251) as u8).collect();
        for piece in [1usize, 7, 63, 64, 65, 1000] {
            let mut hash = Sha256::new();
            for part in message.chunks(piece) {
                hash.update(part);
            }
            assert_eq!(hash.finish(), sha256(&message), "pieces of {piece}");
        }
    }

    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // A message that ends exactly where the padding has to spill into a
        // second block, and a long one.
        assert_eq!(
            hex(&sha256(&[b'a'; 55])),
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
        );
        assert_eq!(
            hex(&sha256(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn the_challenge_is_the_rfc_7636_s256_example() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn each_attempt_has_its_own_verifier_and_state_of_the_right_size() {
        let first = BrowserSignIn::new("https://notes.example.com".into()).unwrap();
        let second = BrowserSignIn::new("https://notes.example.com".into()).unwrap();
        // 32 and 16 bytes as unpadded base64url.
        assert_eq!(first.verifier.len(), 43);
        assert_eq!(first.state.len(), 22);
        assert_eq!(first.challenge().len(), 43);
        for value in [&first.verifier, &first.state] {
            assert!(value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        }
        assert_ne!(first.verifier, second.verifier);
        assert_ne!(first.state, second.state);
        assert_ne!(first.verifier, first.challenge());
    }

    #[test]
    fn the_sign_in_page_gets_the_challenge_and_state_but_never_the_verifier() {
        let attempt = BrowserSignIn::with(
            "http://127.0.0.1:3391".into(),
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into(),
            "c3RhdGUtc3RhdGUtc3RhdA".into(),
        );
        assert_eq!(
            attempt.login_url(),
            "http://127.0.0.1:3391/login?app=1&challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&state=c3RhdGUtc3RhdGUtc3RhdA"
        );
        assert!(!attempt.login_url().contains(&attempt.verifier));
        let (url, body) = attempt.exchange("the-code");
        assert_eq!(url, "http://127.0.0.1:3391/api/v1/auth/app-handoff/exchange");
        assert_eq!(
            body,
            serde_json::json!({"code": "the-code", "verifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"})
        );
    }

    #[test]
    fn only_the_answer_to_this_attempt_on_this_server_yields_a_code() {
        let attempt = BrowserSignIn::with(
            "https://notes.example.com".into(),
            "verifier".into(),
            "expected-state".into(),
        );
        let answer = |state: &str, server: Option<&str>| AuthCallback {
            code: "the-code".into(),
            state: state.into(),
            origin: server.map(|server| ServerOrigin::parse(server).unwrap()),
        };
        assert_eq!(attempt.code(&answer("expected-state", None)), Ok("the-code"));
        assert_eq!(
            attempt.code(&answer("expected-state", Some("https://notes.example.com"))),
            Ok("the-code")
        );
        assert_eq!(
            attempt.code(&answer("expected-state", Some("https://NOTES.example.com:443/"))),
            Ok("the-code")
        );
        for (state, server) in [
            ("another-state", None),
            ("expected-stat", None),
            ("expected-state-and-more", None),
            ("", None),
            ("expected-state", Some("https://evil.example")),
            ("expected-state", Some("https://notes.example.com:8443")),
            ("expected-state", Some("http://localhost")),
        ] {
            assert_eq!(
                attempt.code(&answer(state, server)),
                Err(HandoffError::Mismatch),
                "{state} {server:?}"
            );
        }
        // A server on a private network, over plain HTTP: the answer may
        // name that origin, and only that one.
        let private = BrowserSignIn::with("http://10.30.1.77:3000".into(), "v".into(), "s".into());
        assert_eq!(private.code(&answer("s", Some("http://10.30.1.77:3000"))), Ok("the-code"));
        assert_eq!(private.code(&answer("s", None)), Ok("the-code"));
        assert!(private.login_url().starts_with("http://10.30.1.77:3000/login?app=1&challenge="));
        assert_eq!(private.exchange("c").0, "http://10.30.1.77:3000/api/v1/auth/app-handoff/exchange");
        for server in [
            "https://10.30.1.77:3000",
            "http://10.30.1.77",
            "http://10.30.1.77:3001",
            "http://10.30.1.78:3000",
            "http://localhost:3000",
        ] {
            assert_eq!(
                private.code(&answer("s", Some(server))),
                Err(HandoffError::Mismatch),
                "{server}"
            );
        }
        // A server kept under a path is still named by its origin.
        let nested = BrowserSignIn::with("https://notes.example.com/base".into(), "v".into(), "s".into());
        assert_eq!(nested.code(&answer("s", Some("https://notes.example.com"))), Ok("the-code"));
        assert_eq!(
            nested.login_url(),
            format!("https://notes.example.com/base/login?app=1&challenge={}&state=s", nested.challenge())
        );
    }

    #[test]
    fn the_exchange_answer_is_a_printable_session_or_a_plain_reason() {
        assert_eq!(
            BrowserSignIn::read_exchange(200, br#"{"token":"abc.def-123"}"#),
            Ok("abc.def-123".into())
        );
        for body in [
            &br#"{"token":""}"#[..],
            br#"{"token":"two words"}"#,
            br#"{"token":"line\nbreak"}"#,
            br#"{"token":5}"#,
            br#"{}"#,
            b"<html>",
        ] {
            assert_eq!(
                BrowserSignIn::read_exchange(200, body),
                Err("The server did not return a signed session".into())
            );
        }
        assert_eq!(
            BrowserSignIn::read_exchange(401, br#"{"error":"invalid_code"}"#),
            Err(HandoffError::Rejected.message().into())
        );
        assert_eq!(
            BrowserSignIn::read_exchange(400, br#"{"error":"invalid_request"}"#),
            Err(HandoffError::Rejected.message().into())
        );
        assert_eq!(
            BrowserSignIn::read_exchange(404, b""),
            Err(HandoffError::Unsupported.message().into())
        );
        assert_eq!(
            BrowserSignIn::read_exchange(429, br#"{"error":"rate_limited"}"#),
            Err("Too many attempts, try again in a minute".into())
        );
        assert!(BrowserSignIn::read_exchange(500, b"").unwrap_err().contains("HTTP 500"));
    }

    #[test]
    fn sign_in_methods_decide_the_form_and_the_browser_button() {
        let read = |body: &str| SignInMethods::read(200, body.as_bytes()).unwrap();
        let password = read(r#"{"password":true,"google":false,"instanceName":"Cereal"}"#);
        assert!(password.password && !password.google);
        assert_eq!(password.browser_label(), Some("Continue in browser"));
        let google = read(r#"{"password":false,"google":true,"instanceName":"Cereal"}"#);
        assert!(!google.password);
        assert_eq!(google.browser_label(), Some("Continue with Google"));
        let both = read(r#"{"password":true,"google":true}"#);
        assert!(both.password);
        assert_eq!(both.browser_label(), Some("Continue in browser"));
        // An older server answers 404: the form as it has always been, and
        // no browser button.
        let older = SignInMethods::read(404, b"Not found").unwrap();
        assert_eq!(older, SignInMethods::PASSWORD_ONLY);
        assert!(older.password);
        assert_eq!(older.browser_label(), None);
        assert!(SignInMethods::read(200, b"<html>").is_err());
        assert!(SignInMethods::read(200, br#"{"password":"yes","google":false}"#).is_err());
        assert!(SignInMethods::read(503, b"").is_err());
        assert!(SignInMethods::read(500, b"").unwrap_err().contains("HTTP 500"));
    }
}
