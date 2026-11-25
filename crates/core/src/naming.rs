//! Names that Unseen derives from movie metadata: the movie naming templates
//! of docs/09-filesystem.md ("Naming templates").
//!
//! The engine follows Radarr's `FileNameBuilder` (develop branch): the same
//! token syntax (separator and case variants, prefix and suffix inside the
//! braces), the same cleaning of token values and the same trimming of the
//! result. On top of that the result is safe on every platform: characters
//! that Windows forbids, trailing dots and spaces and reserved device names
//! never reach a path, and a rendered name can never leave its root folder.

use std::fmt;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

#[cfg(test)]
mod tests;

/// Radarr's default movie folder template.
pub const DEFAULT_MOVIE_FOLDER: &str = "{Movie Title} ({Release Year})";
/// Radarr's default movie file template.
pub const DEFAULT_MOVIE_FILE: &str = "{Movie Title} ({Release Year}) {Quality Full}";
/// What `{Release Group}` renders when the file has no group and the token
/// has no prefix or suffix. Radarr writes its own name here.
pub const UNKNOWN_RELEASE_GROUP: &str = "Unseen";

/// How Radarr's "colon replacement" setting writes a `:` in a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColonReplacement {
    /// `A: B` becomes `A B`.
    Delete,
    /// `A: B` becomes `A- B`.
    Dash,
    /// `A: B` becomes `A - B`; `A:B` becomes `A -B`.
    SpaceDash,
    /// `A: B` becomes `A - B`; `A:B` becomes `A - B`.
    SpaceDashSpace,
    /// `A: B` becomes `A - B`; `A:B` becomes `A-B`. Radarr's default.
    #[default]
    Smart,
}

impl ColonReplacement {
    pub const ALL: [Self; 5] = [
        Self::Delete,
        Self::Dash,
        Self::SpaceDash,
        Self::SpaceDashSpace,
        Self::Smart,
    ];

    /// The name in the API (the same words as Radarr's JSON, in camel case).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Dash => "dash",
            Self::SpaceDash => "spaceDash",
            Self::SpaceDashSpace => "spaceDashSpace",
            Self::Smart => "smart",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.as_str() == text)
    }
}

/// The movie naming settings (docs/10-configuration.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamingConfig {
    pub movie_folder: String,
    pub movie_file: String,
    /// Off: an imported file keeps its original name.
    pub rename_movies: bool,
    /// Off: illegal characters are removed instead of replaced.
    pub replace_illegal_characters: bool,
    pub colon_replacement: ColonReplacement,
}

impl Default for NamingConfig {
    fn default() -> Self {
        Self {
            movie_folder: DEFAULT_MOVIE_FOLDER.to_owned(),
            movie_file: DEFAULT_MOVIE_FILE.to_owned(),
            rename_movies: false,
            replace_illegal_characters: true,
            colon_replacement: ColonReplacement::default(),
        }
    }
}

/// What the movie tokens read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MovieFacts {
    pub title: String,
    pub original_title: Option<String>,
    /// `0` renders as nothing.
    pub year: i32,
    pub collection: Option<String>,
    pub imdb_id: Option<String>,
    pub tmdb_id: i32,
}

/// The quality of a file as `{Quality ...}` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityFacts {
    /// The title of the quality definition, for example `Bluray-1080p`.
    pub title: String,
    /// The revision: 2 and up is a proper or repack.
    pub version: u8,
}

/// The `{MediaInfo ...}` values as text, in Radarr's spelling (`x264`,
/// `DTS`, `5.1`, `HDR10`). Until a media probe exists (docs/09, open
/// question) the caller fills them from the release name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaFacts {
    pub video_codec: String,
    pub audio_codec: String,
    pub audio_channels: String,
    pub video_dynamic_range_type: String,
    pub three_d: bool,
}

/// A custom format that matched the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomFormatFacts {
    pub name: String,
    /// The format's `includeWhenRenaming` flag. Only these show in
    /// `{Custom Formats}`.
    pub include_when_renaming: bool,
}

/// What the file tokens read. The movie folder has no file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileFacts {
    pub quality: Option<QualityFacts>,
    pub media: Option<MediaFacts>,
    pub release_group: Option<String>,
    pub edition: Option<String>,
    /// The release name the file came from.
    pub scene_name: Option<String>,
    /// The name of the file on disk, without the extension.
    pub original_file_name: Option<String>,
    pub custom_formats: Vec<CustomFormatFacts>,
}

