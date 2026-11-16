# 07 Release parsing and scoring

Status: reviewed (2026-11-16). Movies only. Episode parsing is added in M7.

## Parsed fields

| Field | Example values |
|---|---|
| `title` | `The Matrix` |
| `year` | `1999` |
| `resolution` | 480p, 576p, 720p, 1080p, 2160p |
| `source` | CAM, TELESYNC, TELECINE, DVDSCR, DVD, HDTV, WEBDL, WEBRip, Bluray, Remux, BR-DISK |
| `quality` | Derived from source and resolution, a Radarr quality name |
| `codec` | x264, x265, AV1, XviD |
| `audio` | AAC, AC3, EAC3, DTS, DTS-HD MA, TrueHD, Atmos |
| `hdr` | HDR10, HDR10+, DV |
| `release_group` | `SPARKS` |
| `edition` | Director's Cut, Extended, IMAX, Remastered |
| `proper`, `repack`, `version` | revision of the same release |
| `languages` | From tags such as `FRENCH`, `MULTI`; default: unknown |
| `hardcoded_subs`, `3d` | flags |

## Qualities

Same names and order as Radarr, lowest to highest. Verify this list against
Radarr's `GET /api/v3/qualitydefinition` before implementing.

```
Unknown, WORKPRINT, CAM, TELESYNC, TELECINE, REGIONAL, DVDSCR, SDTV, DVD,
DVD-R, WEBDL-480p, WEBRip-480p, Bluray-480p, Bluray-576p, HDTV-720p,
WEBDL-720p, WEBRip-720p, Bluray-720p, HDTV-1080p, WEBDL-1080p, WEBRip-1080p,
Bluray-1080p, Remux-1080p, HDTV-2160p, WEBDL-2160p, WEBRip-2160p,
Bluray-2160p, Remux-2160p, BR-DISK, Raw-HD
```

Quality ids match Radarr's ids so the compat API can return them unchanged.

## Quality definitions (size limits)

As in Radarr: each quality has a minimum, a preferred and a maximum size in
megabytes per minute of runtime. The table is seeded with Radarr's defaults
and editable (`profiles:manage`). Instance import and the TRaSH guide's
quality sizes can overwrite it. A release whose size is outside minimum and
maximum is rejected. A movie without a known runtime skips the check.

## Quality profile

- An ordered list of qualities or quality groups, each allowed or not.
- `cutoff`: the quality at which upgrades stop.
- `upgradeAllowed`: whether a better release replaces an existing file.
- `language`: `any`, `original`, or one language, as in Radarr. Default
  `any`. Finer language preferences are expressed with language conditions
  in custom formats, which is how the TRaSH guides do it.
- Custom format scores: one score per custom format, default 0.
- `minFormatScore`: releases with a lower total score are rejected.
- `cutoffFormatScore`: upgrades for score stop when the file reaches it.
- `minUpgradeFormatScore`: smallest score gain that counts as an upgrade.

## Custom formats

Same model and JSON format as Radarr, so that formats exported from Radarr
and published by the TRaSH guides load unchanged.

- A custom format is a name and a list of conditions (specifications).
- Each condition has a type, a value, `negate` and `required`.
- A format matches when every required condition matches and, if there are
  non-required conditions, at least one of each type matches. Verify the
  exact rule against Radarr's source before implementing.
- Condition types in V1: release title (regex), release group (regex),
  edition (regex), language, source, resolution, quality modifier (remux and
  others), size, indexer flag, year. Regex syntax is .NET's upstream; the
  Rust engine must accept every regex in the TRaSH guide data (a test loads
  all of them).
- The total score of a release for a profile is the sum of the scores of the
  matching formats.
- Formats with `includeWhenRenaming` appear in the `{Custom Formats}` naming
  token.
- A test endpoint takes a release name and returns the matching formats and
  the score per profile.

## TRaSH guides

Unseen reads the JSON that the TRaSH guides publish in their repository
(custom formats, custom format groups, quality profiles with score sets,
quality sizes for Radarr). Verify the paths and file formats when the issue
is implemented.

- A scheduled task downloads the guide data and stores it locally.
- The guide browser lists everything with its description, searchable by
  name and by purpose (for example "HDR", "unwanted", "French audio").
- Applying shows a preview first: which custom formats are created or
  changed, which scores change in which profile, which quality sizes change.
- An applied item can be kept in sync. When the guide changes, the task
  updates synced items and reports what changed. Items the user edited are
  not overwritten; the user sees the difference and chooses.
- The data and its licence stay with the TRaSH guides project; the UI links
  to the guide page of each item.

## Rejection rules (in order)

