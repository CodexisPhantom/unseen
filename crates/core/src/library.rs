//! Adopting an existing library (docs/09-filesystem.md, "Adopting an
//! existing library"): what a folder name says about its movie, which files
//! of a folder are videos, and when a TMDB result is a confident match. No
//! disk access here.

use std::sync::LazyLock;

use regex::Regex;

use crate::metadata::MovieSummary;
use crate::release;

/// What the name of a movie folder says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderGuess {
    pub title: String,
    pub year: Option<i32>,
    /// From `{tmdb-123}`, `[tmdbid-123]` and the like.
    pub tmdb_id: Option<i32>,
    /// From `{imdb-tt123}`, `[imdbid-tt123]` and the like; lower case.
    pub imdb_id: Option<String>,
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a fixed pattern")
}

static TMDB_TAG: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\s*[\{\[]\s*tmdb(?:id)?\s*[-=: ]\s*(\d+)\s*[\}\]]"));
static IMDB_TAG: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\s*[\{\[]\s*imdb(?:id)?\s*[-=: ]\s*(tt\d+)\s*[\}\]]"));
/// Radarr's default folder name: `Title (2020)`.
static TITLE_YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"^(.*?)\s*\((\d{4})\)\s*$"));

/// Reads the title, the year and the id tags of a folder name. A name that
/// does not end in `(YYYY)` goes through the release-name parser.
pub fn guess_folder(name: &str) -> FolderGuess {
    let tmdb_id = TMDB_TAG
        .captures(name)
        .and_then(|found| found[1].parse().ok());
    let imdb_id = IMDB_TAG
        .captures(name)
        .map(|found| found[1].to_ascii_lowercase());
    let stripped = TMDB_TAG.replace_all(name, "");
    let stripped = IMDB_TAG.replace_all(&stripped, "");
    let stripped = stripped.trim();
    let (title, year) = match TITLE_YEAR.captures(stripped) {
        Some(found) if !found[1].trim().is_empty() => {
            (found[1].trim().to_owned(), found[2].parse().ok())
        }
        _ => {
            let parsed = release::parse(stripped);
            (parsed.title, parsed.year.map(i32::from))
        }
    };
    FolderGuess {
        title,
        year,
        tmdb_id,
        imdb_id,
    }
}

const VIDEO_EXTENSIONS: [&str; 11] = [
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "ts", "m2ts", "mpg", "mpeg", "webm",
];

/// Sub-folders that hold extras, not the movie (Plex and Jellyfin names).
const EXTRA_FOLDERS: [&str; 14] = [
    "sample",
    "samples",
    "extras",
    "extra",
    "featurettes",
    "behind the scenes",
    "deleted scenes",
    "interviews",
    "scenes",
    "shorts",
    "trailers",
    "other",
    "subs",
    "subtitles",
];

static SAMPLE_NAME: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(?:^|[^a-z0-9])sample(?:$|[^a-z0-9])"));

/// True for a folder that holds extras and is skipped.
pub fn is_extras_folder(name: &str) -> bool {
    let name = name.trim().to_lowercase();
    EXTRA_FOLDERS.contains(&name.as_str())
}

/// True for a video file that can be the movie: a known video extension and
/// not a sample.
pub fn is_movie_video(file_name: &str) -> bool {
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    VIDEO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        && !SAMPLE_NAME.is_match(stem)
}