/// Why a template cannot be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// Nothing is left of the template.
    Empty,
    /// The text between braces is not a token of this version.
    UnknownToken(String),
    /// Radarr's `:` options (`{Movie Title:30}`) are not supported yet.
    UnsupportedModifier(String),
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("must not be empty"),
            Self::UnknownToken(text) => write!(f, "has an unknown token {text}"),
            Self::UnsupportedModifier(text) => {
                write!(f, "uses {text}, but the `:` options are not supported")
            }
        }
    }
}

impl std::error::Error for TemplateError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    MovieTitle,
    MovieCleanTitle,
    MovieTitleThe,
    MovieOriginalTitle,
    MovieTitleFirstCharacter,
    MovieCollection,
    ReleaseYear,
    ImdbId,
    TmdbId,
    QualityFull,
    QualityTitle,
    MediaInfoSimple,
    MediaInfoVideoCodec,
    MediaInfoAudioCodec,
    MediaInfoAudioChannels,
    MediaInfoVideoDynamicRangeType,
    MediaInfo3d,
    ReleaseGroup,
    EditionTags,
    OriginalTitle,
    OriginalFilename,
    CustomFormats,
}

/// Radarr compares token names without case and without the separators
/// `- . _` and space, so `{Movie.Title}`, `{movie_title}` and `{MovieTitle}`
/// are one token.
const TOKENS: [(&str, Token); 22] = [
    ("movietitle", Token::MovieTitle),
    ("moviecleantitle", Token::MovieCleanTitle),
    ("movietitlethe", Token::MovieTitleThe),
    ("movieoriginaltitle", Token::MovieOriginalTitle),
    ("movietitlefirstcharacter", Token::MovieTitleFirstCharacter),
    ("moviecollection", Token::MovieCollection),
    ("releaseyear", Token::ReleaseYear),
    ("imdbid", Token::ImdbId),
    ("tmdbid", Token::TmdbId),
    ("qualityfull", Token::QualityFull),
    ("qualitytitle", Token::QualityTitle),
    ("mediainfosimple", Token::MediaInfoSimple),
    ("mediainfovideocodec", Token::MediaInfoVideoCodec),
    ("mediainfoaudiocodec", Token::MediaInfoAudioCodec),
    ("mediainfoaudiochannels", Token::MediaInfoAudioChannels),
    (
        "mediainfovideodynamicrangetype",
        Token::MediaInfoVideoDynamicRangeType,
    ),
    ("mediainfo3d", Token::MediaInfo3d),
    ("releasegroup", Token::ReleaseGroup),
    ("editiontags", Token::EditionTags),
    ("originaltitle", Token::OriginalTitle),
    ("originalfilename", Token::OriginalFilename),
    ("customformats", Token::CustomFormats),
];

impl Token {
    fn find(written: &str) -> Option<Self> {
        let key: String = written
            .chars()
            .filter(|c| !matches!(c, '-' | ' ' | '.' | '_'))
            .flat_map(char::to_lowercase)
            .collect();
        TOKENS
            .iter()
            .find_map(|(name, token)| (*name == key).then_some(*token))
    }
}

/// Radarr's `TitleRegex`. The optional `tag` is what makes
/// `{imdb-{ImdbId}}` and `{{Edition Tags}}` work: braces in the prefix and
/// suffix are kept when the value is not empty and dropped with it when it is.
static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)(?P<tag>\{[-{ ._\[(]*(?:imdb(?:id)?-|edition-))?",
        r"\{(?P<prefix>[-{ ._\[(]*)",
        r"(?P<token>[a-z0-9]+(?:(?P<separator>[- ._]+)[a-z0-9]+)?)",
        r"(?::(?P<modifier>[ ,a-z0-9|+-]*[,a-z0-9|+]))?",
        r"(?P<suffix>[-} ._)\]]*)\}",
    ))
    .expect("static regex is valid")
});

/// A `{...}` group the token regex did not take: a typo, not a token.
static LEFTOVER_GROUP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{[^{}]*\}").expect("static regex is valid"));

static TITLE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(The|An|A) (.*?)((?: *\([^)]+\))*)$").expect("static regex is valid")
});

static ORDINAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)((?:\b|_)\d{1,3}(?:st|th|rd|nd|mm)(?:\b|_))").expect("static regex is valid")
});

static EDITION_ACRONYM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)((?:\b|_)(?:IMAX|3D|SDR|HDR|DV)(?:\b|_))").expect("static regex is valid")
});

/// Checks that a template holds only known tokens. Run it before a template
/// is saved; [`NamingConfig::movie_folder`] and [`NamingConfig::movie_file`]
/// render an unknown token as nothing, as Radarr does.
pub fn validate_template(template: &str) -> Result<(), TemplateError> {
    if components(template).all(|part| part.trim().is_empty()) {
        return Err(TemplateError::Empty);
    }
    let mut end = 0;
    for caps in TOKEN.captures_iter(template) {
        let whole = caps.get(0).expect("group 0 always exists");
        check_gap(&template[end..whole.start()])?;
        end = whole.end();
        if Token::find(&caps["token"]).is_none() {
            return Err(TemplateError::UnknownToken(whole.as_str().to_owned()));
        }
        if caps.name("modifier").is_some() {
            return Err(TemplateError::UnsupportedModifier(
                whole.as_str().to_owned(),
            ));
        }
    }
    check_gap(&template[end..])
}

fn check_gap(text: &str) -> Result<(), TemplateError> {
    match LEFTOVER_GROUP.find(text) {
        Some(group) => Err(TemplateError::UnknownToken(group.as_str().to_owned())),
        None => Ok(()),
    }
}

/// The path parts of a template. Radarr splits before it replaces tokens, so
/// a `/` inside a value never makes a folder.
fn components(template: &str) -> impl Iterator<Item = &str> {
    template.split(['\\', '/']).filter(|part| !part.is_empty())
}

impl NamingConfig {
    /// The folder of a movie, relative to its root folder. Parts are joined
    /// with `/`. Empty when nothing is left of the template.
    pub fn movie_folder(&self, movie: &MovieFacts) -> String {
        self.render(&self.movie_folder, movie, None)
    }

    /// The file name of a movie without its extension. Not affected by
    /// `rename_movies`: that setting decides whether the caller uses it.
    pub fn movie_file(&self, movie: &MovieFacts, file: &FileFacts) -> String {
        self.render(&self.movie_file, movie, Some(file))
    }

    fn render(&self, template: &str, movie: &MovieFacts, file: Option<&FileFacts>) -> String {
        let context = Context {
            movie,
            file,
            multiple_tokens: TOKEN.find_iter(template).count() > 1,
        };
        let parts: Vec<String> = components(template)
            .map(|part| {
                let replaced = TOKEN
                    .replace_all(part, |caps: &Captures| self.replace_token(caps, &context))
                    .into_owned();
                let replaced = strip_forbidden(&replaced);
                if file.is_some() {
                    file_component(&replaced)
                } else {
                    folder_component(&replaced)
                }
            })
            .map(|part| reserved_device_name(&part))
            .filter(|part| !part.trim().is_empty())
            .collect();
        parts.join("/")
    }

    fn replace_token(&self, caps: &Captures, context: &Context) -> String {
        let token = &caps["token"];
        let group = |name| caps.name(name).map_or("", |m| m.as_str());
        let (tag, prefix, separator, suffix) = (
            group("tag"),
            group("prefix"),
            group("separator"),
            group("suffix"),
        );

        let value = Token::find(token)
            .map(|found| context.value(found, prefix.is_empty() && suffix.is_empty()))
            .unwrap_or_default();
        let mut text = value.trim().to_owned();

        if token
            .chars()
            .all(|c| !c.is_alphabetic() || c.is_lowercase())
        {
            text = text.to_lowercase();
        } else if token
            .chars()
            .all(|c| !c.is_alphabetic() || c.is_uppercase())
        {
            text = text.to_uppercase();
        }
        if !separator.trim().is_empty() {
            text = text.replace(' ', separator);
        }
        let text = self.clean_file_name(&text);
        if text.trim().is_empty() {
            String::new()
        } else {
            format!("{tag}{prefix}{text}{suffix}")
        }
    }

    /// Radarr's `CleanFileName` for the value of one token.
    fn clean_file_name(&self, name: &str) -> String {
        clean_file_name(
            name,
            self.replace_illegal_characters,
            self.colon_replacement,
        )
    }
}

