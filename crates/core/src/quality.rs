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

    /// The quality with this Radarr id, if any.
    pub fn from_id(id: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|quality| quality.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::{Modifier as M, Quality, Source as S};

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
}
