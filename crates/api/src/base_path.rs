//! The mount prefix taken from UNSEEN_BASE_URL.

/// The mount prefix from `UNSEEN_BASE_URL`; empty at the root.
pub fn from_url(base_url: Option<&str>) -> String {
    let Some(base_url) = base_url else {
        return String::new();
    };
    let Ok(url) = url::Url::parse(base_url) else {
        return String::new();
    };
    let path = url.path();
    if path == "/" || path.is_empty() {
        return String::new();
    }
    // Slash-prefixed, no trailing slash, duplicate slashes collapsed.
    let mut out = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        out.push('/');
        out.push_str(segment);
    }
    out
}

/// Origin of a base URL: scheme, host and port. Drops path, query, fragment.
pub fn origin(base_url: &str) -> Option<String> {
    let url = url::Url::parse(base_url).ok()?;
    Some(url.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_url_cases() {
        assert_eq!(from_url(None), "");
        assert_eq!(from_url(Some("https://unseen.example.com")), "");
        assert_eq!(from_url(Some("https://unseen.example.com/")), "");
        assert_eq!(
            from_url(Some("https://unseen.example.com/unseen")),
            "/unseen"
        );
        assert_eq!(
            from_url(Some("https://unseen.example.com/unseen/")),
            "/unseen"
        );
        assert_eq!(
            from_url(Some("https://unseen.example.com/apps/unseen/")),
            "/apps/unseen"
        );
        assert_eq!(
            from_url(Some("https://unseen.example.com/unseen?x=1")),
            "/unseen"
        );
        assert_eq!(from_url(Some("not a url")), "");
        assert_eq!(from_url(Some("/unseen")), "");
    }

    #[test]
    fn origin_cases() {
        assert_eq!(
            origin("https://unseen.example.com/unseen/?q=1"),
            Some("https://unseen.example.com".to_owned())
        );
        assert_eq!(
            origin("http://host:8080/base"),
            Some("http://host:8080".to_owned())
        );
        assert_eq!(origin("not a url"), None);
    }
}
