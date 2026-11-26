//! Quality of an adopted file (docs/09-filesystem.md, "Adopting an existing
//! library"). Adoption never probes the file; it reads the quality from the
//! names, the file name first and then the movie folder name.

use crate::quality::Quality;
use crate::release::{self, ParsedRelease};

fn quality_of_name(name: &str) -> Quality {
    let ParsedRelease {
        source,
        modifier,
        resolution,
        ..
    } = release::parse(name);
    let name = release::quality_name(source, modifier, resolution);
    Quality::ALL
        .into_iter()
        .find(|quality| quality.name() == name)
        .unwrap_or(Quality::Unknown)
}

/// The quality that the names of a file say. `relative_path` is the path of
/// the file inside the movie folder (`/` between parts) and `folder_name`
/// the name of that folder. [`Quality::Unknown`] when neither name says.
pub fn file_quality(relative_path: &str, folder_name: &str) -> Quality {
    let file_name = relative_path.rsplit('/').next().unwrap_or(relative_path);
    match quality_of_name(file_name) {
        Quality::Unknown => quality_of_name(folder_name),
        quality => quality,
    }
}

/// Language codes (ISO 639-1 or `"multi"`) parsed from the file name, falling
/// back to the movie folder name when the file name carries no language tag.
#[must_use]
pub fn file_languages(relative_path: &str, folder_name: &str) -> Vec<String> {
    let file_name = relative_path.rsplit('/').next().unwrap_or(relative_path);
    let from_file = release::parse(file_name).languages;
    let langs = if from_file.is_empty() {
        release::parse(folder_name).languages
    } else {
        from_file
    };
    langs.into_iter().map(|l| l.code().to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_quality_full_name_gives_that_quality() {
        // `{Quality Full}` of the default file template is the quality name.
        for quality in Quality::ALL.into_iter().skip(1) {
            let folder = "The Matrix (1999)";
            let file = format!("{folder} {}.mkv", quality.name());
            assert_eq!(file_quality(&file, folder), quality, "{file}");
            // Also with a revision suffix and inside a sub-folder.
            let file = format!("CD1/{folder} {} Proper.mkv", quality.name());
            assert_eq!(file_quality(&file, folder), quality, "{file}");
        }
    }

    #[test]
    fn the_folder_name_is_the_fallback() {
        assert_eq!(
            file_quality("movie.mkv", "Dune.2021.2160p.WEB-DL.DDP5.1.H.265-FLUX"),
            Quality::WebDl2160p
        );
        // The file name wins over the folder name.
        assert_eq!(
            file_quality(
                "Dune (2021) Bluray-1080p.mkv",
                "Dune.2021.2160p.WEB-DL.H.265-FLUX"
            ),
            Quality::Bluray1080p
        );
    }

    #[test]
    fn names_without_quality_stay_unknown() {
        assert_eq!(
            file_quality("Dune (2021).mkv", "Dune (2021)"),
            Quality::Unknown
        );
        assert_eq!(file_quality("", ""), Quality::Unknown);
        // A resolution without a source says nothing, as in the parser.
        assert_eq!(
            file_quality("Dune.2021.1080p.x264-GRP.mkv", "Dune (2021)"),
            Quality::Unknown
        );
    }

    #[test]
    fn file_languages_reads_file_then_folder_name() {
        assert_eq!(
            file_languages(
                "Amelie.2001.FRENCH.1080p.BluRay.x264-GRP.mkv",
                "Amelie (2001)"
            ),
            vec!["fr"]
        );
        assert_eq!(
            file_languages("movie.mkv", "Amelie.2001.MULTI.1080p.BluRay-GRP"),
            vec!["multi"]
        );
        assert!(file_languages("The Matrix (1999).mkv", "The Matrix (1999)").is_empty());
    }
}
