//! Root folder rules that need no disk access (docs/09-filesystem.md).

use std::path::Path;

/// Why a new root folder collides with an existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlap<'a> {
    /// The same folder is already a root folder.
    Same(&'a Path),
    /// The new folder lies inside this root folder.
    Inside(&'a Path),
    /// This root folder lies inside the new folder.
    Contains(&'a Path),
}

/// The first of `existing` that is the same as `new`, contains it or lies
/// inside it. Compares whole path components, so `/movies2` is not inside
/// `/movies`.
pub fn overlap<'a>(
    new: &Path,
    existing: impl IntoIterator<Item = &'a Path>,
) -> Option<Overlap<'a>> {
    existing.into_iter().find_map(|other| {
        if new == other {
            Some(Overlap::Same(other))
        } else if new.starts_with(other) {
            Some(Overlap::Inside(other))
        } else if other.starts_with(new) {
            Some(Overlap::Contains(other))
        } else {
            None
        }
    })
}

/// The folders on disk that no movie uses, sorted. `known` holds the folder
/// names of the movies in the root folder. Names compare without regard to
/// case, so a library can move between filesystems.
pub fn unmapped(on_disk: impl IntoIterator<Item = String>, known: &[String]) -> Vec<String> {
    let known: Vec<String> = known.iter().map(|name| name.to_lowercase()).collect();
    let mut folders: Vec<String> = on_disk
        .into_iter()
        .filter(|name| !known.contains(&name.to_lowercase()))
        .collect();
    folders.sort();
    folders
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<std::path::PathBuf> {
        list.iter().map(Path::new).map(Path::to_path_buf).collect()
    }

    #[test]
    fn finds_the_same_nested_and_containing_folder() {
        let existing = paths(&["/media/movies", "/media/kids"]);
        let find = |new: &str| overlap(Path::new(new), existing.iter().map(|p| p.as_path()));
        assert_eq!(
            find("/media/movies"),
            Some(Overlap::Same(Path::new("/media/movies")))
        );
        assert_eq!(
            find("/media/movies/4k"),
            Some(Overlap::Inside(Path::new("/media/movies")))
        );
        assert_eq!(
            find("/media"),
            Some(Overlap::Contains(Path::new("/media/movies")))
        );
    }

    #[test]
    fn a_name_prefix_is_not_a_nested_folder() {
        let existing = paths(&["/media/movies"]);
        let found = overlap(
            Path::new("/media/movies2"),
            existing.iter().map(|p| p.as_path()),
        );
        assert_eq!(found, None);
        assert_eq!(overlap(Path::new("/x"), []), None);
    }

    #[test]
    fn unmapped_folders_are_those_without_a_movie() {
        let on_disk = ["Dune (2021)", "Stray", "alien (1979)"].map(String::from);
        let known = ["Alien (1979)".to_owned(), "Dune (2021)".to_owned()];
        assert_eq!(unmapped(on_disk, &known), ["Stray"]);
    }
}
