export interface NoteSummary {
  path: string;
  title: string;
  preview: string;
  modified: string;
  bytes: number;
}

export interface NoteDocument {
  path: string;
  title: string;
  text: string;
  modified: string;
  workspace: string;
}

export interface AssetItem {
  path: string;
  name: string;
  modified: string;
  bytes: number;
}

export interface WorkspaceSettings {
  language?: string;
  workspace: string;
  theme: string;
  active_note: string | null;
  shortcut_dictation: string;
  shortcut_capture: string;
  voice_mode: string;
  destination: string;
}

export interface VoiceState {
  microphone_open: boolean;
  phase: string;
  mode: string;
  kind: string;
  session_id: string | null;
  text: string;
  partial: string;
  error: string | null;
  audio_path: string | null;
  duration_secs: number;
  level: number;
  progress: number | null;
  engine: string;
  message: string;
  target_label: string;
  queued: number;
}
