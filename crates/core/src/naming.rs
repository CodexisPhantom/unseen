//! Names that Unseen derives from movie metadata (docs/09-filesystem.md,
//! "Naming templates"). Only the default movie folder template is here; the
//! template engine with all Radarr tokens is a later issue.

/// Replaces the characters a file name cannot hold, as Radarr does with
/// "replace illegal characters" on and the colon replacement `Delete`.
///
/// `\` and `/` become `+`, `?` becomes `!`, `*` becomes `-`, and `< > : | "`
/// are removed. Leading spaces and dots and trailing spaces are trimmed.
pub fn clean_file_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .filter_map(|c| match c {
            '\\' | '/' => Some('+'),
            '?' => Some('!'),
            '*' => Some('-'),
            '<' | '>' | ':' | '|' | '"' => None,
            c => Some(c),
        })
        .collect();
    replaced
        .trim_start_matches([' ', '.'])
        .trim_end_matches(' ')
        .to_owned()
}

/// The folder of a movie with the default template
/// `{Movie Title} ({Release Year})`.
pub fn movie_folder_name(title: &str, year: i32) -> String {
    format!("{} ({year})", clean_file_name(title))
}

/// The title to sort by: lower case without a leading article.
pub fn sort_title(title: &str) -> String {
    let lower = title.trim().to_lowercase();
    ["the ", "an ", "a "]
        .iter()
        .find_map(|article| lower.strip_prefix(article))
        .unwrap_or(&lower)
        .trim_start()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_follow_the_radarr_default() {
        for (title, year, expected) in [
            ("The Matrix", 1999, "The Matrix (1999)"),
            (
                "Mission: Impossible - Fallout",
                2018,
                "Mission Impossible - Fallout (2018)",
            ),
            (
                "Who Framed Roger Rabbit?",
                1988,
                "Who Framed Roger Rabbit! (1988)",
            ),
            ("Face/Off", 1997, "Face+Off (1997)"),
            ("S*P*Y*S", 1974, "S-P-Y-S (1974)"),
            ("\"Crocodile\" Dundee", 1986, "Crocodile Dundee (1986)"),
            ("<Title> | Two", 2020, "Title  Two (2020)"),
            ("...And Justice for All", 1979, "And Justice for All (1979)"),
            ("Amélie", 2001, "Amélie (2001)"),
        ] {
            assert_eq!(movie_folder_name(title, year), expected, "{title}");
        }
    }

    #[test]
    fn sort_titles_drop_leading_articles() {
        assert_eq!(sort_title("The Matrix"), "matrix");
        assert_eq!(sort_title("A Quiet Place"), "quiet place");
        assert_eq!(sort_title("An American Tail"), "american tail");
        assert_eq!(sort_title("Theory of Everything"), "theory of everything");
        assert_eq!(sort_title("Anna"), "anna");
        assert_eq!(sort_title("The"), "the");
    }
}
