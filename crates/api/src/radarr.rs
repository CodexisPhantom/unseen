//! Radarr v3 compatibility API mounted at `/radarr/api/v3`
//! (docs/14-compatibility.md, section D).

use crate::compat::CompatRouter;

/// Mount prefix for the Radarr v3 compatibility router.
pub const API_BASE: &str = "/radarr/api/v3";

/// Every Radarr v3 compatibility route is registered here with its required
/// permission. Routes are served case-insensitively with API-key-only auth;
/// anything unimplemented falls back to a logged `501 Not Implemented` JSON
/// response.
#[must_use]
pub fn routes() -> CompatRouter {
    CompatRouter::with_base(API_BASE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_base_matches_arr_compat_url_base() {
        assert_eq!(API_BASE, format!("{}/api/v3", arr_compat::radarr::URL_BASE));
    }
}
