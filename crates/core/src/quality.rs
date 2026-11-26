//! Radarr's qualities: ids, names, source, resolution, modifier and order.
//!
//! The values equal Radarr's `NzbDrone.Core.Qualities.Quality` so that quality
//! profiles, release parsing, library adoption and the compat API agree with
//! Radarr. The ids are stored in the database and must never change.

/// Where a release comes from. Same members as Radarr's `QualitySource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Unknown,
    Cam,
    Telesync,
    Telecine,
    Workprint,
    Dvd,
    Tv,
    WebDl,
    WebRip,
    Bluray,
}

/// What distinguishes qualities that share a source and a resolution. Same
/// members as Radarr's `Modifier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    None,
    Regional,
    Screener,
    RawHd,
    BrDisk,
    Remux,
}

/// A Radarr quality. Variants are declared in the default order, lowest to
/// highest, so the derived `Ord` is the default ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Quality {
    Unknown,
    Workprint,
    Cam,
    Telesync,
    Telecine,
    Regional,
    DvdScr,
    Sdtv,
    Dvd,
    DvdR,
    WebDl480p,
    WebRip480p,
    Bluray480p,
    Bluray576p,
    Hdtv720p,
    WebDl720p,
    WebRip720p,
    Bluray720p,
    Hdtv1080p,
    WebDl1080p,
    WebRip1080p,
    Bluray1080p,
    Remux1080p,
    Hdtv2160p,
    WebDl2160p,
    WebRip2160p,
    Bluray2160p,
    Remux2160p,
    BrDisk,
    RawHd,
}

impl Quality {
    /// Every quality in the default order, lowest to highest.
    pub const ALL: [Self; 30] = [
        Self::Unknown,
        Self::Workprint,
        Self::Cam,
        Self::Telesync,
        Self::Telecine,
        Self::Regional,
        Self::DvdScr,
        Self::Sdtv,
        Self::Dvd,
        Self::DvdR,
        Self::WebDl480p,
        Self::WebRip480p,
        Self::Bluray480p,
        Self::Bluray576p,
        Self::Hdtv720p,
        Self::WebDl720p,
        Self::WebRip720p,
        Self::Bluray720p,
        Self::Hdtv1080p,
        Self::WebDl1080p,
        Self::WebRip1080p,
        Self::Bluray1080p,
        Self::Remux1080p,
        Self::Hdtv2160p,
        Self::WebDl2160p,
        Self::WebRip2160p,
        Self::Bluray2160p,
        Self::Remux2160p,
        Self::BrDisk,
        Self::RawHd,
    ];

