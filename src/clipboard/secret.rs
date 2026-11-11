//! Local secret heuristic for the history previews (v0.7 Phase 4, spec 3.6).
//!
//! Rows that look like credentials are not hidden or blocked: their preview is
//! drawn as `••••••••` until the row is hovered or focused. Nothing leaves the
//! machine and nothing is remembered about the decision — it is a pure function
//! over the stored text.
//!
//! The rules are deliberately narrow, because a false positive only hides a
//! preview behind a hover while a false negative shows a token in plain sight.
//! Paths, URLs and addresses are explicitly not secrets: they are the strings
//! most likely to be long, mixed and full of digits.

/// Credential prefixes issued by well-known services. Each one only counts
/// when the whole token is long enough to actually carry a value.
const PREFIXES: [&str; 22] = [
    "sk-",
    "sk_live_",
    "sk_test_",
    "pk_live_",
    "pk_test_",
    "rk_live_",
    "rk_test_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxs-",
    "AKIA",
    "ASIA",
    "AIza",
    "ya29.",
    "npm_",
];

/// Key names that only ever appear on the left of `key=value` when the value
/// is a credential.
const SECRET_KEYS: [&str; 12] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "client_secret",
    "auth",
    "bearer",
];

/// Shortest run the randomness rule will consider.
const RANDOM_MIN_LEN: usize = 28;
/// Digits a generated value is expected to carry; prose identifiers rarely do.
const RANDOM_MIN_DIGITS: usize = 5;
/// Shortest value that follows `Bearer` / `key: value` and still counts as the
/// credential itself rather than a word ("password: enabled" is not a secret).
const SECRET_VALUE_MIN_LEN: usize = 8;

/// Whether a history row's preview should be masked (spec 3.6).
pub fn looks_secret(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
        return true;
    }
    let mut expect_value = false;
    for token in text.split_whitespace() {
        if expect_value {
            // `Bearer <token>` / `password: <token>`: whatever follows is the
            // credential itself, so no prefix is needed.
            if token.len() >= SECRET_VALUE_MIN_LEN {
                return true;
            }
            expect_value = false;
            continue;
        }
        if token.eq_ignore_ascii_case("bearer") || token.eq_ignore_ascii_case("authorization:") {
            expect_value = true;
            continue;
        }
        // `key: value` split across whitespace.
        if let Some(key) = token.strip_suffix(':') {
            if is_secret_key(key) {
                expect_value = true;
                continue;
            }
        }
        if looks_like_token(token) {
            return true;
        }
    }
    false
}

fn looks_like_token(token: &str) -> bool {
    if PREFIXES.iter().any(|prefix| token.starts_with(prefix)) {
        return token.len() >= 20;
    }
    if let Some(value) = assigned_secret(token) {
        return value.len() >= 6;
    }
    looks_random(token)
}

fn is_secret_key(key: &str) -> bool {
    let key = key
        .trim()
        .trim_start_matches(['"', '\''])
        .to_ascii_lowercase();
    SECRET_KEYS.iter().any(|candidate| key.ends_with(candidate))
}

/// The value side of a `key=value` pair whose key names a credential.
fn assigned_secret(token: &str) -> Option<&str> {
    let (key, value) = token.split_once('=')?;
    if !is_secret_key(key) {
        return None;
    }
    let value = value.trim().trim_end_matches(['"', '\'']);
    (!value.is_empty()).then_some(value)
}

/// A whitespace-free run that mixes character classes and carries enough
/// digits to be generated rather than written.
fn looks_random(token: &str) -> bool {
    if token.len() < RANDOM_MIN_LEN || token.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    // A path, a URL, an address or a file name is long and busy but never a
    // credential.
    if token.contains('\\') || token.contains("://") || (token.contains('@') && token.contains('.'))
    {
        return false;
    }
    if let Some((_, ext)) = token.rsplit_once('.') {
        if !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return false;
        }
    }
    let (mut lower, mut upper, mut digit, mut symbol) = (0usize, 0usize, 0usize, 0usize);
    for c in token.chars() {
        if c.is_lowercase() {
            lower += 1;
        } else if c.is_uppercase() {
            upper += 1;
        } else if c.is_numeric() {
            digit += 1;
        } else {
            symbol += 1;
        }
    }
    let classes = [lower > 0, upper > 0, digit > 0, symbol > 0]
        .iter()
        .filter(|present| **present)
        .count();
    classes >= 3 && digit >= RANDOM_MIN_DIGITS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_token_shapes_are_masked() {
        // Provider prefixes. The Stripe-shaped value stays under GitHub push
        // protection's 24-character key body (a longer literal is blocked as a
        // real credential); 21 characters still clears our own 20-char rule.
        assert!(looks_secret("sk_live_4eC39HqLyjWDa"));
        assert!(looks_secret("ghp_94Xv9VpQn8bJm2sT6uW0yZ1aC3dE5fG7hI9j"));
        assert!(looks_secret("xoxb-24601-897654321234-abcdef123456"));
        assert!(looks_secret("AKIAIOSFODNN7EXAMPLE"));
        assert!(looks_secret("AIzaSyD-1234567890abcdefgHIJKLMNOPQRSTU"));
        // JWT.
        assert!(looks_secret(
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJVadQssw5c"
        ));
        // Private key header.
        assert!(looks_secret(
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQ..."
        ));
        // Bearer style.
        assert!(looks_secret("Bearer dGhpcyBpcyBhIGxvbmcgYmFzZTY0IHRva2Vu"));
        // Key/value pairs.
        assert!(looks_secret("api_key=Ab12Cd34Ef56Gh78"));
        assert!(looks_secret("client_secret: s3cr3tVALUE"));
        assert!(looks_secret("MY_DATABASE_PASSWORD=hunter22"));
    }

    #[test]
    fn generated_values_are_masked() {
        // A UUID is the classic long random string.
        assert!(looks_secret("550e8400-e29b-41d4-a716-446655440000"));
        // Mixed case, digits and symbols, long enough to be generated.
        assert!(looks_secret("aB3kZ9pQ7rT2vX6mN4wY8sD1fG5hJ0lK"));
    }

    #[test]
    fn ordinary_text_is_not_masked() {
        assert!(!looks_secret(""));
        assert!(!looks_secret("   "));
        assert!(!looks_secret(
            "hello world, this is a perfectly normal sentence"
        ));
        assert!(!looks_secret("correct horse battery staple"));
        // Long, mixed and full of digits — but it is prose, not a credential.
        assert!(!looks_secret("TaskbarIslandClipboardV07Release"));
        assert!(!looks_secret("Invoice2026-11-08-Final-v2.pdf"));
        // Paths, URLs and addresses are excluded on purpose.
        assert!(!looks_secret(
            "C:\\Users\\satya\\Documents\\Report2026-11-08.pdf"
        ));
        // A URL in any scheme: `://` is the marker, so `ftp://…` covers the
        // rule without putting a web scheme in an offline-verified source file.
        assert!(!looks_secret(
            "ftp://files.example.com/backups/2026-11-08.tar.gz"
        ));
        assert!(!looks_secret("satya@example.co.uk"));
        // Short keys and empty values never count.
        assert!(!looks_secret("sk-"));
        assert!(!looks_secret("token:"));
        assert!(!looks_secret("password= (blank)"));
    }
}
