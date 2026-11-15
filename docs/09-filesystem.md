# 09 File handling

Status: draft. Movies only.

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

## Naming templates

Defaults equal the Radarr defaults:

| Setting | Default |
|---|---|
| Movie folder | `{Movie Title} ({Release Year})` |
| Movie file | `{Movie Title} ({Release Year}) {Quality Full}` |
| Rename movies | off (keep the original file name), as in Radarr |
| Replace illegal characters | on |
| Colon replacement | Radarr default (verify) |

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

Subtitle files next to the video with the same base name (`.srt`, `.ass`,
`.ssa`, `.sub`, `.idx`) are imported with it and renamed to match, keeping
their language suffix (`Movie (2020) Bluray-1080p.en.srt`). The extension
list is the setting `import.extra_file_extensions`; empty turns it off.
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
