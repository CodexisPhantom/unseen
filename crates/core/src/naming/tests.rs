//! The expected values come from Radarr: the unit tests of
//! `NzbDrone.Core.Test/OrganizerTests` (develop branch) and the output of its
//! `FileNameBuilder` for its own sample movie (`FileNameSampleService`, the
//! source of `GET /api/v3/config/naming/examples`). They are not captured from
//! a running Radarr.

use std::path::{Component, Path};

use super::*;

fn movie(title: &str) -> MovieFacts {
    MovieFacts {
        title: title.to_owned(),
        ..MovieFacts::default()
    }
}

fn file(title: &str, version: u8) -> FileFacts {
    FileFacts {
        quality: Some(QualityFacts {
            title: title.to_owned(),
            version,
        }),
        release_group: Some("RadarrTest".to_owned()),
        ..FileFacts::default()
    }
}

fn config(file_template: &str, colon: ColonReplacement) -> NamingConfig {
    NamingConfig {
        movie_file: file_template.to_owned(),
        colon_replacement: colon,
        ..NamingConfig::default()
    }
}

/// The file name for `template` with Radarr's usual test file (HDTV-720p).
fn file_name(template: &str, title: &str) -> String {
    config(template, ColonReplacement::Smart).movie_file(&movie(title), &file("HDTV-720p", 1))
}

fn folder(template: &str, title: &str, year: i32) -> String {
    NamingConfig {
        movie_folder: template.to_owned(),
        ..NamingConfig::default()
    }
    .movie_folder(&MovieFacts {
        year,
        ..movie(title)
    })
}

#[test]
fn each_token_renders_what_radarr_renders_for_its_sample() {
    let (movie, file) = sample();
    for (token, expected) in [
        ("{Movie Title}", "The Movie - Title"),
        ("{Movie CleanTitle}", "The Movie Title"),
        ("{Movie TitleThe}", "Movie - Title, The"),
        ("{Movie OriginalTitle}", "The Original Movie Title"),
        ("{Movie TitleFirstCharacter}", "M"),
        ("{Movie Collection}", "The Movie Collection"),
        ("{Release Year}", "2010"),
        ("{ImdbId}", "tt0066921"),
        ("{TmdbId}", "345691"),
        ("{Quality Full}", "Bluray-1080p Proper"),
        ("{Quality Title}", "Bluray-1080p"),
        ("{MediaInfo Simple}", "x264 DTS"),
        ("{MediaInfo VideoCodec}", "x264"),
        ("{MediaInfo AudioCodec}", "DTS"),
        ("{MediaInfo AudioChannels}", "5.1"),
        ("{MediaInfo VideoDynamicRangeType}", "HLG"),
        ("{MediaInfo 3D}", "3D"),
        ("{Release Group}", "EVOLVE"),
        ("{Edition Tags}", "Ultimate Extended Edition"),
        (
            "{Original Title}",
            "The.Movie.Title.2010.1080p.BluRay.DTS.x264-EVOLVE",
        ),
        (
            "{Original Filename}",
            "The.Movie.Title.2010.1080p.BluRay.DTS.x264-EVOLVE",
        ),
        ("{Custom Formats}", "Surround Sound x264"),
    ] {
        let config = config(token, ColonReplacement::Smart);
        assert_eq!(config.movie_file(&movie, &file), expected, "{token}");
    }
}

#[test]
fn the_default_templates_render_radarrs_example_names() {
    let (movie, file) = sample();
    let config = NamingConfig::default();
    assert_eq!(config.movie_folder(&movie), "The Movie - Title (2010)");
    assert_eq!(
        config.movie_file(&movie, &file),
        "The Movie - Title (2010) Bluray-1080p Proper"
    );
}

#[test]
fn the_defaults_equal_radarrs() {
    let config = NamingConfig::default();
    assert_eq!(config.movie_folder, "{Movie Title} ({Release Year})");
    assert_eq!(
        config.movie_file,
        "{Movie Title} ({Release Year}) {Quality Full}"
    );
    assert!(!config.rename_movies);
    assert!(config.replace_illegal_characters);
    assert_eq!(config.colon_replacement, ColonReplacement::Smart);
}

