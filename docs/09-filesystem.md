# 09 File handling

Status: reviewed (2026-11-16). Movies only.

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

Defaults equal the Radarr defaults. The engine is `core::naming`, written
after Radarr's `FileNameBuilder`; the settings are read and saved at
`GET/PUT /api/v1/settings/naming` (`settings:read` / `settings:manage`).

| Setting | Default |
|---|---|
| Movie folder | `{Movie Title} ({Release Year})` |
| Movie file | `{Movie Title} ({Release Year}) {Quality Full}` |
| Rename movies | off (keep the original file name), as in Radarr |
| Replace illegal characters | on |
| Colon replacement | Smart (Radarr 6; Radarr 5 and older default to Delete, which is also available) |

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

Rules, as in Radarr:

- Token names ignore case and the separators `. - _` and space. The
  separator written in a token replaces the spaces of the value
  (`{Movie.Title}` gives `The.Matrix`). A token written in all lower or all
  upper case gives a lower or upper case value.
- Prefix and suffix inside the braces (`{[Quality Full]}`, `{-Release Group}`)
  show only when the value is not empty. `{imdb-{ImdbId}}` and
  `{{Edition Tags}}` keep their outer braces the same way.
- `{Release Group}` renders `Unseen` when the file has no group and the
  token has no prefix or suffix (Radarr renders `Radarr`).
- `/` and `\` in a template make sub-folders; in a value they become `+`.
  Radarr's `{Original Title}` and `{Original Filename}` are empty when a
  template holds more than one token, so a new name cannot contain itself.
- Radarr's `:` options (`{Movie Title:30}`, `{Custom Formats:-AMZN}`) are not
  supported yet and are refused when saved.
- Colon replacement is `delete`, `dash`, `spaceDash`, `spaceDashSpace` or
  `smart`, as in Radarr. With "replace illegal characters" off, illegal
  characters (including `:`) are removed.

An unknown token is a 422 when the template is saved. The preview endpoint
(`POST /api/v1/settings/naming/preview`) renders Radarr's example movie with
saved or unsaved settings, so the UI can show the result while the user types.
The expected output for each token is tested against what Radarr produces
(the table test in `crates/core/src/naming/tests.rs` holds values from
Radarr's own unit tests and example movie).

Whatever the template says, a rendered name is a relative path that cannot
leave the root folder: no `.` or `..` part, none of `<>:"|?*` or control
characters, no trailing dot or space, and no Windows device name. Radarr
writes `Con_Game` for `Con.Game`; Unseen does the same and writes `CON_` for a
part that is the device name alone.

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

## Decisions (2026-11-15)

- Recycle bin: not in V1. Replaced files are deleted. Planned for M11.
- Extra files: sidecar subtitles only, in V1.
- Case-insensitive and Windows filesystems: supported.