    /// Radarr's row for this quality: id, name, source, resolution, modifier.
    const fn row(self) -> (i32, &'static str, Source, u16, Modifier) {
        use Modifier as M;
        use Source as S;
        match self {
            Self::Unknown => (0, "Unknown", S::Unknown, 0, M::None),
            Self::Workprint => (24, "WORKPRINT", S::Workprint, 0, M::None),
            Self::Cam => (25, "CAM", S::Cam, 0, M::None),
            Self::Telesync => (26, "TELESYNC", S::Telesync, 0, M::None),
            Self::Telecine => (27, "TELECINE", S::Telecine, 0, M::None),
            Self::Regional => (29, "REGIONAL", S::Dvd, 480, M::Regional),
            Self::DvdScr => (28, "DVDSCR", S::Dvd, 480, M::Screener),
            Self::Sdtv => (1, "SDTV", S::Tv, 480, M::None),
            Self::Dvd => (2, "DVD", S::Dvd, 0, M::None),
            Self::DvdR => (23, "DVD-R", S::Dvd, 480, M::Remux),
            Self::WebDl480p => (8, "WEBDL-480p", S::WebDl, 480, M::None),
            Self::WebRip480p => (12, "WEBRip-480p", S::WebRip, 480, M::None),
            Self::Bluray480p => (20, "Bluray-480p", S::Bluray, 480, M::None),
            Self::Bluray576p => (21, "Bluray-576p", S::Bluray, 576, M::None),
            Self::Hdtv720p => (4, "HDTV-720p", S::Tv, 720, M::None),
            Self::WebDl720p => (5, "WEBDL-720p", S::WebDl, 720, M::None),
            Self::WebRip720p => (14, "WEBRip-720p", S::WebRip, 720, M::None),
            Self::Bluray720p => (6, "Bluray-720p", S::Bluray, 720, M::None),
            Self::Hdtv1080p => (9, "HDTV-1080p", S::Tv, 1080, M::None),
            Self::WebDl1080p => (3, "WEBDL-1080p", S::WebDl, 1080, M::None),
            Self::WebRip1080p => (15, "WEBRip-1080p", S::WebRip, 1080, M::None),
            Self::Bluray1080p => (7, "Bluray-1080p", S::Bluray, 1080, M::None),
            Self::Remux1080p => (30, "Remux-1080p", S::Bluray, 1080, M::Remux),
            Self::Hdtv2160p => (16, "HDTV-2160p", S::Tv, 2160, M::None),
            Self::WebDl2160p => (18, "WEBDL-2160p", S::WebDl, 2160, M::None),
            Self::WebRip2160p => (17, "WEBRip-2160p", S::WebRip, 2160, M::None),
            Self::Bluray2160p => (19, "Bluray-2160p", S::Bluray, 2160, M::None),
            Self::Remux2160p => (31, "Remux-2160p", S::Bluray, 2160, M::Remux),
            Self::BrDisk => (22, "BR-DISK", S::Bluray, 1080, M::BrDisk),
            Self::RawHd => (10, "Raw-HD", S::Tv, 1080, M::RawHd),
        }
    }

    /// Radarr's id. Stable: stored in the database and sent by the compat API.
    pub const fn id(self) -> i32 {
        self.row().0
    }

    /// Radarr's name, for example `Bluray-1080p`.
    pub const fn name(self) -> &'static str {
        self.row().1
    }

    pub const fn source(self) -> Source {
        self.row().2
    }

    /// Vertical resolution in pixels. 0 when Radarr gives none.
    pub const fn resolution(self) -> u16 {
        self.row().3
    }

    pub const fn modifier(self) -> Modifier {
        self.row().4
    }

    /// Every quality allowed for TV shows in Sonarr's default order, lowest to
    /// highest (`docs/07-release-scoring.md`, "Qualities for TV").
    pub const TV_DEFAULT_ORDER: [Self; 22] = [
        Self::Unknown,
        Self::Sdtv,
        Self::WebRip480p,
        Self::WebDl480p,
        Self::Dvd,
        Self::Bluray480p,
        Self::Bluray576p,
        Self::Hdtv720p,
        Self::Hdtv1080p,
        Self::RawHd,
        Self::WebRip720p,
        Self::WebDl720p,
        Self::Bluray720p,
        Self::WebRip1080p,
        Self::WebDl1080p,
        Self::Bluray1080p,
        Self::Remux1080p,
        Self::Hdtv2160p,
        Self::WebRip2160p,
        Self::WebDl2160p,
        Self::Bluray2160p,
        Self::Remux2160p,
    ];

    /// The quality with this Radarr id, if any.
    pub fn from_id(id: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|quality| quality.id() == id)
    }

    /// Whether this quality is allowed for TV shows. Excludes the movie-only
    /// qualities (`WORKPRINT`, `CAM`, `TELESYNC`, `TELECINE`, `REGIONAL`,
    /// `DVDSCR`, `DVD-R`, `BR-DISK`).
    pub const fn is_tv(self) -> bool {
        self.sonarr_id().is_some()
    }

    /// Qualities allowed for TV shows in Sonarr's default ranking order,
    /// lowest to highest.
    pub const fn tv_default_order() -> &'static [Self] {
        &Self::TV_DEFAULT_ORDER
    }

    /// Sonarr's quality id, or `None` for movie-only qualities.
    ///
    /// Four qualities differ from Unseen's stored Radarr ids: `Bluray-480p`
    /// (13), `Bluray-576p` (22), `Remux-1080p` (20) and `Remux-2160p` (21).
    pub const fn sonarr_id(self) -> Option<i32> {
        match self {
            Self::Workprint
            | Self::Cam
            | Self::Telesync
            | Self::Telecine
            | Self::Regional
            | Self::DvdScr
            | Self::DvdR
            | Self::BrDisk => None,
            Self::Bluray480p => Some(13),
            Self::Bluray576p => Some(22),
            Self::Remux1080p => Some(20),
            Self::Remux2160p => Some(21),
            _ => Some(self.id()),
        }
    }

    /// Sonarr's quality name, or `None` for movie-only qualities.
    ///
    /// Differs from Radarr's name for the two remux qualities (`Bluray-1080p
    /// Remux` and `Bluray-2160p Remux`).
    pub const fn sonarr_name(self) -> Option<&'static str> {
        match self {
            Self::Workprint
            | Self::Cam
            | Self::Telesync
            | Self::Telecine
            | Self::Regional
            | Self::DvdScr
            | Self::DvdR
            | Self::BrDisk => None,
            Self::Remux1080p => Some("Bluray-1080p Remux"),
            Self::Remux2160p => Some("Bluray-2160p Remux"),
            _ => Some(self.name()),
        }
    }

    /// The quality with this Sonarr id, if any.
    pub fn from_sonarr_id(id: i32) -> Option<Self> {
        Self::TV_DEFAULT_ORDER
            .into_iter()
            .find(|quality| quality.sonarr_id() == Some(id))
    }
}