1. The title or year does not match the movie.
2. Quality is not allowed in the profile.
3. The release is on the blocklist.
4. Size is outside the quality definition limits.
5. Language does not satisfy the profile language. A release name without a
   language tag counts as the movie's original language (verify against
   Radarr).
6. The custom format score is below `minFormatScore`.
7. The movie profile already has a file and upgrades are off, or both the
   quality cutoff and the format score cutoff are met.
8. The release is not better than the existing file.
9. The indexer has tags and shares none with the movie.

A rejected release is still listed in manual search, with its reasons.

## Scoring (accepted releases)

Compare in this order, first difference wins:

1. Quality rank in the profile.
2. Custom format score.
3. Revision: repack/proper version, higher wins.
4. Indexer priority.
5. Seeders (torrents), on a log scale.
6. Size, closest to the preferred size of the quality definition; larger
   wins when no preferred size is set.

Verify this order against Radarr's decision comparer before implementing.

## Upgrade rules

A release upgrades the file of a movie profile when `upgradeAllowed` is on
and one of these is true:

- the existing quality is below the cutoff and the release has a higher
  quality rank;
- the quality is the same, the existing format score is below
  `cutoffFormatScore`, and the release scores at least
  `minUpgradeFormatScore` more;
- the quality and score are the same and the release is a higher revision.

## Test table

These rows become unit tests. Expected values are a draft. Before
implementing, run each name through a real Radarr
(`GET /api/v3/parse?title=...`) and replace the row with what Radarr reports.

| Release name | Title | Year | Quality | Group | Notes |
|---|---|---|---|---|---|
| `The.Matrix.1999.1080p.BluRay.x264-SPARKS` | The Matrix | 1999 | Bluray-1080p | SPARKS | |
| `The.Matrix.1999.2160p.UHD.BluRay.REMUX.HDR.HEVC.TrueHD.7.1.Atmos-FGT` | The Matrix | 1999 | Remux-2160p | FGT | HDR10, Atmos |
| `Dune.Part.Two.2024.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX` | Dune Part Two | 2024 | WEBDL-2160p | FLUX | DV, HDR10 |
| `Dune.Part.Two.2024.1080p.WEBRip.x265.10bit.AAC5.1-GROUP` | Dune Part Two | 2024 | WEBRip-1080p | GROUP | |
| `Oppenheimer.2023.IMAX.1080p.BluRay.REMUX.AVC.DTS-HD.MA.5.1-TRiToN` | Oppenheimer | 2023 | Remux-1080p | TRiToN | edition IMAX |
| `Blade.Runner.1982.The.Final.Cut.720p.BluRay.x264-AMIABLE` | Blade Runner | 1982 | Bluray-720p | AMIABLE | edition The Final Cut |
| `Some.Movie.2023.REPACK.1080p.WEB.H264-NAISU` | Some Movie | 2023 | WEBDL-1080p | NAISU | repack, bare `WEB` |
| `Some.Movie.2023.PROPER.720p.HDTV.x264-GROUP` | Some Movie | 2023 | HDTV-720p | GROUP | proper |
| `Some.Movie.2024.HDCAM.x264-GROUP` | Some Movie | 2024 | CAM | GROUP | |
| `Some.Movie.2024.TS.XviD-GROUP` | Some Movie | 2024 | TELESYNC | GROUP | |
| `Some.Movie.2010.DVDRip.XviD-GROUP` | Some Movie | 2010 | DVD | GROUP | |
| `Some.Movie.2015.COMPLETE.BLURAY-GROUP` | Some Movie | 2015 | BR-DISK | GROUP | |
| `Le.Fabuleux.Destin.d.Amelie.Poulain.2001.FRENCH.1080p.BluRay.x264-GROUP` | Le Fabuleux Destin d Amelie Poulain | 2001 | Bluray-1080p | GROUP | language French |
| `2001.A.Space.Odyssey.1968.1080p.BluRay.x264-GROUP` | 2001 A Space Odyssey | 1968 | Bluray-1080p | GROUP | year-like title |
| `Some Movie (2019) [1080p] [BluRay] [YTS.MX]` | Some Movie | 2019 | Bluray-1080p | YTS.MX | bracket style |
| `Some.Movie.2019.1080p.x264-GROUP` | Some Movie | 2019 | Unknown | GROUP | resolution without source; check what Radarr reports |

## Decisions (2026-11-15)

- Size limits reuse Radarr's quality definitions.
- Language: profile language as in Radarr, plus language conditions in custom
  formats. This keeps instance import and the TRaSH guides lossless; a
  separate preferred list would be a second, incompatible mechanism.
- Custom formats and the TRaSH guide browser are in V1.

## Open questions

- Regex compatibility: which Rust regex engine accepts all upstream patterns
  (look-around is used in the guide data)?