/// Radarr's `CleanFileName`: the colon rule, then the other illegal
/// characters (`\ /` become `+`, `?` becomes `!`, `*` becomes `-`, `< > | "`
/// go), then leading spaces and dots and trailing spaces go. With `replace`
/// off every one of them is removed.
fn clean_file_name(name: &str, replace: bool, colon: ColonReplacement) -> String {
    let mut result = if !replace {
        name.replace(':', "")
    } else {
        match colon {
            ColonReplacement::Smart => name.replace(": ", " - ").replace(':', "-"),
            ColonReplacement::Delete => name.replace(':', ""),
            ColonReplacement::Dash => name.replace(':', "-"),
            ColonReplacement::SpaceDash => name.replace(':', " -"),
            ColonReplacement::SpaceDashSpace => name.replace(':', " - "),
        }
    };
    for (bad, good) in [
        ('\\', "+"),
        ('/', "+"),
        ('<', ""),
        ('>', ""),
        ('?', "!"),
        ('*', "-"),
        ('|', ""),
        ('"', ""),
    ] {
        result = result.replace(bad, if replace { good } else { "" });
    }
    result
        .trim_start_matches([' ', '.'])
        .trim_end_matches(' ')
        .to_owned()
}

/// Removes what no platform accepts in a name, from the literal parts of a
/// template too (token values are clean already). Radarr leaves a literal
/// `:` in a template alone; Windows cannot hold it.
fn strip_forbidden(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\') && !c.is_control()
        })
        .collect()
}

/// `([- ._])\1+` becomes the one character.
fn collapse_separators(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous = None;
    for c in text.chars() {
        if matches!(c, '-' | ' ' | '.' | '_') && previous == Some(c) {
            continue;
        }
        out.push(c);
        previous = Some(c);
    }
    out
}

/// A part of a file name: trimmed, repeated separators merged, separators at
/// the end removed (which also removes trailing dots and spaces).
fn file_component(text: &str) -> String {
    collapse_separators(text.trim())
        .trim_end_matches(['-', ' ', '.', '_'])
        .to_owned()
}

/// A part of a folder name, as Radarr's `CleanFolderName`.
fn folder_component(text: &str) -> String {
    collapse_separators(text)
        .trim_matches([' ', '.'])
        .to_owned()
}

/// Windows cannot hold a file or folder named like a device (`CON`,
/// `NUL.txt`). Radarr writes `Con_Game` for `Con.Game`; a name that is the
/// device name alone gets a `_` after it.
fn reserved_device_name(part: &str) -> String {
    let (stem, rest) = match part.split_once('.') {
        Some((stem, rest)) => (stem, Some(rest)),
        None => (part, None),
    };
    let lower = stem.trim_end().to_ascii_lowercase();
    let numbered = |prefix: &str| {
        lower
            .strip_prefix(prefix)
            .is_some_and(|digit| matches!(digit.as_bytes(), [b'1'..=b'9']))
    };
    let reserved = matches!(lower.as_str(), "aux" | "con" | "nul" | "prn")
        || numbered("com")
        || numbered("lpt");
    match (reserved, rest) {
        (false, _) => part.to_owned(),
        (true, Some(rest)) => format!("{stem}_{rest}"),
        (true, None) => format!("{stem}_"),
    }
}

fn not_blank(text: &Option<String>) -> Option<&str> {
    text.as_deref().filter(|text| !text.trim().is_empty())
}

struct Context<'a> {
    movie: &'a MovieFacts,
    file: Option<&'a FileFacts>,
    /// Radarr leaves `{Original Filename}` empty when the template has more
    /// than one token, so the new name cannot contain itself.
    multiple_tokens: bool,
}

