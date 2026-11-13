//! Pure checks for the web server's three gates: Host, token, Origin.

pub const COOKIE: &str = "shipherd";

/// 256 random bits as hex, generated once per run.
pub fn new_token() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time, so response timing doesn't reveal how much of a guess was right.
pub fn token_matches(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// Rejecting other Host values defeats DNS rebinding, where an attacker's domain resolves to 127.0.0.1.
pub fn host_allowed(host: Option<&str>, port: u16) -> bool {
    host.is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"))
}

/// Browsers always send Origin on WebSocket upgrades; only our own page may connect.
pub fn origin_allowed(origin: Option<&str>, port: u16) -> bool {
    origin.is_some_and(|o| {
        o == format!("http://127.0.0.1:{port}") || o == format!("http://localhost:{port}")
    })
}

pub fn cookie_token(header: Option<&str>) -> Option<&str> {
    header?
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(COOKIE)?.strip_prefix('='))
}

pub fn query_token(query: &str) -> Option<&str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_64_hex_chars_and_unique() {
        let a = new_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, new_token());
    }

    #[test]
    fn token_comparison() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("abd", "abc"));
        assert!(!token_matches("ab", "abc"));
    }

    #[test]
    fn only_loopback_hosts_on_our_port() {
        assert!(host_allowed(Some("127.0.0.1:7777"), 7777));
        assert!(host_allowed(Some("localhost:7777"), 7777));
        assert!(!host_allowed(Some("127.0.0.1:8888"), 7777));
        assert!(!host_allowed(Some("evil.example:7777"), 7777));
        assert!(!host_allowed(None, 7777));
    }

    #[test]
    fn only_our_own_origin() {
        assert!(origin_allowed(Some("http://127.0.0.1:7777"), 7777));
        assert!(origin_allowed(Some("http://localhost:7777"), 7777));
        assert!(!origin_allowed(Some("https://evil.example"), 7777));
        assert!(!origin_allowed(
            Some("http://127.0.0.1:7777.evil.example"),
            7777
        ));
        assert!(!origin_allowed(None, 7777));
    }

    #[test]
    fn reads_the_token_from_cookie_and_query() {
        assert_eq!(cookie_token(Some("a=1; shipherd=tok; b=2")), Some("tok"));
        assert_eq!(cookie_token(Some("shipherdx=tok")), None);
        assert_eq!(cookie_token(None), None);
        assert_eq!(query_token("x=1&token=tok"), Some("tok"));
        assert_eq!(query_token("tokenx=tok"), None);
    }
}