const MEGABYTE: f64 = 1024.0 * 1024.0;

/// Size limits of one quality, in megabytes per minute of runtime, as in
/// Radarr's quality definitions (docs/07-release-scoring.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeLimits {
    pub min: f64,
    /// The size to prefer when two releases tie. `None`: no preference.
    pub preferred: Option<f64>,
    /// `None`: no limit.
    pub max: Option<f64>,
}

/// How a release size compares with the limits of its quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeCheck {
    Within,
    TooSmall,
    TooLarge,
}

/// Why a set of limits is refused: the field and a message for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitsError {
    pub field: &'static str,
    pub message: &'static str,
}

impl SizeLimits {
    /// The limits must be finite numbers with `min` at least 0, `max` above 0
    /// and not below `min`, and `preferred` between `min` and `max`.
    pub fn validate(&self) -> Result<(), LimitsError> {
        let error = |field, message| Err(LimitsError { field, message });
        let finite = |value: f64| value.is_finite();
        if !finite(self.min) || self.min < 0.0 {
            return error("minSize", "must be 0 or more");
        }
        if let Some(max) = self.max {
            if !finite(max) || max <= 0.0 {
                return error("maxSize", "must be more than 0, or empty for no limit");
            }
            if max < self.min {
                return error("maxSize", "must not be below the minimum");
            }
        }
        if let Some(preferred) = self.preferred {
            if !finite(preferred) || preferred < self.min {
                return error("preferredSize", "must not be below the minimum");
            }
            if self.max.is_some_and(|max| preferred > max) {
                return error("preferredSize", "must not be above the maximum");
            }
        }
        Ok(())
    }

    /// The preferred size in bytes for a movie of `runtime_minutes`. `None`
    /// when there is no preferred size or no runtime.
    pub fn preferred_bytes(&self, runtime_minutes: Option<u32>) -> Option<f64> {
        let minutes = runtime_minutes.filter(|minutes| *minutes > 0)?;
        Some(self.preferred? * f64::from(minutes) * MEGABYTE)
    }