impl Context<'_> {
    /// The raw value of a token; empty when the facts have nothing for it.
    /// `bare` is true when the token has no prefix and no suffix.
    fn value(&self, token: Token, bare: bool) -> String {
        let movie = self.movie;
        let file = self.file;
        let media = || file.and_then(|f| f.media.as_ref());
        match token {
            Token::MovieTitle => movie.title.clone(),
            Token::MovieCleanTitle => clean_title(&movie.title),
            Token::MovieTitleThe => title_the(&movie.title),
            Token::MovieOriginalTitle => movie.original_title.clone().unwrap_or_default(),
            Token::MovieTitleFirstCharacter => title_first_character(&title_the(&movie.title)),
            Token::MovieCollection => movie.collection.clone().unwrap_or_default(),
            Token::ReleaseYear if movie.year == 0 => String::new(),
            Token::ReleaseYear => movie.year.to_string(),
            Token::ImdbId => movie.imdb_id.clone().unwrap_or_default(),
            Token::TmdbId => movie.tmdb_id.to_string(),
            Token::QualityFull | Token::QualityTitle => {
                let Some(quality) = file.and_then(|f| f.quality.as_ref()) else {
                    return String::new();
                };
                if token == Token::QualityTitle || quality.version <= 1 {
                    quality.title.clone()
                } else {
                    format!("{} Proper", quality.title)
                }
            }
            Token::MediaInfoSimple => media()
                .map(|m| format!("{} {}", m.video_codec, m.audio_codec))
                .unwrap_or_default(),
            Token::MediaInfoVideoCodec => {
                media().map(|m| m.video_codec.clone()).unwrap_or_default()
            }
            Token::MediaInfoAudioCodec => {
                media().map(|m| m.audio_codec.clone()).unwrap_or_default()
            }
            Token::MediaInfoAudioChannels => media()
                .map(|m| m.audio_channels.clone())
                .unwrap_or_default(),
            Token::MediaInfoVideoDynamicRangeType => media()
                .map(|m| m.video_dynamic_range_type.clone())
                .unwrap_or_default(),
            Token::MediaInfo3d => match media() {
                Some(m) if m.three_d => "3D".to_owned(),
                _ => String::new(),
            },
            Token::ReleaseGroup => {
                let Some(file) = file else {
                    return String::new();
                };
                match not_blank(&file.release_group) {
                    Some(group) => group.to_owned(),
                    None if bare => UNKNOWN_RELEASE_GROUP.to_owned(),
                    None => String::new(),
                }
            }
            Token::EditionTags => file
                .and_then(|f| not_blank(&f.edition))
                .map(edition_tags)
                .unwrap_or_default(),
            Token::OriginalTitle => {
                let Some(file) = file else {
                    return String::new();
                };
                // Radarr cleans this with its default settings, whatever the
                // user chose; the token's own cleaning follows.
                let clean = |text: &str| clean_file_name(text, true, ColonReplacement::default());
                match not_blank(&file.scene_name) {
                    Some(scene) => clean(scene),
                    None => clean(&self.original_file_name()),
                }
            }
            Token::OriginalFilename => self.original_file_name(),
            Token::CustomFormats => file
                .map(|f| {
                    f.custom_formats
                        .iter()
                        .filter(|format| format.include_when_renaming)
                        .map(|format| format.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default(),
        }
    }

    fn original_file_name(&self) -> String {
        if self.multiple_tokens {
            return String::new();
        }
        self.file
            .and_then(|f| f.original_file_name.clone())
            .unwrap_or_default()
    }
}

/// `{Movie CleanTitle}`: the title as a scene release writes it. Quotes
/// before `s`, `t` and the like and brackets go, `&` becomes `and`, `/`
/// becomes a space and accents go.
fn clean_title(title: &str) -> String {
    let text: Vec<char> = title
        .replace('&', "and")
        .replace('/', " ")
        .chars()
        .collect();
    let follows = |index: usize, words: &[&str]| {
        let rest = &text[index + 1..];
        match rest.first() {
            None => true,
            Some(next) if next.is_whitespace() => true,
            Some(_) => words.iter().any(|word| {
                let word: Vec<char> = word.chars().collect();
                rest.len() > word.len()
                    && rest[..word.len()]
                        .iter()
                        .zip(&word)
                        .all(|(a, b)| a.to_ascii_lowercase() == *b)
                    && rest[word.len()].is_whitespace()
            }),
        }
    };
    let kept: String = text
        .iter()
        .enumerate()
        .filter(|(index, c)| {
            let space_before = *index > 0 && text[index - 1].is_whitespace();
            let space_after = text.get(index + 1).is_some_and(|n| n.is_whitespace());
            let lonely_mark =
                space_before && space_after && ",<>/\\;:'\"|`’~!?@%*-_=".contains(**c);
            let contraction = matches!(**c, '\'' | '`' | '’' | ':' | '?' | ',')
                && follows(*index, &["s", "m", "t", "ve", "ll", "d", "re"]);
            let bracket = matches!(**c, '(' | ')' | '[' | ']' | '{' | '}');
            !(lonely_mark || contraction || bracket)
        })
        .map(|(_, c)| *c)
        .collect();
    remove_diacritics(&kept)
}

/// `The Matrix` becomes `Matrix, The`; a trailing `(2010)` stays last.
fn title_the(title: &str) -> String {
    if title.trim().is_empty() {
        return String::new();
    }
    TITLE_PREFIX.replace(title, "${2}, ${1}${3}").into_owned()
}

/// The first letter or digit of the first two characters, in upper case and
/// without accent; `_` when there is none.
fn title_first_character(title: &str) -> String {
    title
        .chars()
        .take(2)
        .find(|c| c.is_alphanumeric())
        .and_then(|c| {
            let upper: String = c.to_uppercase().collect();
            remove_diacritics(&upper).chars().next()
        })
        .map_or_else(|| "_".to_owned(), String::from)
}

/// `Beyoncé` becomes `Beyonce`. Only Latin letters with accents change; other
/// scripts stay as they are.
fn remove_diacritics(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        match c {
            'Æ' => out.push_str("AE"),
            'æ' => out.push_str("ae"),
            'Œ' => out.push_str("OE"),
            'œ' => out.push_str("oe"),
            'ß' => out.push_str("ss"),
            'Ø' => out.push('O'),
            'ø' => out.push('o'),
            'Ł' => out.push('L'),
            'ł' => out.push('l'),
            'Đ' | 'Ð' => out.push('D'),
            'đ' | 'ð' => out.push('d'),
            _ => {
                let mut parts = c.nfd();
                match parts.next() {
                    Some(base) if base.is_ascii_alphabetic() && parts.all(is_combining_mark) => {
                        out.push(base);
                    }
                    _ => out.push(c),
                }
            }
        }
    }
    out
}

/// `{Edition Tags}`: title case, with `1st`, `35mm` in lower case and `IMAX`,
/// `3D`, `SDR`, `HDR`, `DV` in upper case.
fn edition_tags(edition: &str) -> String {
    let mut titled = String::with_capacity(edition.len());
    let mut in_word = false;
    for c in edition.to_lowercase().chars() {
        if c.is_alphabetic() {
            if in_word {
                titled.push(c);
            } else {
                titled.extend(c.to_uppercase());
                in_word = true;
            }
        } else {
            in_word = c == '\'' && in_word;
            titled.push(c);
        }
    }
    let titled = ORDINAL.replace_all(&titled, |caps: &Captures| caps[1].to_lowercase());
    EDITION_ACRONYM
        .replace_all(&titled, |caps: &Captures| caps[1].to_uppercase())
        .into_owned()
}

/// The folder of a movie with the default template.
pub fn movie_folder_name(title: &str, year: i32) -> String {
    NamingConfig::default().movie_folder(&MovieFacts {
        title: title.to_owned(),
        year,
        ..MovieFacts::default()
    })
}

/// The movie and file of Radarr's naming examples
/// (`FileNameSampleService`), for the preview in settings.
pub fn sample() -> (MovieFacts, FileFacts) {
    let scene = "The.Movie.Title.2010.1080p.BluRay.DTS.x264-EVOLVE";
    let movie = MovieFacts {
        title: "The Movie: Title".to_owned(),
        original_title: Some("The Original Movie Title".to_owned()),
        year: 2010,
        collection: Some("The Movie Collection".to_owned()),
        imdb_id: Some("tt0066921".to_owned()),
        tmdb_id: 345_691,
    };
    let file = FileFacts {
        quality: Some(QualityFacts {
            title: "Bluray-1080p".to_owned(),
            version: 2,
        }),
        media: Some(MediaFacts {
            video_codec: "x264".to_owned(),
            audio_codec: "DTS".to_owned(),
            audio_channels: "5.1".to_owned(),
            video_dynamic_range_type: "HLG".to_owned(),
            three_d: true,
        }),
        release_group: Some("EVOLVE".to_owned()),
        edition: Some("Ultimate extended edition".to_owned()),
        scene_name: Some(scene.to_owned()),
        original_file_name: Some(scene.to_owned()),
        custom_formats: ["Surround Sound", "x264"]
            .map(|name| CustomFormatFacts {
                name: name.to_owned(),
                include_when_renaming: true,
            })
            .to_vec(),
    };
    (movie, file)
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
