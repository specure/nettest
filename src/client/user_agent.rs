//! Reading a browser's identity out of its user-agent string.
//!
//! Lives outside the wasm module so it is covered by the ordinary test run: it
//! is pure string handling, and the interesting part — that every Chromium
//! browser also claims to be Chrome — is exactly the kind of thing a test
//! should pin down.

/// Pull `<name>/<version>` out of a user-agent string.
///
/// Order matters: every Chromium browser also claims "Chrome", and Chrome
/// claims "Safari", so the more specific brands are matched first.
pub fn parse_user_agent(user_agent: &str) -> (Option<String>, Option<String>) {
    for (needle, name) in [
        ("Edg/", "Edge"),
        ("OPR/", "Opera"),
        ("Chrome/", "Chrome"),
        ("Firefox/", "Firefox"),
        ("Version/", "Safari"),
    ] {
        if let Some(at) = user_agent.find(needle) {
            let rest = &user_agent[at + needle.len()..];
            let version: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            // "Version/17.0 ... Safari/605" is Safari; anything else claiming
            // Version/ is not, so only accept it when Safari is really there.
            if name == "Safari" && !user_agent.contains("Safari/") {
                continue;
            }
            return (Some(name.to_string()), Some(version).filter(|v| !v.is_empty()));
        }
    }
    (None, None)
}


#[cfg(test)]
mod tests {
    use super::parse_user_agent;

    #[test]
    fn recognises_the_common_browsers() {
        let chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";
        assert_eq!(parse_user_agent(chrome), (Some("Chrome".into()), Some("151.0.0.0".into())));

        let safari = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Safari/605.1.15";
        assert_eq!(parse_user_agent(safari), (Some("Safari".into()), Some("18.3".into())));

        let firefox = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14.7; rv:135.0) Gecko/20100101 Firefox/135.0";
        assert_eq!(parse_user_agent(firefox), (Some("Firefox".into()), Some("135.0".into())));
    }

    #[test]
    fn prefers_the_specific_brand_over_chrome() {
        // Every Chromium browser also says "Chrome"; Edge and Opera must not be
        // reported as Chrome.
        let edge = "Mozilla/5.0 ... Chrome/151.0.0.0 Safari/537.36 Edg/151.0.0.0";
        assert_eq!(parse_user_agent(edge).0, Some("Edge".into()));

        let opera = "Mozilla/5.0 ... Chrome/151.0.0.0 Safari/537.36 OPR/120.0.0.0";
        assert_eq!(parse_user_agent(opera).0, Some("Opera".into()));
    }

    #[test]
    fn leaves_an_unknown_agent_unnamed() {
        assert_eq!(parse_user_agent("curl/8.4.0"), (None, None));
    }
}