#[test]
fn separators_and_case_follow_the_token_as_written() {
    for (template, expected) in [
        ("{Movie Title}", "South Park"),
        ("{Movie_Title}", "South_Park"),
        ("{Movie.Title}", "South.Park"),
        ("{Movie-Title}", "South-Park"),
        ("{MOVIE TITLE}", "SOUTH PARK"),
        ("{mOvIe-tItLE}", "South-Park"),
        ("{movie title}", "south park"),
        ("{MovieTitle}", "South Park"),
    ] {
        assert_eq!(file_name(template, "South Park"), expected, "{template}");
    }
}

#[test]
fn affixes_show_with_a_value_and_go_with_an_empty_one() {
    for (template, version, expected) in [
        (
            "{Movie.Title}.{_Quality.Title_}",
            1,
            "South.Park._HDTV-720p",
        ),
        (
            "{Movie Title} [{Quality Title}] {[Edition Tags]}",
            1,
            "South Park [HDTV-720p]",
        ),
        (
            "{Movie Title} [{Quality Title}] {[Quality Full]}",
            2,
            "South Park [HDTV-720p] [HDTV-720p Proper]",
        ),
        (
            "{Movie Title} [{Quality Full}]",
            2,
            "South Park [HDTV-720p Proper]",
        ),
        ("{Movie Title}{-Release Group}", 1, "South Park-RadarrTest"),
    ] {
        let name = config(template, ColonReplacement::Smart)
            .movie_file(&movie("South Park"), &file("HDTV-720p", version));
        assert_eq!(name, expected, "{template}");
    }
}

#[test]
fn a_missing_release_group_shows_a_name_only_without_affixes() {
    let config = |template| config(template, ColonReplacement::Smart);
    let no_group = FileFacts::default();
    let movie = movie("South Park");
    assert_eq!(
        config("{Release Group}").movie_file(&movie, &no_group),
        UNKNOWN_RELEASE_GROUP
    );
    for template in [
        "{Movie Title}{-Release Group}",
        "{Movie Title}{ Release Group}",
        "{Movie Title}{ [Release Group]}",
    ] {
        assert_eq!(
            config(template).movie_file(&movie, &no_group),
            "South Park",
            "{template}"
        );
    }
}

#[test]
fn separators_between_empty_tokens_are_trimmed() {
    for separator in [' ', '-', '.', '_'] {
        let end = format!("{{Quality{separator}Title}}{separator}{{Edition{separator}Tags}}");
        assert_eq!(file_name(&end, "South Park"), "HDTV-720p");
        let middle = format!(
            "{{Quality{separator}Title}}{separator}{{Edition{separator}Tags}}{separator}{{Movie{separator}Title}}"
        );
        assert_eq!(
            file_name(&middle, "South Park"),
            format!("HDTV-720p{separator}South{separator}Park")
        );
    }
}

#[test]
fn repeated_separators_merge_and_trailing_ones_go() {
    assert_eq!(file_name("{Movie.Title}.", "Chicago P.D."), "Chicago.P.D");
    assert_eq!(file_name("{Movie.Title}", "Venture Bros."), "Venture.Bros");
    assert_eq!(file_name("{Movie Title}", "Foo   Bar"), "Foo Bar");
}

