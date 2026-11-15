# 07 Release parsing and scoring

Status: draft. Movies only. Episode parsing is added in M7.

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

## Quality profile

- An ordered list of qualities or quality groups, each allowed or not.
- `cutoff`: the quality at which upgrades stop.
- `upgradeAllowed`: whether a better release replaces an existing file.

## Rejection rules (in order)

1. The title or year does not match the movie.
2. Quality is not allowed in the profile.
3. The release is on the blocklist.
4. Size is outside the limits for its quality (limits: open question).
5. The movie already has a file and upgrades are off, or the cutoff is met.
6. The release is not better than the existing file.

A rejected release is still listed in manual search, with its reasons.

## Scoring (accepted releases)

Compare in this order, first difference wins:

1. Quality rank in the profile.
2. Revision: repack/proper version, higher wins.
3. Indexer priority.
4. Seeders (torrents), on a log scale.
5. Size, larger wins.

Custom formats are out of scope until after V1. Open question.

## Upgrade rules

A release upgrades an existing file when `upgradeAllowed` is on, the existing
quality is below the cutoff, and the release ranks higher on step 1 or step 2
above.

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
| `Some.Movie.2019.1080p.x264-GROUP` | Some Movie | 2019 | Unknown resolution source | GROUP | check what Radarr reports |

## Open questions

- Size limits per quality: reuse Radarr's quality definitions?
- Language handling: profile language as in Radarr, or a simple preferred list?
- Custom formats.
