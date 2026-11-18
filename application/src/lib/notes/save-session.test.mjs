import test from 'node:test';
import assert from 'node:assert/strict';
import { SaveSession, draftKey } from './save-session.ts';

const initial = { workspace: 'D:\\Écho\\v2.1\\donnees', path: 'note.md', title: 'Note', text: 'Départ', modified: '2026-10-20T18:00:00Z' };
function storage() {
  /** @type {Map<string, string>} */
  const values = new Map();
  return { values, getItem: (/** @type {string} */ key) => values.get(key) ?? null, setItem: (/** @type {string} */ key, /** @type {string} */ value) => { values.set(key, value); }, removeItem: (/** @type {string} */ key) => { values.delete(key); } };
}
function deferred() {
  let resolve = () => {};
  /** @type {Promise<void>} */
  const promise = new Promise(yes => { resolve = () => yes(); });
  return { promise, resolve };
}

test('edits made during disk write are saved afterwards with the acknowledged base', async () => {
  const first = deferred();
  /** @type {import('./save-session.ts').SaveRequest[]} */
  const calls = [];
  const drafts = storage();
  const session = new SaveSession({ document: initial, storage: drafts, changed: () => {}, debounceMs: 60_000,
    save: async request => {
      calls.push(request);
      if (calls.length === 1) await first.promise;
      return { ...initial, text: request.text };
    },
  });
  session.edit('Première phrase.');
  const saving = session.flush();
  session.edit('Première phrase. Deuxième phrase.');
  const joiningFlush = session.flush();
  assert.equal(calls.length, 1, 'only one disk writer may run');
  first.resolve();
  await Promise.all([saving, joiningFlush]);
  assert.deepEqual(calls.map(({ text, expected }) => ({ text, expected })), [
    { text: 'Première phrase.', expected: 'Départ' },
    { text: 'Première phrase. Deuxième phrase.', expected: 'Première phrase.' },
  ]);
  assert.equal(session.snapshot().text, 'Première phrase. Deuxième phrase.');
  assert.equal(session.snapshot().dirty, false);
  assert.equal(drafts.values.size, 0);
  session.dispose();
});

test('failed save keeps the exact target, draft and original expected text for retry', async () => {
  const drafts = storage();
  /** @type {import('./save-session.ts').SaveRequest[]} */
  const calls = [];
  const session = new SaveSession({ document: initial, storage: drafts, changed: () => {}, debounceMs: 60_000,
    save: async request => {
      calls.push(request);
      if (calls.length === 1) throw new Error('Disque indisponible');
      return { ...initial, text: request.text };
    },
  });
  session.edit('Ne pas perdre cette idée.');
  await assert.rejects(session.flush(), /Disque indisponible/);
  const retained = JSON.parse(drafts.getItem(draftKey(initial.workspace, initial.path)) || 'null');
  assert.equal(retained.text, 'Ne pas perdre cette idée.');
  assert.equal(retained.expected, 'Départ');
  assert.equal(session.snapshot().dirty, true);
  assert.equal(session.snapshot().saving, false);
  await session.flush();
  assert.equal(calls[1].expected, 'Départ');
  assert.equal(calls[1].workspace, initial.workspace);
  assert.equal(calls[1].path, initial.path);
  assert.equal(session.snapshot().error, null);
  session.dispose();
});

test('a recovered draft never overwrites a file changed by another editor', async () => {
  const drafts = storage();
  drafts.setItem(draftKey(initial.workspace, initial.path), JSON.stringify({version: 1, text: 'Mon brouillon', expected: 'Ancienne version', updated: 1}));
  let writes = 0;
  const session = new SaveSession({ document: initial, storage: drafts, changed: () => {}, debounceMs: 60_000,
    save: async request => { writes++; return { ...initial, text: request.text }; },
  });
  assert.equal(session.snapshot().text, 'Mon brouillon');
  assert.equal(session.snapshot().recoveryConflict, true);
  await assert.rejects(session.flush(), /fichier a changé/);
  assert.equal(writes, 0);
  assert.ok(drafts.getItem(draftKey(initial.workspace, initial.path)));
  session.dispose();
});

test('external refresh cannot replace dirty text and a clean buffer accepts a dictation append', async () => {
  const session = new SaveSession({ document: initial, storage: storage(), changed: () => {}, debounceMs: 60_000,
    save: async request => ({ ...initial, text: request.text }),
  });
  session.edit('En cours de rédaction');
  assert.equal(session.acceptExternal({ ...initial, text: 'Texte extérieur' }), false);
  assert.equal(session.snapshot().text, 'En cours de rédaction');
  await session.flush();
  assert.equal(session.acceptExternal({ ...initial, text: 'En cours de rédaction\n\nDictée ajoutée.' }), true);
  assert.equal(session.snapshot().text, 'En cours de rédaction\n\nDictée ajoutée.');
  session.dispose();
});

test('draft keys separate files with the same name in different folders', () => {
  assert.notEqual(draftKey('D:\\A', 'note.md'), draftKey('D:\\B', 'note.md'));
  assert.notEqual(draftKey('D:\\A:x', 'a/b.md'), draftKey('D:\\A', 'x:a/b.md'));
});