#[test]
fn colons_follow_the_colon_replacement() {
    for (title, colon, expected) in [
        ("CSI: Vegas", ColonReplacement::Smart, "CSI - Vegas"),
        ("CSI: Vegas", ColonReplacement::Dash, "CSI- Vegas"),
        ("CSI: Vegas", ColonReplacement::Delete, "CSI Vegas"),
        ("CSI: Vegas", ColonReplacement::SpaceDash, "CSI - Vegas"),
        (
            "CSI: Vegas",
            ColonReplacement::SpaceDashSpace,
            "CSI - Vegas",
        ),
        ("Movie:Title", ColonReplacement::Smart, "Movie-Title"),
        ("Movie:Title", ColonReplacement::Dash, "Movie-Title"),
        ("Movie:Title", ColonReplacement::Delete, "MovieTitle"),
        ("Movie:Title", ColonReplacement::SpaceDash, "Movie -Title"),
        (
            "Movie:Title",
            ColonReplacement::SpaceDashSpace,
            "Movie - Title",
        ),
    ] {
        let name = config("{Movie Title}", colon).movie_file(&movie(title), &file("HDTV-720p", 1));
        assert_eq!(name, expected, "{title} {colon:?}");
    }
}

#[test]
fn illegal_characters_are_replaced_or_removed() {
    let title = "A\\B/C<D>E?F*G|H\"I";
    let render = |replace| {
        NamingConfig {
            movie_file: "{Movie Title}".to_owned(),
            replace_illegal_characters: replace,
            ..NamingConfig::default()
        }
        .movie_file(&movie(title), &FileFacts::default())
    };
    assert_eq!(render(true), "A+B+CDE!F-GHI");
    assert_eq!(render(false), "ABCDEFGHI");
}

#[test]
fn clean_file_name_trims_leading_dots_and_spaces() {
    for (name, expected) in [
        (".45 (2006)", "45 (2006)"),
        (" The Movie Title ", "The Movie Title"),
        (
            "Mission: Impossible - no [HDTV-720p]",
            "Mission - Impossible - no [HDTV-720p]",
        ),
    ] {
        assert_eq!(
            clean_file_name(name, true, ColonReplacement::Smart),
            expected
        );
    }
}

#[test]
fn clean_titles_follow_radarr() {
    for (title, expected) in [
        ("Florence + the Machine", "Florence + the Machine"),
        ("Beyoncé X10", "Beyonce X10"),
        (
            "Girlfriends' Guide to Divorce",
            "Girlfriends Guide to Divorce",
        ),
        (
            "Rule #23: Never Lie to the Kids",
            "Rule #23 Never Lie to the Kids",
        ),
        (
            "Anne Hathaway/Florence + The Machine",
            "Anne Hathaway Florence + The Machine",
        ),
        ("Chris Rock/Prince", "Chris Rock Prince"),
        ("Karma's a B*tch!", "Karmas a B-tch!"),
        (
            "Ke$ha: My Crazy Beautiful Life",
            "Ke$ha My Crazy Beautiful Life",
        ),
        ("$#*! My Dad Says", "$#-! My Dad Says"),
        ("Free! - Iwatobi Swim Club", "Free! Iwatobi Swim Club"),
        (
            "Tamara Ecclestone: Billion $$ Girl",
            "Tamara Ecclestone Billion $$ Girl",
        ),
        (
            "Marvel's Agents of S.H.I.E.L.D.",
            "Marvels Agents of S.H.I.E.L.D",
        ),
        ("Castle (2009)", "Castle 2009"),
        ("Law & Order (UK)", "Law and Order UK"),
        ("Is this okay?", "Is this okay"),
        ("[a] title", "a title"),
        ("backslash \\ backlash", "backslash backlash"),
        ("I'm the Boss", "Im the Boss"),
        ("I've Been Caught", "Ive Been Caught"),
        ("That'll Be The Day", "Thatll Be The Day"),
        ("I'd Rather Be Alone", "Id Rather Be Alone"),
        ("I Can't Die", "I Cant Die"),
        ("Won`t Get Fooled Again", "Wont Get Fooled Again"),
        ("Don’t Blink", "Dont Blink"),
        ("The ` Legend of Kings", "The Legend of Kings"),
        ("Joker: Folie à deux", "Joker Folie a deux"),
    ] {
        assert_eq!(file_name("{Movie CleanTitle}", title), expected, "{title}");
    }
}

