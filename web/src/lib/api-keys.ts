/** The last use of a key, for a table cell. */
export function lastUsed(iso: string | null | undefined): string {
  if (!iso) return "Never";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "Never" : date.toLocaleString();
}

/** The key as it appears in a list: the stored prefix, then dots. */
export function maskedKey(prefix: string): string {
  return `${prefix}…`;
}
