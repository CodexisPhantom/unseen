import { describe, expect, it } from 'vitest';
import { changes, draftOf, problem, type QualityDefinition } from './quality-definitions.js';

const hd: QualityDefinition = {
  qualityId: 5,
  name: 'WEBDL-720p',
  title: 'WEBDL-720p',
  minSize: 0,
  preferredSize: 95,
  maxSize: 100
};
const open: QualityDefinition = { ...hd, preferredSize: null, maxSize: null };

describe('draftOf', () => {
  it('shows an absent size as an empty field', () => {
    expect(draftOf(open)).toEqual({
      title: 'WEBDL-720p',
      minSize: '0',
      preferredSize: '',
      maxSize: ''
    });
  });
});

describe('problem', () => {
  const draft = draftOf(hd);

  it('accepts the stored values and an empty maximum', () => {
    expect(problem(draft)).toBe('');
    expect(problem({ ...draft, preferredSize: '', maxSize: '' })).toBe('');
  });

  it('names the first rule that fails', () => {
    expect(problem({ ...draft, title: ' ' })).toContain('name');
    expect(problem({ ...draft, minSize: '' })).toContain('minimum');
    expect(problem({ ...draft, minSize: '-1' })).toContain('minimum');
    expect(problem({ ...draft, minSize: 'abc' })).toContain('minimum');
    expect(problem({ ...draft, maxSize: '0' })).toContain('maximum');
    expect(problem({ ...draft, minSize: '50', maxSize: '40' })).toContain('maximum');
    expect(problem({ ...draft, minSize: '10', preferredSize: '5' })).toContain('preferred');
    expect(problem({ ...draft, preferredSize: '150' })).toContain('preferred');
    expect(problem({ ...draft, preferredSize: 'x', maxSize: '' })).toContain('preferred');
  });
});

describe('changes', () => {
  it('sends nothing for an untouched row', () => {
    expect(changes(draftOf(hd), hd)).toEqual({});
    expect(changes(draftOf(open), open)).toEqual({});
  });

  it('sends the fields that changed, and null for an emptied optional size', () => {
    const draft = { ...draftOf(hd), title: ' HD ', minSize: '10', preferredSize: '', maxSize: '90' };
    expect(changes(draft, hd)).toEqual({
      title: 'HD',
      minSize: 10,
      preferredSize: null,
      maxSize: 90
    });
  });

  it('sets a size that was empty', () => {
    expect(changes({ ...draftOf(open), maxSize: '120' }, open)).toEqual({ maxSize: 120 });
  });
});