#[test]
fn title_the_moves_the_article_before_a_trailing_year() {
    for (title, expected) in [
        ("The Mist", "Mist, The"),
        ("A Place to Call Home", "Place to Call Home, A"),
        (
            "An Adventure in Space and Time",
            "Adventure in Space and Time, An",
        ),
        ("The Flash (2010)", "Flash, The (2010)"),
        ("The Fixer (ZH) (2015)", "Fixer, The (ZH) (2015)"),
        ("The Rat Pack (A&E)", "Rat Pack, The (A&E)"),
        (
            "The Climax: I (Almost) Got Away With It (2016)",
            "Climax - I (Almost) Got Away With It, The (2016)",
        ),
        ("A", "A"),
        ("Anne", "Anne"),
        ("Theodore", "Theodore"),
        ("3%", "3%"),
    ] {
        assert_eq!(file_name("{Movie TitleThe}", title), expected, "{title}");
    }
}

#[test]
fn the_first_character_skips_one_symbol_and_loses_accents() {
    for (title, expected) in [
        ("The Mist", "M/The Mist"),
        ("A", "A/A"),
        ("30 Rock", "3/30 Rock"),
        ("The '80s Greatest", "8/The '80s Greatest"),
        ("좀비버스", "좀/좀비버스"),
        ("¡Mucha Lucha!", "M/¡Mucha Lucha!"),
        (".hack", "H/hack"),
        ("Ütopya", "U/Ütopya"),
        ("Æon Flux", "A/Æon Flux"),
    ] {
        let rendered = folder("{Movie TitleFirstCharacter}\\{Movie Title}", title, 0);
        assert_eq!(rendered, expected, "{title}");
    }
    assert_eq!(
        folder("{movie titlefirstcharacter}\\{movie title}", "Westworld", 0),
        "w/westworld"
    );
}

#[test]
fn folders_follow_radarrs_folder_tests() {
    for (title, year, template, expected) in [
        (
            "Arrival",
            2016,
            "{Movie Title} ({Release Year})",
            "Arrival (2016)",
        ),
        (
            "The Big Short",
            2015,
            "{Movie TitleThe} ({Release Year})",
            "Big Short, The (2015)",
        ),
        (
            "The Big Short",
            2015,
            "{Movie Title} ({Release Year})",
            "The Big Short (2015)",
        ),
        (
            "Movie Title",
            2015,
            "{Movie Title} ({ImdbId})",
            "Movie Title ()",
        ),
    ] {
        assert_eq!(folder(template, title, year), expected, "{template}");
    }
}

#[test]
fn id_tokens_keep_their_tag_only_with_a_value() {
    let with_ids = MovieFacts {
        imdb_id: Some("tt0000001".to_owned()),
        tmdb_id: 12,
        ..movie("Movie Title")
    };
    let render = |template: &str, movie: &MovieFacts| {
        NamingConfig {
            movie_folder: template.to_owned(),
            ..NamingConfig::default()
        }
        .movie_folder(movie)
    };
    assert_eq!(
        render("{Movie Title} {imdb-{ImdbId}}", &with_ids),
        "Movie Title {imdb-tt0000001}"
    );
    assert_eq!(
        render("{Movie Title} {{tmdb-{TmdbId}}}", &with_ids),
        "Movie Title {{tmdb-12}}"
    );
    for template in [
        "{Movie Title} {imdb-{ImdbId}}",
        "{Movie Title} {imdbid-{ImdbId}}",
        "{Movie Title} {{imdb-{ImdbId}}}",
    ] {
        assert_eq!(
            render(template, &movie("Movie Title")),
            "Movie Title",
            "{template}"
        );
    }
}