fn alphanumeric(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Titles compare by their letters and digits, so `Mission: Impossible` and
/// the folder `Mission Impossible` are the same.
pub fn same_title(a: &str, b: &str) -> bool {
    let a = alphanumeric(a);
    !a.is_empty() && a == alphanumeric(b)
}

/// The one result with the folder's year and title, or `None`. A folder
/// without a year, or with two or no such results, is not guessed.
pub fn confident_match<'a>(
    guess: &FolderGuess,
    results: &'a [MovieSummary],
) -> Option<&'a MovieSummary> {
    let year = guess.year?;
    let mut found = results.iter().filter(|movie| {
        movie.year() == Some(year)
            && (same_title(&movie.title, &guess.title)
                || same_title(&movie.original_title, &guess.title))
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess(name: &str) -> (String, Option<i32>, Option<i32>, Option<String>) {
        let g = guess_folder(name);
        (g.title, g.year, g.tmdb_id, g.imdb_id)
    }

    #[test]
    fn the_radarr_default_name_gives_title_and_year() {
        assert_eq!(
            guess("The Matrix (1999)"),
            ("The Matrix".to_owned(), Some(1999), None, None)
        );
        assert_eq!(
            guess("2001 A Space Odyssey (1968)"),
            ("2001 A Space Odyssey".to_owned(), Some(1968), None, None)
        );
        assert_eq!(
            guess("Blade Runner 2049 (2017)"),
            ("Blade Runner 2049".to_owned(), Some(2017), None, None)
        );
    }

    #[test]
    fn id_tags_are_read_and_removed_from_the_title() {
        assert_eq!(
            guess("The Matrix (1999) {tmdb-603}"),
            ("The Matrix".to_owned(), Some(1999), Some(603), None)
        );
        assert_eq!(
            guess("The Matrix (1999) [imdbid-tt0133093]"),
            (
                "The Matrix".to_owned(),
                Some(1999),
                None,
                Some("tt0133093".to_owned())
            )
        );
        assert_eq!(
            guess("Alien (1979) {imdb-TT0078748} {tmdb-348}"),
            (
                "Alien".to_owned(),
                Some(1979),
                Some(348),
                Some("tt0078748".to_owned())
            )
        );
    }

    #[test]
    fn a_release_style_name_goes_through_the_parser() {
        assert_eq!(
            guess("Dune.2021.1080p.BluRay.x264-GRP"),
            ("Dune".to_owned(), Some(2021), None, None)
        );
        assert_eq!(guess("Stray"), ("Stray".to_owned(), None, None, None));
    }

    #[test]
    fn video_files_exclude_samples_and_other_files() {
        for yes in [
            "The Matrix (1999) Bluray-1080p.mkv",
            "movie.MP4",
            "a.b.c.avi",
            "Sampler Pack Movie.mkv",
        ] {
            assert!(is_movie_video(yes), "{yes}");
        }
        for no in [
            "movie-sample.mkv",
            "Sample.mkv",
            "movie.sample.mp4",
            "movie.nfo",
            "poster.jpg",
            "movie.srt",
            "noextension",
        ] {
            assert!(!is_movie_video(no), "{no}");
        }
    }

    #[test]
    fn extras_folders_are_skipped_without_regard_to_case() {
        assert!(is_extras_folder("Extras"));
        assert!(is_extras_folder("sample"));
        assert!(is_extras_folder("Behind The Scenes"));
        assert!(!is_extras_folder("CD1"));
    }

    fn summary(id: i32, title: &str, date: &str) -> MovieSummary {
        MovieSummary {
            tmdb_id: id,
            title: title.to_owned(),
            original_title: title.to_owned(),
            overview: String::new(),
            release_date: Some(date.to_owned()),
            poster_path: None,
        }
    }

    #[test]
    fn a_match_needs_the_same_year_and_title() {
        let results = [
            summary(1, "Dune", "1984-12-14"),
            summary(2, "Dune", "2021-09-15"),
            summary(3, "Dune: Part Two", "2024-02-27"),
        ];
        let found = confident_match(&guess_folder("Dune (2021)"), &results);
        assert_eq!(found.map(|m| m.tmdb_id), Some(2));
        // Punctuation does not matter.
        let results = [summary(9, "Mission: Impossible - Fallout", "2018-07-25")];
        let found = confident_match(
            &guess_folder("Mission Impossible - Fallout (2018)"),
            &results,
        );
        assert_eq!(found.map(|m| m.tmdb_id), Some(9));
    }

    #[test]
    fn a_doubtful_match_is_none() {
        let results = [summary(2, "Dune", "2021-09-15")];
        // Wrong year, no year, other title.
        assert!(confident_match(&guess_folder("Dune (2020)"), &results).is_none());
        assert!(confident_match(&guess_folder("Dune"), &results).is_none());
        assert!(confident_match(&guess_folder("Duna (2021)"), &results).is_none());
        // Two results with the same title and year.
        let twice = [
            summary(1, "Dune", "2021-01-01"),
            summary(2, "Dune", "2021-09-15"),
        ];
        assert!(confident_match(&guess_folder("Dune (2021)"), &twice).is_none());
        assert!(confident_match(&guess_folder("Dune (2021)"), &[]).is_none());
    }
}
