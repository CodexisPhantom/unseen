import { describe, expect, it } from 'vitest';
import { formatBytes } from './root-folders.js';

describe('formatBytes', () => {
  it('uses binary steps and a short fraction', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1023 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(150 * 1024 ** 3)).toBe('150 GB');
    expect(formatBytes(3 * 1024 ** 4)).toBe('3.0 TB');
  });

  it('stops at the largest unit', () => {
    expect(formatBytes(2048 * 1024 ** 5)).toBe('2048 PB');
  });

  it('shows a dash when the size is unknown', () => {
    expect(formatBytes(null)).toBe('—');
    expect(formatBytes(undefined)).toBe('—');
    expect(formatBytes(-1)).toBe('—');
    expect(formatBytes(Number.NaN)).toBe('—');
  });
});
