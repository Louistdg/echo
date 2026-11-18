import type { NoteDocument } from './types';

export interface DraftStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

interface Draft {
  version: 1;
  text: string;
  expected: string;
  updated: number;
}

export interface SaveRequest {
  path: string;
  workspace: string;
  text: string;
  expected: string;
}

export interface SaveSnapshot {
  document: NoteDocument;
  text: string;
  dirty: boolean;
  saving: boolean;
  error: string | null;
  draftError: string | null;
  recovered: boolean;
  recoveryConflict: boolean;
}

export function draftKey(workspace: string, path: string): string {
  return `echo.v3.note-draft:${encodeURIComponent(workspace)}:${encodeURIComponent(path)}`;
}

/** One note, one serialized writer. The expected text advances only after disk confirms it. */
export class SaveSession {
  private document: NoteDocument;
  private text: string;
  private expected: string;
  private save: (request: SaveRequest) => Promise<NoteDocument>;
  private storage: DraftStorage;
  private changed: (state: SaveSnapshot) => void;
  private debounceMs: number;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private running: Promise<void> | null = null;
  private disposed = false;
  private saving = false;
  private error: string | null = null;
  private draftError: string | null = null;
  private recovered = false;
  private recoveryConflict = false;

  constructor(options: {
    document: NoteDocument;
    save: (request: SaveRequest) => Promise<NoteDocument>;
    storage: DraftStorage;
    changed: (state: SaveSnapshot) => void;
    debounceMs?: number;
  }) {
    this.document = { ...options.document };
    this.text = options.document.text;
    this.expected = options.document.text;
    this.save = options.save;
    this.storage = options.storage;
    this.changed = options.changed;
    this.debounceMs = options.debounceMs ?? 650;
    this.restoreDraft();
    this.emit();
    if (this.text !== this.expected && !this.recoveryConflict) this.schedule();
  }

  snapshot(): SaveSnapshot {
    return {
      document: { ...this.document },
      text: this.text,
      dirty: this.text !== this.expected || this.recoveryConflict,
      saving: this.saving,
      error: this.error,
      draftError: this.draftError,
      recovered: this.recovered,
      recoveryConflict: this.recoveryConflict,
    };
  }

  edit(text: string): void {
    if (this.disposed || text === this.text) return;
    this.text = text;
    this.persistDraft();
    this.emit();
    if (!this.recoveryConflict && !this.error) this.schedule();
  }

  async flush(): Promise<void> {
    this.clearTimer();
    if (this.disposed) throw new Error('Cette note n’est plus ouverte.');
    if (this.recoveryConflict) {
      throw new Error('Le fichier a changé depuis ce brouillon. Récupère le brouillon dans une nouvelle note pour conserver les deux versions.');
    }
    if (this.running) {
      await this.running;
      if (this.text !== this.expected) await this.flush();
      return;
    }
    if (this.text === this.expected) return;
    const task = this.saveUntilCurrent();
    this.running = task;
    try {
      await task;
    } finally {
      if (this.running === task) this.running = null;
    }
  }

  /** External updates may replace only a clean buffer. */
  acceptExternal(document: NoteDocument): boolean {
    if (document.workspace !== this.document.workspace || document.path !== this.document.path) return false;
    if (this.saving || this.text !== this.expected || this.recoveryConflict) return false;
    this.document = { ...document };
    this.text = document.text;
    this.expected = document.text;
    this.error = null;
    this.persistDraft();
    this.emit();
    return true;
  }

  /** Called only after a separate, recovered note has been durably saved. */
  clearRecoveredDraft(): void {
    this.storage.removeItem(draftKey(this.document.workspace, this.document.path));
  }

  dispose(): void {
    this.clearTimer();
    this.disposed = true;
  }

  private schedule(): void {
    this.clearTimer();
    if (this.text === this.expected) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.flush().catch(() => { /* The snapshot retains the failure and draft. */ });
    }, this.debounceMs);
  }

  private clearTimer(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }

  private async saveUntilCurrent(): Promise<void> {
    this.saving = true;
    this.emit();
    try {
      while (this.text !== this.expected) {
        const sentText = this.text;
        const result = await this.save({
          path: this.document.path,
          workspace: this.document.workspace,
          text: sentText,
          expected: this.expected,
        });
        if (result.workspace !== this.document.workspace || result.path !== this.document.path) {
          throw new Error('La sauvegarde a répondu pour un autre fichier. Le brouillon a été conservé.');
        }
        this.document = { ...result };
        this.expected = result.text;
        if (this.text === sentText) this.text = result.text;
        this.error = null;
        this.recovered = false;
        this.persistDraft();
        this.emit();
      }
    } catch (error) {
      this.clearTimer();
      this.error = String(error instanceof Error ? error.message : error);
      this.persistDraft();
      throw error;
    } finally {
      this.saving = false;
      this.emit();
    }
  }

  private restoreDraft(): void {
    try {
      const raw = this.storage.getItem(draftKey(this.document.workspace, this.document.path));
      if (!raw) return;
      const draft: Draft = JSON.parse(raw);
      if (draft.version !== 1 || typeof draft.text !== 'string' || typeof draft.expected !== 'string') {
        this.draftError = 'Un ancien brouillon ne peut pas être lu. Il a été conservé.';
        return;
      }
      if (draft.text === this.document.text) {
        this.storage.removeItem(draftKey(this.document.workspace, this.document.path));
        return;
      }
      this.text = draft.text;
      this.expected = draft.expected;
      this.recovered = true;
      this.recoveryConflict = draft.expected !== this.document.text;
    } catch {
      this.draftError = 'Le brouillon local ne peut pas être lu. Le fichier sur disque reste disponible.';
    }
  }

  private persistDraft(): void {
    try {
      const key = draftKey(this.document.workspace, this.document.path);
      if (this.text === this.expected && !this.recoveryConflict) {
        this.storage.removeItem(key);
      } else {
        this.storage.setItem(key, JSON.stringify({
          version: 1,
          text: this.text,
          expected: this.expected,
          updated: Date.now(),
        } satisfies Draft));
      }
      this.draftError = null;
    } catch {
      this.draftError = 'Le brouillon de secours ne peut pas être stocké. Enregistre la note avant de fermer Écho.';
    }
  }

  private emit(): void {
    if (!this.disposed) this.changed(this.snapshot());
  }
}