#[test]
fn edition_tags_follow_radarrs_casing() {
    for (edition, token, expected) in [
        ("Uncut", "{Edition Tags}", "Uncut"),
        (
            "1st anniversary edition",
            "{Edition Tags}",
            "1st Anniversary Edition",
        ),
        (
            "4th anNiverSary eDitIOn",
            "{Edition Tags}",
            "4th Anniversary Edition",
        ),
        (
            "10Th anniversary edition",
            "{edition tags}",
            "10th anniversary edition",
        ),
        (
            "10th anniversary edition",
            "{EDITION TAGS}",
            "10TH ANNIVERSARY EDITION",
        ),
        ("35MM EDITION", "{Edition Tags}", "35mm Edition"),
        ("imax", "{Edition Tags}", "IMAX"),
        ("Imax", "{edition tags}", "imax"),
        ("imax version", "{Edition Tags}", "IMAX Version"),
        ("IMAX-edition", "{Edition Tags}", "IMAX-Edition"),
        ("IMAX_edition", "{Edition Tags}", "IMAX_Edition"),
        ("IMAX.eDiTioN", "{Edition Tags}", "IMAX.Edition"),
        ("IMAX ed.", "{Edition Tags}", "IMAX Ed."),
        ("3d", "{Edition Tags}", "3D"),
        ("hdr", "{Edition Tags}", "HDR"),
        ("Dv", "{Edition Tags}", "DV"),
        ("sdr", "{Edition Tags}", "SDR"),
        ("THEATRICAL", "{Edition Tags}", "Theatrical"),
        ("director's CUt", "{Edition Tags}", "Director's Cut"),
    ] {
        let facts = FileFacts {
            edition: Some(edition.to_owned()),
            ..FileFacts::default()
        };
        let name = config(
            &format!("{{Movie Title}} [{token}]"),
            ColonReplacement::Smart,
        )
        .movie_file(&movie("Movie Title"), &facts);
        assert_eq!(name, format!("Movie Title [{expected}]"), "{edition}");
    }
    // Without an edition the braces go with the token.
    for template in [
        "{Movie Title} {Edition Tags}",
        "{Movie Title} {{Edition Tags}}",
        "{Movie Title} {edition-{Edition Tags}}",
    ] {
        assert_eq!(
            file_name(template, "Movie Title"),
            "Movie Title",
            "{template}"
        );
    }
    let uncut = FileFacts {
        edition: Some("Uncut".to_owned()),
        ..FileFacts::default()
    };
    assert_eq!(
        config("{Movie Title} {{Edition Tags}}", ColonReplacement::Smart)
            .movie_file(&movie("Movie Title"), &uncut),
        "Movie Title {Uncut}"
    );
}

#[test]
fn custom_formats_list_only_those_included_when_renaming() {
    let formats = |list: &[(&str, bool)]| FileFacts {
        custom_formats: list
            .iter()
            .map(|(name, include)| CustomFormatFacts {
                name: (*name).to_owned(),
                include_when_renaming: *include,
            })
            .collect(),
        ..FileFacts::default()
    };
    let config = config("{Custom Formats}", ColonReplacement::Smart);
    let facts = formats(&[
        ("INTERNAL", true),
        ("AMZN", true),
        ("NAME WITH SPACES", true),
        ("NotIncludedFormat", false),
    ]);
    assert_eq!(
        config.movie_file(&movie("South Park"), &facts),
        "INTERNAL AMZN NAME WITH SPACES"
    );
    assert_eq!(
        config.movie_file(&movie("South Park"), &formats(&[("Hidden", false)])),
        ""
    );
}

#[test]
fn original_title_and_filename_cannot_contain_themselves() {
    let movie = movie("My Movie");
    let named = |scene: Option<&str>| FileFacts {
        scene_name: scene.map(str::to_owned),
        original_file_name: Some("My Movie".to_owned()),
        ..FileFacts::default()
    };
    let render = |template: &str, facts: &FileFacts| {
        config(template, ColonReplacement::Smart).movie_file(&movie, facts)
    };
    // Several tokens: no file name, so the new name is not built from itself.
    assert_eq!(
        render("{Movie Title} {[Original Title]}", &named(None)),
        "My Movie"
    );
    assert_eq!(
        render(
            "{Movie Title} {[Original Title]}",
            &named(Some("my.movie.2008"))
        ),
        "My Movie [my.movie.2008]"
    );
    assert_eq!(render("{Original Title}", &named(None)), "My Movie");
    assert_eq!(render("{Original Filename}", &named(None)), "My Movie");
    assert_eq!(
        render("{Movie Title} {Original Filename}", &named(None)),
        "My Movie"
    );
}

