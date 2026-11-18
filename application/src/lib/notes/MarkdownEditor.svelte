<script lang="ts">
  import { onMount } from 'svelte';
  import { Compartment, EditorState } from '@codemirror/state';
  import { EditorView, keymap, placeholder } from '@codemirror/view';
  import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
  import { markdown } from '@codemirror/lang-markdown';
  import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
  import { search, searchKeymap, openSearchPanel } from '@codemirror/search';
  import { tags } from '@lezer/highlight';

  let { value, readOnly = false, onChange, onSave }: {
    value: string;
    readOnly?: boolean;
    onChange: (text: string) => void;
    onSave: () => void;
  } = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  const editable = new Compartment();
  let applying = false;

  const noteHighlight = HighlightStyle.define([
    { tag: tags.heading1, fontSize: '1.6em', fontWeight: '650', color: 'var(--text)' },
    { tag: tags.heading2, fontSize: '1.3em', fontWeight: '650', color: 'var(--text)' },
    { tag: tags.heading, fontWeight: '650', color: 'var(--text)' },
    { tag: tags.strong, fontWeight: '650' },
    { tag: tags.emphasis, fontStyle: 'italic' },
    { tag: tags.link, color: 'var(--accent)', textDecoration: 'underline', textUnderlineOffset: '3px' },
    { tag: tags.url, color: 'var(--accent)' },
    { tag: tags.monospace, fontFamily: 'Consolas, monospace', fontSize: '.92em' },
    { tag: tags.processingInstruction, color: 'var(--muted)' },
    { tag: tags.meta, color: 'var(--muted)' },
    { tag: tags.quote, color: 'var(--muted)' },
  ]);

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          markdown(),
          history(),
          search({ top: true }),
          keymap.of([
            { key: 'Mod-s', run: () => { onSave(); return true; } },
            ...defaultKeymap,
            ...historyKeymap,
            ...searchKeymap,
            indentWithTab,
          ]),
          EditorView.lineWrapping,
          placeholder('Écris une idée, colle du texte, ou ouvre la barre de dictée…'),
          syntaxHighlighting(noteHighlight),
          editable.of(EditorState.readOnly.of(readOnly)),
          EditorView.contentAttributes.of({ role: 'textbox', 'aria-multiline': 'true', 'aria-label': 'Contenu de la note', spellcheck: 'true', autocapitalize: 'sentences' }),
          EditorView.updateListener.of(update => {
            if (update.docChanged && !applying) onChange(update.state.doc.toString());
          }),
          EditorView.theme({
            '&': { height: '100%', fontSize: '17px', backgroundColor: 'var(--surface)', color: 'var(--text)' },
            '&.cm-focused': { outline: 'none' },
            '.cm-scroller': { overflow: 'auto', fontFamily: 'inherit', lineHeight: '1.8', scrollbarColor: 'var(--border) transparent' },
            '.cm-content': { padding: '34px max(30px, calc((100% - 72ch) / 2)) 150px', caretColor: 'var(--accent)', minHeight: '100%' },
            '.cm-line': { padding: '1px 0' },
            '.cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection': { backgroundColor: 'var(--accent-soft)' },
            '.cm-cursor': { borderLeftColor: 'var(--accent)' },
            '.cm-placeholder': { color: 'var(--muted)', fontStyle: 'normal' },
            '.cm-panels': { backgroundColor: 'var(--sidebar)', color: 'var(--text)', borderColor: 'var(--border)', fontFamily: 'inherit', fontSize: '13px' },
            '.cm-panel.cm-search': { padding: '12px 24px' },
            '.cm-textfield': { backgroundColor: 'var(--surface)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: '6px', padding: '5px 8px' },
            '.cm-button': { backgroundImage: 'none', backgroundColor: 'var(--surface)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: '6px', fontFamily: 'inherit', padding: '5px 8px' },
            '.cm-searchMatch': { backgroundColor: 'var(--accent-soft)', outline: '1px solid var(--accent)' },
            '.cm-searchMatch-selected': { backgroundColor: 'var(--accent-soft)' },
            '.cm-tooltip': { backgroundColor: 'var(--surface)', color: 'var(--text)', borderColor: 'var(--border)' },
            '@media (max-width: 760px)': { '.cm-content': { padding: '24px 22px 100px' } },
          }),
        ],
      }),
    });
    return () => { view?.destroy(); view = undefined; };
  });

  $effect(() => {
    const next = value;
    const locked = readOnly;
    if (!view) return;
    if (view.state.doc.toString() !== next) {
      applying = true;
      try {
        view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next } });
      } finally {
        applying = false;
      }
    }
    view.dispatch({ effects: editable.reconfigure(EditorState.readOnly.of(locked)) });
  });

  export function focus() { view?.focus(); }
  export function find() { if (view) openSearchPanel(view); }
  export function insertText(text: string) {
    if (!view || readOnly) return;
    const selection = view.state.selection.main;
    view.dispatch({ changes: { from: selection.from, to: selection.to, insert: text }, selection: { anchor: selection.from + text.length } });
    view.focus();
  }
</script>

<div class="markdown-editor" bind:this={host}></div>

<style>
  .markdown-editor { height: 100%; min-height: 0; min-width: 0; overflow: hidden; }
  .markdown-editor :global(.cm-content:focus-visible) { outline: none; }
  .markdown-editor :global(.cm-search label) { margin-right: 10px; }
</style>
