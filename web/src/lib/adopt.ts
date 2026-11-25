import type { components } from "./api/schema.js";

export type ScanFolder = components["schemas"]["ScanFolder"];
export type ScanStatus = components["schemas"]["ScanStatus"];
export type MovieLookup = components["schemas"]["MovieLookup"];
export type AdoptItem = components["schemas"]["AdoptItem"];
export type AdoptResult = components["schemas"]["AdoptResult"];
export type AdoptFailure = components["schemas"]["AdoptFailure"];

/** A scanned folder as the user reviews it: the proposed match, which can change. */
export interface ReviewRow {
  folder: ScanFolder;
  tmdbId: number | null;
  matchTitle: string | null;
  matchYear: number | null;
  posterPath: string | null;
  /** Chosen for adoption. */
  include: boolean;
  /** The user replaced the proposed match. */
  corrected: boolean;
}

export const statusNames: Record<ScanStatus, string> = {
  matched: "Matched",
  unmatched: "No match",
  duplicate: "Already in library",
  noVideo: "No video file",
};

/** Only a confident match is selected up front; the rest need a decision. */
export function reviewRows(folders: readonly ScanFolder[]): ReviewRow[] {
  return folders.map((folder) => ({
    folder,
    tmdbId: folder.tmdbId ?? null,
    matchTitle: folder.matchTitle ?? null,
    matchYear: folder.matchYear ?? null,
    posterPath: folder.posterPath ?? null,
    include: folder.status === "matched" && folder.tmdbId != null,
    corrected: false,
  }));
}

/** A folder can be adopted when it has a video file and a movie to attach it to. */
export function adoptable(row: ReviewRow): boolean {
  return row.folder.files.length > 0 && row.tmdbId != null;
}

/** Replaces the match of a row with a lookup result and selects the row. */
export function withMatch(row: ReviewRow, movie: MovieLookup): ReviewRow {
  return {
    ...row,
    tmdbId: movie.tmdbId,
    matchTitle: movie.title,
    matchYear: movie.year ?? null,
    posterPath: movie.posterPath ?? null,
    include: row.folder.files.length > 0,
    corrected: true,
  };
}

/** The rows that the next adopt call sends. */
export function selectedItems(rows: readonly ReviewRow[]): AdoptItem[] {
  return rows
    .filter((row) => row.include && adoptable(row))
    .map((row) => ({ folderName: row.folder.folderName, tmdbId: row.tmdbId! }));
}

/** Other selected rows that point at the same movie: only the first can be adopted. */
export function duplicateTargets(rows: readonly ReviewRow[]): Set<number> {
  const seen = new Set<number>();
  const repeated = new Set<number>();
  for (const row of rows) {
    if (!row.include || !adoptable(row)) continue;
    if (seen.has(row.tmdbId!)) repeated.add(row.tmdbId!);
    seen.add(row.tmdbId!);
  }
  return repeated;
}

export interface Summary {
  adopted: number;
  failed: number;
  /** Folders left out of the request. */
  skipped: number;
  files: number;
}

export function summarize(result: AdoptResult, skipped: number): Summary {
  return {
    adopted: result.adopted.length,
    failed: result.failed.length,
    skipped,
    files: result.adopted.reduce((total, movie) => total + movie.fileCount, 0),
  };
}

/** The rows that stay on screen after an adopt: everything not adopted. */
export function remainingRows(
  rows: readonly ReviewRow[],
  result: AdoptResult,
): ReviewRow[] {
  const adopted = new Set(result.adopted.map((movie) => movie.folderName));
  return rows
    .filter((row) => !adopted.has(row.folder.folderName))
    .map((row) => ({ ...row, include: false }));
}