#[test]
fn quality_tokens_are_empty_without_a_quality() {
    assert_eq!(
        config("{Movie Title} {Quality Full}", ColonReplacement::Smart)
            .movie_file(&movie("South Park"), &FileFacts::default()),
        "South Park"
    );
}

#[test]
fn media_info_tokens_are_empty_without_media_info() {
    let name = config(
        "{Movie Title} {MediaInfo 3D} {MediaInfo Simple}",
        ColonReplacement::Smart,
    )
    .movie_file(&movie("South Park"), &FileFacts::default());
    assert_eq!(name, "South Park");
    let facts = FileFacts {
        media: Some(MediaFacts {
            video_codec: "h264".to_owned(),
            audio_codec: "DTS".to_owned(),
            three_d: true,
            ..MediaFacts::default()
        }),
        ..FileFacts::default()
    };
    let name = config(
        "{Movie.Title}.{MEDIAINFO.3D}.{MediaInfo.Simple}",
        ColonReplacement::Smart,
    )
    .movie_file(&movie("South Park"), &facts);
    assert_eq!(name, "South.Park.3D.h264.DTS");
}

#[test]
fn reserved_device_names_get_an_underscore() {
    for (title, expected) in [
        ("Con.Game", "Con_Game (2021)"),
        ("Com1.Sat", "Com1_Sat (2021)"),
        ("Con Game", "Con Game (2021)"),
        ("Console", "Console (2021)"),
    ] {
        assert_eq!(
            folder("{Movie Title} ({Release Year})", title, 2021),
            expected
        );
    }
    // The whole name, with or without an extension to come.
    for (title, expected) in [
        ("CON", "CON_"),
        ("nul", "nul_"),
        ("Lpt9", "Lpt9_"),
        ("COM0", "COM0"),
        ("aux.mkv", "aux_mkv"),
    ] {
        assert_eq!(file_name("{Movie Title}", title), expected, "{title}");
    }
}

#[test]
fn trailing_dots_spaces_and_literal_forbidden_characters_never_stay() {
    for (template, title, expected) in [
        ("{Movie Title}...", "Foo", "Foo"),
        ("{Movie Title}: {Release Year}", "Foo", "Foo"),
        ("Fixed <name>?", "Foo", "Fixed name"),
        ("{Movie Title}  ", "Foo", "Foo"),
    ] {
        assert_eq!(file_name(template, title), expected, "{template}");
        assert_eq!(folder(template, title, 0), expected, "{template}");
    }
}

#[test]
fn a_template_with_separators_makes_sub_folders() {
    assert_eq!(
        folder(
            "{Movie Collection}\\{Movie Title} ({Release Year})",
            "Foo",
            1999
        ),
        "Foo (1999)"
    );
    let with_collection = MovieFacts {
        collection: Some("The Y-Women Collection".to_owned()),
        year: 2005,
        ..movie("The Y-Women 14")
    };
    let config = NamingConfig {
        movie_folder: "{Movie Collection}/{Movie Title} ({Release Year})".to_owned(),
        ..NamingConfig::default()
    };
    assert_eq!(
        config.movie_folder(&with_collection),
        "The Y-Women Collection/The Y-Women 14 (2005)"
    );
    // A slash in a value is a `+`, never a folder.
    assert_eq!(folder("{Movie Title}", "Face/Off", 0), "Face+Off");
}

