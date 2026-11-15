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
| Other | `{Custom Formats}` renders empty until custom formats exist |

An unknown token is a validation error when the template is saved. The
expected output for each token is tested against what Radarr produces.

Open question: `MediaInfo` tokens need a media probe (ffprobe or a Rust
library). Decide in M4; until then those tokens render from the release name.

## Adopting an existing library

1. The user picks a root folder and starts a scan.
2. For each sub-folder: parse title and year (and `{tmdb-123}` / `{imdb-tt123}`
   tags if present), look it up on TMDB, find the largest video file.
3. The user reviews the matches and confirms.
4. Unseen stores the movie with its existing folder name and relative file path.

Adoption never moves, renames or deletes anything. Renaming to the template
is a separate, explicit action with a preview.

## Import

1. Find the video files of a completed download (skip samples and extras).
2. Build the target path from the templates.
3. Hardlink. If the hardlink fails with a cross-device error, copy.
4. For an upgrade: remove the old file after the new one is in place.
5. Record the file and write a history event.

| Case | Action |
|---|---|
| Torrent, same filesystem | Hardlink; the torrent keeps seeding |
| Torrent, other filesystem | Copy, with a warning |
| Usenet (M9) | Move |

## Ownership and permissions

- The container runs as `PUID`:`PGID` (default 1000:1000).
- New folders use mode 755 and files 644, adjustable with `UMASK`.
- Unseen never changes ownership of existing files.

## Open questions

- Recycle bin for replaced files, as in Radarr.
- Extra files (subtitles, nfo) during import.
- Case-insensitive and Windows filesystems: supported or best effort.