    /// Compares the size of a release, in bytes, with the limits for a movie
    /// of `runtime_minutes`. A movie without a runtime passes: there is
    /// nothing to scale the limits with.
    pub fn check(&self, size_bytes: u64, runtime_minutes: Option<u32>) -> SizeCheck {
        let Some(minutes) = runtime_minutes.filter(|minutes| *minutes > 0) else {
            return SizeCheck::Within;
        };
        let size = size_bytes as f64;
        let scale = f64::from(minutes) * MEGABYTE;
        if size < self.min * scale {
            SizeCheck::TooSmall
        } else if self.max.is_some_and(|max| size > max * scale) {
            SizeCheck::TooLarge
        } else {
            SizeCheck::Within
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LimitsError, Modifier as M, Quality, SizeCheck, SizeLimits, Source as S};

    const MB: u64 = 1024 * 1024;

    fn limits(min: f64, preferred: Option<f64>, max: Option<f64>) -> SizeLimits {
        SizeLimits {
            min,
            preferred,
            max,
        }
    }

    #[test]
    fn a_size_is_checked_against_the_limits_scaled_by_the_runtime() {
        // 10 to 100 MB per minute, 100 minutes: 1000 MB to 10000 MB.
        let hd = limits(10.0, Some(50.0), Some(100.0));
        assert_eq!(hd.check(999 * MB, Some(100)), SizeCheck::TooSmall);
        assert_eq!(hd.check(1000 * MB, Some(100)), SizeCheck::Within);
        assert_eq!(hd.check(10_000 * MB, Some(100)), SizeCheck::Within);
        assert_eq!(hd.check(10_001 * MB, Some(100)), SizeCheck::TooLarge);
    }

    #[test]
    fn a_movie_without_a_runtime_passes() {
        let hd = limits(10.0, None, Some(100.0));
        assert_eq!(hd.check(1, None), SizeCheck::Within);
        assert_eq!(hd.check(u64::MAX, None), SizeCheck::Within);
        assert_eq!(hd.check(u64::MAX, Some(0)), SizeCheck::Within);
    }

    #[test]
    fn no_maximum_is_unlimited_and_a_zero_minimum_accepts_any_small_size() {
        let open = limits(0.0, None, None);
        assert_eq!(open.check(0, Some(120)), SizeCheck::Within);
        assert_eq!(open.check(u64::MAX, Some(120)), SizeCheck::Within);
    }

    #[test]
    fn limits_are_validated() {
        let field =
            |limits: SizeLimits| limits.validate().map_err(|LimitsError { field, .. }| field);
        assert_eq!(field(limits(0.0, None, None)), Ok(()));
        assert_eq!(field(limits(5.0, Some(5.0), Some(5.0))), Ok(()));
        assert_eq!(field(limits(-1.0, None, None)), Err("minSize"));
        assert_eq!(field(limits(f64::NAN, None, None)), Err("minSize"));
        assert_eq!(field(limits(0.0, None, Some(0.0))), Err("maxSize"));
        assert_eq!(field(limits(10.0, None, Some(9.0))), Err("maxSize"));
        assert_eq!(field(limits(10.0, Some(9.0), None)), Err("preferredSize"));
        assert_eq!(
            field(limits(1.0, Some(11.0), Some(10.0))),
            Err("preferredSize")
        );
    }

    /// Radarr's qualities, transcribed from `Quality.cs` (Radarr `develop`,
    /// commit e83f5e83): the constructor arguments of each quality, in the
    /// order of the `Weight` of `DefaultQualityDefinitions`. `GET
    /// /api/v3/qualitydefinition` reports these ids and names.
    const RADARR: [(i32, &str, S, u16, M); 30] = [
        (0, "Unknown", S::Unknown, 0, M::None),
        (24, "WORKPRINT", S::Workprint, 0, M::None),
        (25, "CAM", S::Cam, 0, M::None),
        (26, "TELESYNC", S::Telesync, 0, M::None),
        (27, "TELECINE", S::Telecine, 0, M::None),
        (29, "REGIONAL", S::Dvd, 480, M::Regional),
        (28, "DVDSCR", S::Dvd, 480, M::Screener),
        (1, "SDTV", S::Tv, 480, M::None),
        (2, "DVD", S::Dvd, 0, M::None),
        (23, "DVD-R", S::Dvd, 480, M::Remux),
        (8, "WEBDL-480p", S::WebDl, 480, M::None),
        (12, "WEBRip-480p", S::WebRip, 480, M::None),
        (20, "Bluray-480p", S::Bluray, 480, M::None),
        (21, "Bluray-576p", S::Bluray, 576, M::None),
        (4, "HDTV-720p", S::Tv, 720, M::None),
        (5, "WEBDL-720p", S::WebDl, 720, M::None),
        (14, "WEBRip-720p", S::WebRip, 720, M::None),
        (6, "Bluray-720p", S::Bluray, 720, M::None),
        (9, "HDTV-1080p", S::Tv, 1080, M::None),
        (3, "WEBDL-1080p", S::WebDl, 1080, M::None),
        (15, "WEBRip-1080p", S::WebRip, 1080, M::None),
        (7, "Bluray-1080p", S::Bluray, 1080, M::None),
        (30, "Remux-1080p", S::Bluray, 1080, M::Remux),
        (16, "HDTV-2160p", S::Tv, 2160, M::None),
        (18, "WEBDL-2160p", S::WebDl, 2160, M::None),
        (17, "WEBRip-2160p", S::WebRip, 2160, M::None),
        (19, "Bluray-2160p", S::Bluray, 2160, M::None),
        (31, "Remux-2160p", S::Bluray, 2160, M::Remux),
        (22, "BR-DISK", S::Bluray, 1080, M::BrDisk),
        (10, "Raw-HD", S::Tv, 1080, M::RawHd),
    ];

    #[test]
    fn matches_radarr_ids_names_and_order() {
        let actual: Vec<_> = Quality::ALL
            .iter()
            .map(|q| (q.id(), q.name(), q.source(), q.resolution(), q.modifier()))
            .collect();
        assert_eq!(actual, RADARR);
    }

    #[test]
    fn all_is_in_declaration_order() {
        assert!(Quality::ALL.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(Quality::ALL[0], Quality::Unknown);
        assert_eq!(Quality::ALL[29], Quality::RawHd);
    }

    #[test]
    fn from_id_round_trips_and_rejects_unknown_ids() {
        for quality in Quality::ALL {
            assert_eq!(Quality::from_id(quality.id()), Some(quality));
        }
        // Radarr skips 11 and 13.
        for id in [-1, 11, 13, 32] {
            assert_eq!(Quality::from_id(id), None);
        }
    }

    /// The names in the first code block of the "Qualities" section of
    /// docs/07, in order.
    fn documented_names(doc: &str) -> Vec<&str> {
        let section = doc.split("\n## Qualities\n").nth(1).expect("section");
        let block = section.split("```").nth(1).expect("code block");
        block
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect()
    }

    #[test]
    fn matches_the_design_doc() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/07-release-scoring.md"
        );
        let doc = std::fs::read_to_string(path)
            .expect("read docs/07")
            .replace("\r\n", "\n");
        let names: Vec<_> = Quality::ALL.iter().map(|q| q.name()).collect();
        assert_eq!(names, documented_names(&doc));
    }