#[test]
fn the_default_folder_name_matches_the_old_helper() {
    for (title, year, expected) in [
        ("The Matrix", 1999, "The Matrix (1999)"),
        (
            "Mission: Impossible - Fallout",
            2018,
            "Mission - Impossible - Fallout (2018)",
        ),
        (
            "Who Framed Roger Rabbit?",
            1988,
            "Who Framed Roger Rabbit! (1988)",
        ),
        ("Face/Off", 1997, "Face+Off (1997)"),
        ("S*P*Y*S", 1974, "S-P-Y-S (1974)"),
        ("\"Crocodile\" Dundee", 1986, "Crocodile Dundee (1986)"),
        ("<Title> | Two", 2020, "Title Two (2020)"),
        ("...And Justice for All", 1979, "And Justice for All (1979)"),
        ("Amélie", 2001, "Amélie (2001)"),
    ] {
        assert_eq!(movie_folder_name(title, year), expected, "{title}");
    }
}

#[test]
fn unknown_tokens_are_rejected_and_known_ones_pass() {
    for template in [
        DEFAULT_MOVIE_FOLDER,
        DEFAULT_MOVIE_FILE,
        "{Movie.Title}.{Release_Year}.{[Quality Full]}{-Release Group}",
        "{{Edition Tags}} {imdb-{ImdbId}} {MEDIAINFO.3D} {tmdbid}",
        "Plain text { only",
    ] {
        assert_eq!(validate_template(template), Ok(()), "{template}");
    }
    for (template, offender) in [
        ("{Movie Titel}", "{Movie Titel}"),
        ("{Movie Title} {Season}", "{Season}"),
        (
            "{Movie Title First Character}",
            "{Movie Title First Character}",
        ),
        ("{Movie Title} {}", "{}"),
        ("{Series Title}", "{Series Title}"),
    ] {
        assert_eq!(
            validate_template(template),
            Err(TemplateError::UnknownToken(offender.to_owned())),
            "{template}"
        );
    }
    assert_eq!(
        validate_template("{Movie Title:30}"),
        Err(TemplateError::UnsupportedModifier(
            "{Movie Title:30}".to_owned()
        ))
    );
    for template in ["", "  ", "/", "\\/"] {
        assert_eq!(validate_template(template), Err(TemplateError::Empty));
    }
}

#[test]
fn unknown_tokens_render_as_nothing() {
    assert_eq!(file_name("{Movie Title} {Nope Nope}", "Foo"), "Foo");
}

#[test]
fn colon_replacement_names_round_trip() {
    for value in ColonReplacement::ALL {
        assert_eq!(ColonReplacement::parse(value.as_str()), Some(value));
    }
    assert_eq!(ColonReplacement::parse("Delete"), None);
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

/// A small seeded generator; the workspace has no property test crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() % items.len() as u64) as usize]
    }

    fn text(&mut self, parts: &[&str], max: u64) -> String {
        (0..self.next() % max).map(|_| *self.pick(parts)).collect()
    }
}

const NASTY: &[&str] = &[
    "..",
    ".",
    "/",
    "\\",
    "../",
    "..\\",
    "/etc",
    "C:",
    "c:\\",
    "\\\\srv\\share",
    ":",
    "<",
    ">",
    "\"",
    "|",
    "?",
    "*",
    "\0",
    "\n",
    "\t",
    " ",
    "  ",
    "-",
    "_",
    "CON",
    "nul",
    "Com1",
    "lpt9",
    "con.",
    "aux.mkv",
    "The ",
    "A ",
    "'",
    "’",
    "(",
    ")",
    "{",
    "}",
    "é",
    "Æ",
    "좀",
    "💥",
    "a",
    "B",
    "7",
    "...",
    "~",
    "%2e%2e",
    "\u{202e}",
];

