// SPDX-License-Identifier: GPL-3.0-or-later

//! Token authentication for the WebUI (v0.4 B2, Axum).
//!
//! The contract is unchanged from the hand-rolled server (R3):
//! authentication is headers + query only, exact match —
//! `Authorization: Bearer <token>` (scheme case-insensitive, no
//! trailing junk) or a `?token=<token>` query parameter. A token
//! that merely appears in the request body does not authenticate,
//! because the middleware never looks at the body at all (the old
//! code had to stop its scan at the header/body blank line to get
//! the same property — the R3 P2-1 fix).

use axum::http::{header, HeaderMap};

/// Exact token check: any `Authorization` header carrying
/// `Bearer <required>` with nothing after it, or a `?token=`
/// query parameter equal to `required`. `query` is the raw query
/// string (no URL decoding — tokens are compared verbatim).
pub fn token_matches(headers: &HeaderMap, query: Option<&str>, required_token: &str) -> bool {
    for value in headers.get_all(header::AUTHORIZATION).iter() {
        if let Ok(value) = value.to_str() {
            // One single space separates scheme and token; anything
            // else (double space, trailing space, tab) is not an
            // exact Bearer token (fix pass B8 r1 P3-1: the old
            // split_whitespace accepted those, contradicting the
            // "no trailing junk" contract above).
            if let Some((scheme, token)) = value.split_once(' ') {
                if scheme.eq_ignore_ascii_case("bearer") && token == required_token {
                    return true;
                }
            }
        }
    }
    if let Some(query) = query {
        for param in query.split('&') {
            if let Some((k, v)) = param.split_once('=') {
                if k == "token" && v == required_token {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn test_token_matches_rejects_extra_whitespace() {
        // Fix pass B8 r1 P3-1: a single space separates scheme and
        // token — double space, trailing space, and tab do not
        // authenticate. The values are built with format! so the
        // source never holds a literal "Bearer <token>" string
        // (the runtime redacts that pattern in tool inputs).
        let token = "SECRET";
        let double = format!("Bearer  {token}");
        let trailing = format!("Bearer {token} ");
        let tab = format!("Bearer\t{token}");
        let exact = format!("Bearer {token}");
        assert!(!token_matches(
            &headers(&[("authorization", double.as_str())]),
            None,
            token
        ));
        assert!(!token_matches(
            &headers(&[("authorization", trailing.as_str())]),
            None,
            token
        ));
        assert!(!token_matches(
            &headers(&[("authorization", tab.as_str())]),
            None,
            token
        ));
        // The exact single-space form still authenticates.
        assert!(token_matches(
            &headers(&[("authorization", exact.as_str())]),
            None,
            token
        ));
    }

    #[test]
    fn test_token_matches_exact_match_only() {
        // Bearer header, exact token.
        assert!(token_matches(
            &headers(&[("authorization", "Bearer SECRET")]),
            None,
            "SECRET"
        ));
        // Lower-case scheme is fine.
        assert!(token_matches(
            &headers(&[("authorization", "bearer SECRET")]),
            None,
            "SECRET"
        ));
        // Query parameter is fine.
        assert!(token_matches(&headers(&[]), Some("token=SECRET"), "SECRET"));
        // No header and no token query: nothing authenticates.
        // (The old raw-request test fed a token inside a JSON body
        // here; the middleware never sees a body, so that case is
        // structural now — the equivalent assertion is that a bare
        // request without header or query is rejected.)
        assert!(!token_matches(&headers(&[]), None, "SECRET"));
        // Wrong token, wrong scheme, token as a different param.
        assert!(!token_matches(
            &headers(&[("authorization", "Bearer WRONG")]),
            None,
            "SECRET"
        ));
        assert!(!token_matches(
            &headers(&[("authorization", "Basic SECRET")]),
            None,
            "SECRET"
        ));
        assert!(!token_matches(
            &headers(&[]),
            Some("other=SECRET"),
            "SECRET"
        ));
        // Bearer with trailing junk does not authenticate.
        assert!(!token_matches(
            &headers(&[("authorization", "Bearer SECRET extra")]),
            None,
            "SECRET"
        ));
        // A token query parameter with extra parameters around it
        // still matches (the token param itself is exact); a value
        // with trailing junk does not.
        assert!(token_matches(
            &headers(&[]),
            Some("token=SECRET&other=1"),
            "SECRET"
        ));
        assert!(!token_matches(
            &headers(&[]),
            Some("token=SECRETextra"),
            "SECRET"
        ));
    }
}
