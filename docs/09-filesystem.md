# 09 File handling

Status: reviewed (2026-11-16). Movies, with the TV rules in their own section
below (M7).

## Folder layout

```
/data
  /torrents/<category>/...     written by qBittorrent
  /media/movies/<Movie Folder>/<Movie File>.<ext>
```

Downloads and library must be on the same filesystem and visible to Unseen
under one mount (for example `/data`). Otherwise hardlinks are impossible and
Unseen copies instead. Unseen warns at startup and in settings when a root
folder and a download path are on different filesystems.

When Unseen and qBittorrent see the same folder under different paths, a
remote path mapping per download client translates them, as in Radarr.

## Root folders

`GET/POST /api/v1/root-folders`, `DELETE /api/v1/root-folders/{id}`. Reading
needs `settings:read`; adding, deleting and the folder browser
(`GET /api/v1/filesystem`) need `settings:manage`.

A new root folder must be an absolute path that exists, is a directory,
and is writable (Unseen creates and removes a probe file). It must not be
the same as, lie inside, or contain another root folder; the comparison uses
whole path components on the resolved paths. A failed check is a 422 on the
field `path`; the same folder twice is a 409. The response carries the free
space of the filesystem and the sub-folders on disk that no movie uses. A
root folder that holds movies cannot be deleted (409). Deleting never touches
the disk.

## Naming templates

Defaults equal the Radarr defaults:

| Setting | Default |
|---|---|
| Movie folder | `{Movie Title} ({Release Year})` |
| Movie file | `{Movie Title} ({Release Year}) {Quality Full}` |
| Rename movies | off (keep the original file name), as in Radarr |
| Replace illegal characters | on |
| Colon replacement | Delete (Radarr's default; `core::naming` implements it for the folder name, still to verify against a real Radarr) |

Supported tokens in V1. The syntax is Radarr's, including the separator
variants (`{Movie.Title}`, `{Movie_Title}`), case variants and the optional
prefix/suffix inside braces (`{[Quality Full]}`, `{-Release Group}`).

| Group | Tokens |
|---|---|
| Movie | `{Movie Title}`, `{Movie CleanTitle}`, `{Movie TitleThe}`, `{Movie OriginalTitle}`, `{Movie TitleFirstCharacter}`, `{Movie Collection}`, `{Release Year}` |
| Ids | `{ImdbId}`, `{TmdbId}` |
| Quality | `{Quality Full}`, `{Quality Title}` |
| Media info | `{MediaInfo Simple}`, `{MediaInfo VideoCodec}`, `{MediaInfo AudioCodec}`, `{MediaInfo AudioChannels}`, `{MediaInfo VideoDynamicRangeType}`, `{MediaInfo 3D}` |
| Release | `{Release Group}`, `{Edition Tags}`, `{Original Title}`, `{Original Filename}` |
| Other | `{Custom Formats}` lists the matching custom formats that have `includeWhenRenaming` |

An unknown token is a validation error when the template is saved. The
expected output for each token is tested against what Radarr produces.

Open question: `MediaInfo` tokens need a media probe (ffprobe or a Rust
library). Decide in M4; until then those tokens render from the release name.

## Adopting an existing library

1. The user picks a root folder and starts a scan.
2. For each sub-folder: parse title and year (and `{tmdb-123}` / `{imdb-tt123}`
   tags if present), look it up on TMDB, find the video files. The largest
   becomes the file of the primary profile. Other video files (not samples
   or extras) are adopted as unmanaged files.
3. The user reviews the matches and confirms.
4. Unseen stores the movie with its existing folder name and relative file path.

Adoption never moves, renames or deletes anything. Renaming to the template
is a separate, explicit action with a preview.

## Import

1. Find the video files of a completed download (skip samples and extras).
2. Build the target path from the templates.
3. Hardlink. If the hardlink fails with a cross-device error, copy.
4. Import sidecar subtitle files (see below).
5. For an upgrade: remove the old file of the same movie profile after the
   new one is in place. Files of other profiles and unmanaged files are not
   touched.
6. Record the file and write a history event.

Two profiles of one movie share the movie folder. The target name must be
unique in the folder: when the template gives the same name for both files,
the import fails with a clear error instead of overwriting. The default file
template contains `{Quality Full}`, which keeps a 1080p and a 2160p file
apart.

### Sidecar subtitles

The name rule below is the one for subtitles that Unseen downloads (M10) as
well as for those it imports.

Subtitle files next to the video with the same base name (`.srt`, `.ass`,
`.ssa`, `.sub`, `.idx`) are imported with it and renamed to match, keeping
their language suffix (`Movie (2020) Bluray-1080p.en.srt`). The extension
list is the setting `import.extra_file_extensions`; empty turns it off.

Subtitle names are `<video base name>.<language>[.<kind>].<extension>`:

| Subtitle | Name |
|---|---|
| English | `Movie (2020) Bluray-1080p.en.srt` |
| English, hearing impaired | `Movie (2020) Bluray-1080p.en.hi.srt` |
| French, forced | `Movie (2020) Bluray-1080p.fr.forced.srt` |
| Brazilian Portuguese | `Movie (2020) Bluray-1080p.pt-BR.srt` |

- The language is the code stored in `subtitles.language`. The kind suffix
  is `hi` or `forced`; `normal` has none. These are the names Bazarr, Plex
  and Jellyfin read, so a library that Bazarr filled is adopted as it is.
- An imported file keeps its own language and kind suffix when it has one.
  Reading a name accepts `sdh` and `cc` as `hi`, `foreign` as `forced`, and
  three-letter codes (`eng`); the stored name is the normalised one only when
  Unseen renames the file. A file without a language suffix is imported as it is and recorded
  with the language `und` (undetermined), which satisfies no wanted item.
- A downloaded file is always UTF-8 text with the provider's extension
  (`srt`, `ass`). It is written to a temporary name in the movie folder and
  renamed, so a media server never sees a half file. An existing file with
  the target name is never overwritten.
- When the video is renamed or upgraded, its subtitles are renamed with it
  and keep their language and kind. They are deleted with the video.
- Several movie profiles of one movie have separate video names, so each
  video has its own subtitle files, one set per file.

Other extra files (`.nfo`, images, samples) are not imported: Unseen does
not manage them and they would be left behind on upgrades.

## Case-insensitive and Windows filesystems

Supported, not best effort. The same code must be correct on ext4, on NTFS
and on case-insensitive network shares.

- Paths are compared through one helper that knows whether the root folder
  is case-insensitive (probed once per root folder).
- Name collisions are checked case-insensitively everywhere, so a library
  can move between filesystems.
- Templates replace the characters Windows forbids (`<>:"/\|?*`), trailing
  dots and spaces, and reserved names (`CON`, `NUL`, `COM1` and others) on
  every platform.
- Long paths: the full target path is checked against the platform limit
  before the import starts.
- Hardlinks are used on NTFS as on Linux; the cross-device fallback is the
  same.
- CI runs the `core` and import tests on Linux and on Windows.

A packaged Windows install is a different question and not offered yet
([13-deployment.md](13-deployment.md)).

| Case | Action |
|---|---|
| Torrent, same filesystem | Hardlink; the torrent keeps seeding |
| Torrent, other filesystem | Copy, with a warning |
| Usenet (M9) | Move |

## Ownership and permissions

- The container runs as `PUID`:`PGID` (default 1000:1000).
- New folders use mode 755 and files 644, adjustable with `UMASK`.
- Unseen never changes ownership of existing files.

## TV (M7)

Files of shows. Everything above applies unless this section changes it:
hardlink and copy, the case-insensitive rules, ownership and sidecar
subtitles are the same for episode files.

### Folder layout

```
/data
  /torrents/<category>/...     written by qBittorrent
  /media/tv/<Show Folder>/Season 1/<Episode File>.<ext>
  /media/tv/<Show Folder>/Specials/<Episode File>.<ext>
```

- A root folder holds movies or shows, never both: the kind is chosen when
  the folder is added (`root_folders.media_kind`, default `movie` for the
  folders of V1) and cannot change while the folder is in use. The rules for
  a new root folder (absolute, exists, writable, no nesting) are the same, and
  the nesting check covers folders of both kinds.
- The season folder is named by the season folder template, and season 0 by
  the specials folder template. A show can turn season folders off
  (`shows.season_folder`): its files then sit in the show folder.
- Several show profiles share the show folder and the season folder. The
  target name must be unique in the folder, as for movies; the default
  templates contain `{Quality Full}`.

### Naming templates for TV

Defaults equal the defaults of Sonarr's `NamingConfig` (read in its source on
2026-11-25). Recheck them against `GET /api/v3/config/naming` of a new Sonarr
when #91 is implemented.

| Setting | Default |
|---|---|
| Show folder | `{Series Title}` |
| Season folder | `Season {season}` |
| Specials folder | `Specials` |
| Standard episode file | `{Series Title} - S{season:00}E{episode:00} - {Episode Title} {Quality Full}` |
| Daily episode file | `{Series Title} - {Air-Date} - {Episode Title} {Quality Full}` |
| Anime episode file | `{Series Title} - S{season:00}E{episode:00} - {Episode Title} {Quality Full}` |
| Multi-episode style | Prefixed range |
| Rename episodes | off (keep the original file name), as in Sonarr |
| Replace illegal characters, colon replacement | The settings for movies, one setting each for both kinds. Sonarr's own default for the colon is Smart |

The syntax is the one of movies: Radarr's and Sonarr's, with the separator
variants (`{Series.Title}`), case variants, prefix and suffix inside braces,
and the number format after a colon (`{season:00}`). Tokens for TV, on top of
the groups for media info, release, quality and custom formats:

| Group | Tokens |
|---|---|
| Show | `{Series Title}`, `{Series CleanTitle}`, `{Series TitleYear}`, `{Series CleanTitleYear}`, `{Series TitleWithoutYear}`, `{Series TitleThe}`, `{Series TitleTheYear}`, `{Series TitleFirstCharacter}`, `{Series Year}` |
| Numbers | `{season}`, `{season:00}`, `{episode}`, `{episode:00}`, `{absolute}`, `{absolute:000}`, `{Air Date}` (`{Air-Date}` gives `2024-05-17`) |
| Episode | `{Episode Title}`, `{Episode CleanTitle}` |
| Ids | `{ImdbId}`, `{TvdbId}`, `{TmdbId}` |

- `{absolute}` renders only for a show of type `anime` whose covered episodes
  all have an absolute number; otherwise it and the text around it in the
  pattern are dropped, as in Sonarr. The default anime template does not use
  it.
- The multi-episode style applies to a file that covers several episodes. For
  episodes 1 to 3 of season 1 with the standard template:

  | Style | Result |
  |---|---|
  | Extend | `S01E01-02-03` |
  | Duplicate | the whole `S01E01` pattern, once per episode |
  | Repeat | `S01E01E02E03` |
  | Scene | `S01E01-E02-E03` |
  | Range | `S01E01-03` |
  | Prefixed range (default) | `S01E01-E03` |

- `{Episode Title}` of a multi-episode file joins the titles with ` + `
  (`{Episode CleanTitle}` with ` and `). When the file name would be longer
  than the limit, the title is shortened and ends in `...`; no other part of
  the name is cut. The limit is 255 bytes per name, and the full-path check of
  the Windows section applies.
- An unknown token is a validation error on save, and the tests compare every
  token with what Sonarr produces. The settings are in
  [10-configuration.md](10-configuration.md).

### Import of an episode or a season pack

Extends the steps of "Import" above.

1. Find the video files of the completed download (skip samples and extras).
   A pack has many files, often in sub-folders.
2. Match each file to episodes: parse the file name, and when the name has
   no season, the folder name. Use the show's series type and numbering (see
   [07-release-scoring.md](07-release-scoring.md)).
3. A file imports only for episodes that are in `download_episodes` of the
   download. A file for other episodes, or that matches none, is skipped and
   listed in the history with the reason. A hand-grabbed pack lists every
   episode it holds there.
4. Build the target path: show folder, season folder (or specials folder),
   file name from the template for the series type. Create the season folder
   when it does not exist.
5. Hardlink each file, or copy on a cross-device error. A file with the target
   name already in the library for the same episodes and profile is the
   upgrade case of step 7; any other existing target fails that file with a
   clear error.
6. Import the sidecar subtitles of each file (rules above; names use the
   episode file's base name, one set per file).
7. Upgrade: after the new file is in place, remove its episodes from the old
   managed file of the same show profile. An old file that covers no episode
   any more is deleted; a multi-episode file that still covers another
   episode stays. Files of other profiles and unmanaged files are not touched.
8. Record the `episode_files` row and its links, and write a history event for
   each file.

The download is `Imported` when every file was imported or skipped, and
`ImportFailed` when a file failed (see
[06-state-machines.md](06-state-machines.md)). The manual retry imports the
files that are not in the library yet. For Usenet (M9) every file is moved,
not linked.

### Adopting a Sonarr library

As the steps for movies, with these differences. Adoption never moves, renames
or deletes anything.

1. The user picks a root folder of kind `show` and starts a scan.
2. Per sub-folder: parse the title and year, and the tags `{tvdb-123}`,
   `{tmdb-123}`, `{imdb-tt123}` when present; look the show up with the
   provider order. The user reviews the match.
3. Per video file under the folder: parse its name, and its season folder name
   when the name has no season, and link it to the episodes of the show. A
   file that matches no episode is listed as unmatched in the review and is
   not stored.
4. For an episode with several files, the largest becomes the file of the
   primary show profile and the others are unmanaged. A multi-episode file
   counts for every episode it covers.
5. Store the show with its folder name and `season_folder` (on when any file
   sits in a `Season N` or `Specials` folder), and each file with its path
   relative to the show folder.
6. The monitoring option of the add form applies as for a new show; `missing`
   skips the episodes that have a file.

A library that follows Sonarr's default naming is matched completely and stays
byte-identical, which is the test of #92.

### TV decisions (2026-11-25)

- A root folder has a kind. Mixed folders are not supported.
- Defaults equal Sonarr's, including `Season {season}` for the season folder
  and the anime template without an absolute number.
- A pack imports into the episodes its download lists. Everything else in the
  pack is skipped with a reason, not failed.

## Decisions (2026-11-15)

- Recycle bin: not in V1. Replaced files are deleted. Planned for M11.
- Extra files: sidecar subtitles only, in V1.
- Case-insensitive and Windows filesystems: supported.
