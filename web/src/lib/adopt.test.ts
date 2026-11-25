import { describe, expect, it } from "vitest";
import {
  adoptable,
  duplicateTargets,
  remainingRows,
  reviewRows,
  selectedItems,
  summarize,
  withMatch,
  type AdoptResult,
  type MovieLookup,
  type ScanFolder,
} from "./adopt.js";

function folder(patch: Partial<ScanFolder>): ScanFolder {
  return {
    folderName: "The Matrix (1999)",
    title: "The Matrix",
    year: 1999,
    status: "matched",
    tmdbId: 603,
    matchTitle: "The Matrix",
    matchYear: 1999,
    files: [{ relativePath: "The Matrix (1999).mkv", size: 1000 }],
    ...patch,
  } as ScanFolder;
}

const unmatched = folder({
  folderName: "Mystery",
  status: "unmatched",
  tmdbId: null,
  matchTitle: null,
  matchYear: null,
});
const duplicate = folder({ folderName: "Alien (1979)", status: "duplicate", tmdbId: 348 });
const noVideo = folder({ folderName: "Empty", status: "noVideo", tmdbId: null, files: [] });

describe("reviewRows", () => {
  it("selects only confident matches", () => {
    const rows = reviewRows([folder({}), unmatched, duplicate, noVideo]);
    expect(rows.map((row) => row.include)).toEqual([true, false, false, false]);
  });

  it("selects nothing for an empty scan", () => {
    expect(reviewRows([])).toEqual([]);
  });
});

describe("adoptable", () => {
  it("needs a video file and a match", () => {
    const [matched, none, empty] = reviewRows([folder({}), unmatched, noVideo]);
    expect(adoptable(matched)).toBe(true);
    expect(adoptable(none)).toBe(false);
    expect(adoptable(empty)).toBe(false);
  });
});

describe("withMatch", () => {
  const lookup = {
    tmdbId: 604,
    title: "The Matrix Reloaded",
    year: 2003,
    posterPath: "/p.jpg",
  } as MovieLookup;

  it("replaces the match and selects the row", () => {
    const [row] = reviewRows([unmatched]);
    const fixed = withMatch(row, lookup);
    expect(fixed).toMatchObject({
      tmdbId: 604,
      matchTitle: "The Matrix Reloaded",
      matchYear: 2003,
      include: true,
      corrected: true,
    });
    expect(adoptable(fixed)).toBe(true);
  });

  it("does not select a folder without a video file", () => {
    const [row] = reviewRows([noVideo]);
    expect(withMatch(row, lookup).include).toBe(false);
  });

  it("sends the corrected id, not the proposed one", () => {
    const [row] = reviewRows([folder({})]);
    expect(selectedItems([withMatch(row, lookup)])).toEqual([
      { folderName: "The Matrix (1999)", tmdbId: 604 },
    ]);
  });
});

describe("selectedItems", () => {
  it("skips unselected and unadoptable rows", () => {
    const rows = reviewRows([folder({}), unmatched, duplicate]);
    rows[2].include = true;
    rows[1].include = true;
    expect(selectedItems(rows)).toEqual([
      { folderName: "The Matrix (1999)", tmdbId: 603 },
      { folderName: "Alien (1979)", tmdbId: 348 },
    ]);
  });
});

describe("duplicateTargets", () => {
  it("flags a movie chosen twice", () => {
    const rows = reviewRows([folder({}), folder({ folderName: "Matrix copy" })]);
    expect([...duplicateTargets(rows)]).toEqual([603]);
  });

  it("ignores unselected rows", () => {
    const rows = reviewRows([folder({}), folder({ folderName: "Matrix copy" })]);
    rows[1].include = false;
    expect(duplicateTargets(rows).size).toBe(0);
  });
});

describe("summarize and remainingRows", () => {
  const result: AdoptResult = {
    adopted: [{ folderName: "The Matrix (1999)", movieId: 1, fileCount: 2 }],
    failed: [{ folderName: "Alien (1979)", code: "movie_exists", message: "exists" }],
  };

  it("counts adopted, failed and skipped", () => {
    expect(summarize(result, 3)).toEqual({ adopted: 1, failed: 1, skipped: 3, files: 2 });
  });

  it("keeps the folders that were not adopted, deselected", () => {
    const rows = reviewRows([folder({}), duplicate]);
    rows[1].include = true;
    const left = remainingRows(rows, result);
    expect(left.map((row) => row.folder.folderName)).toEqual(["Alien (1979)"]);
    expect(left[0].include).toBe(false);
  });

  it("summarizes 200 adopted folders as one count", () => {
    const many: AdoptResult = {
      adopted: Array.from({ length: 200 }, (_, index) => ({
        folderName: `Movie ${index}`,
        movieId: index,
        fileCount: 1,
      })),
      failed: [],
    };
    expect(summarize(many, 0)).toEqual({ adopted: 200, failed: 0, skipped: 0, files: 200 });
  });
});