const FRAGMENTS: &[&str] = &[
    "{Movie Title}",
    "{Movie CleanTitle}",
    "{Movie TitleThe}",
    "{Movie OriginalTitle}",
    "{Movie TitleFirstCharacter}",
    "{Movie Collection}",
    "{Release Year}",
    "{ImdbId}",
    "{TmdbId}",
    "{Quality Full}",
    "{Quality Title}",
    "{MediaInfo Simple}",
    "{MediaInfo 3D}",
    "{Release Group}",
    "{Edition Tags}",
    "{Original Title}",
    "{Original Filename}",
    "{Custom Formats}",
    "{Movie.Title}",
    "{movie_title}",
    "{[Quality Full]}",
    "{-Release Group}",
    "{{Edition Tags}}",
    "{imdb-{ImdbId}}",
    "/",
    "\\",
    "..",
    "../",
    "..\\",
    "...",
    ".",
    " ",
    "-",
    "_",
    ":",
    "CON",
    "nul.",
    "{",
    "}",
    "<x>",
    "é",
];

#[test]
fn a_rendered_name_never_leaves_its_root_folder() {
    let root = Path::new("library");
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..6000 {
        let nasty = |rng: &mut Rng| rng.text(NASTY, 8);
        let facts = MovieFacts {
            title: nasty(&mut rng),
            original_title: Some(nasty(&mut rng)),
            year: (rng.next() % 3000) as i32,
            collection: Some(nasty(&mut rng)),
            imdb_id: Some(nasty(&mut rng)),
            tmdb_id: (rng.next() % 100_000) as i32,
        };
        let file = FileFacts {
            quality: Some(QualityFacts {
                title: nasty(&mut rng),
                version: (rng.next() % 3) as u8,
            }),
            media: Some(MediaFacts {
                video_codec: nasty(&mut rng),
                audio_codec: nasty(&mut rng),
                audio_channels: nasty(&mut rng),
                video_dynamic_range_type: nasty(&mut rng),
                three_d: rng.next().is_multiple_of(2),
            }),
            release_group: Some(nasty(&mut rng)),
            edition: Some(nasty(&mut rng)),
            scene_name: Some(nasty(&mut rng)),
            original_file_name: Some(nasty(&mut rng)),
            custom_formats: vec![CustomFormatFacts {
                name: nasty(&mut rng),
                include_when_renaming: true,
            }],
        };
        let config = NamingConfig {
            movie_folder: rng.text(FRAGMENTS, 7),
            movie_file: rng.text(FRAGMENTS, 7),
            rename_movies: true,
            replace_illegal_characters: rng.next().is_multiple_of(2),
            colon_replacement: *rng.pick(&ColonReplacement::ALL),
        };
        let folder = config.movie_folder(&facts);
        let name = config.movie_file(&facts, &file);
        for rendered in [&folder, &name] {
            check_safe(rendered, root, case, &config, &facts);
        }
        let full = root.join(&folder).join(format!("{name}.mkv"));
        assert!(full.starts_with(root), "case {case}: {full:?}");
    }
}

fn check_safe(rendered: &str, root: &Path, case: usize, config: &NamingConfig, facts: &MovieFacts) {
    let context = format!("case {case}: {rendered:?} from {config:?} and {facts:?}");
    for part in rendered.split('/').filter(|_| !rendered.is_empty()) {
        assert!(!part.trim().is_empty(), "empty part, {context}");
        assert_ne!(part, ".", "{context}");
        assert_ne!(part, "..", "{context}");
        assert!(
            !part
                .chars()
                .any(|c| "<>:\"|?*\\/".contains(c) || c.is_control()),
            "forbidden character, {context}"
        );
        assert!(
            !part.ends_with(['.', ' ']),
            "trailing dot or space, {context}"
        );
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_ascii_lowercase();
        assert!(
            !matches!(stem.as_str(), "aux" | "con" | "nul" | "prn")
                && !(stem.len() == 4
                    && (stem.starts_with("com") || stem.starts_with("lpt"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9')),
            "reserved name, {context}"
        );
    }
    let joined = root.join(rendered);
    assert!(
        joined
            .strip_prefix(root)
            .expect("joined path starts with the root")
            .components()
            .all(|c| matches!(c, Component::Normal(_))),
        "path escapes, {context}"
    );
}