    /// Sonarr's qualities, transcribed from `Quality.cs` (Sonarr `develop`):
    /// Sonarr id, Sonarr name, and Unseen `Quality` variant, in the order of
    /// `DefaultQualityDefinitions` weights.
    const SONARR: [(i32, &str, Quality); 22] = [
        (0, "Unknown", Quality::Unknown),
        (1, "SDTV", Quality::Sdtv),
        (12, "WEBRip-480p", Quality::WebRip480p),
        (8, "WEBDL-480p", Quality::WebDl480p),
        (2, "DVD", Quality::Dvd),
        (13, "Bluray-480p", Quality::Bluray480p),
        (22, "Bluray-576p", Quality::Bluray576p),
        (4, "HDTV-720p", Quality::Hdtv720p),
        (9, "HDTV-1080p", Quality::Hdtv1080p),
        (10, "Raw-HD", Quality::RawHd),
        (14, "WEBRip-720p", Quality::WebRip720p),
        (5, "WEBDL-720p", Quality::WebDl720p),
        (6, "Bluray-720p", Quality::Bluray720p),
        (15, "WEBRip-1080p", Quality::WebRip1080p),
        (3, "WEBDL-1080p", Quality::WebDl1080p),
        (7, "Bluray-1080p", Quality::Bluray1080p),
        (20, "Bluray-1080p Remux", Quality::Remux1080p),
        (16, "HDTV-2160p", Quality::Hdtv2160p),
        (17, "WEBRip-2160p", Quality::WebRip2160p),
        (18, "WEBDL-2160p", Quality::WebDl2160p),
        (19, "Bluray-2160p", Quality::Bluray2160p),
        (21, "Bluray-2160p Remux", Quality::Remux2160p),
    ];

    #[test]
    fn matches_sonarr_ids_names_and_default_order() {
        let actual: Vec<_> = Quality::tv_default_order()
            .iter()
            .map(|&q| {
                (
                    q.sonarr_id().expect("tv quality has sonarr id"),
                    q.sonarr_name().expect("tv quality has sonarr name"),
                    q,
                )
            })
            .collect();
        assert_eq!(actual, SONARR);
    }

    #[test]
    fn movie_only_qualities_are_excluded_from_tv() {
        let movie_only = [
            Quality::Workprint,
            Quality::Cam,
            Quality::Telesync,
            Quality::Telecine,
            Quality::Regional,
            Quality::DvdScr,
            Quality::DvdR,
            Quality::BrDisk,
        ];
        for q in movie_only {
            assert!(!q.is_tv(), "{q:?} should not be a TV quality");
            assert_eq!(q.sonarr_id(), None, "{q:?}");
            assert_eq!(q.sonarr_name(), None, "{q:?}");
            assert!(!Quality::tv_default_order().contains(&q), "{q:?}");
        }
        let tv_count = Quality::ALL.iter().filter(|q| q.is_tv()).count();
        assert_eq!(tv_count, 22);
        assert_eq!(tv_count + movie_only.len(), Quality::ALL.len());
    }

    #[test]
    fn from_sonarr_id_round_trips_and_rejects_non_tv_ids() {
        for &(id, _, quality) in &SONARR {
            assert_eq!(Quality::from_sonarr_id(id), Some(quality));
        }
        // Sonarr skips 11 and has no ids above 22 (Radarr's 23..=31 are movie-only or remapped).
        for id in [-1, 11, 23, 24, 25, 30, 31, 32] {
            assert_eq!(Quality::from_sonarr_id(id), None, "id {id}");
        }
        // Verify the four remapped ids explicitly.
        assert_eq!(Quality::from_sonarr_id(13), Some(Quality::Bluray480p));
        assert_eq!(Quality::from_sonarr_id(22), Some(Quality::Bluray576p));
        assert_eq!(Quality::from_sonarr_id(20), Some(Quality::Remux1080p));
        assert_eq!(Quality::from_sonarr_id(21), Some(Quality::Remux2160p));
    }

    #[test]
    fn tv_default_order_matches_the_design_doc() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/07-release-scoring.md"
        );
        let doc = std::fs::read_to_string(path)
            .expect("read docs/07")
            .replace("\r\n", "\n");
        let section = doc
            .split("\n### Qualities for TV\n")
            .nth(1)
            .expect("tv qualities section");
        let order_para = section
            .split("Default order of a new show profile")
            .nth(1)
            .expect("default order paragraph")
            .split(". The order differs")
            .next()
            .expect("order list");
        let after_colon = order_para.split("): ").nth(1).expect("after colon");
        let documented: Vec<&str> = after_colon
            .split([',', '/'])
            .map(|token| token.trim().trim_matches('`'))
            .filter(|token| !token.is_empty())
            .collect();
        let actual: Vec<&str> = Quality::tv_default_order()
            .iter()
            .map(|q| q.name())
            .collect();
        assert_eq!(actual, documented);
    }
}
