import { EditorView, basicSetup } from 'codemirror';
import { EditorState } from '@codemirror/state';
import { yaml } from '@codemirror/lang-yaml';
import { setDiagnostics } from '@codemirror/lint';
import { syntaxHighlighting } from '@codemirror/language';
import { oneDarkHighlightStyle } from '@codemirror/theme-one-dark';

export interface YamlEditor {
  /** Replace the document without triggering `onChange`. */
  setDoc(text: string): void;
  /** Show (or clear, when `message` is undefined) an error on a 1-based line. */
  showError(line: number | undefined, message: string | undefined): void;
}

function prefersDark(): boolean {
  return globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false;
}

/** Editor chrome drawn from the page's CSS tokens, so it follows light and dark mode. */
const chrome = EditorView.theme({
  '&': { height: '100%', fontSize: '12.5px', backgroundColor: 'var(--surface)', color: 'var(--ink)' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.6' },
  '.cm-content': { caretColor: 'var(--ink)', paddingBlock: '8px' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--ink)' },
  '.cm-gutters': { backgroundColor: 'var(--surface)', color: 'var(--faint)', border: 'none' },
  '.cm-lineNumbers .cm-gutterElement': { paddingInline: '12px 8px' },
  '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'var(--active-line)' },
  '.cm-activeLineGutter': { color: 'var(--muted)' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection':
    { backgroundColor: 'var(--selection) !important' },
  '.cm-foldGutter .cm-gutterElement': { color: 'var(--faint)' },
  '.cm-tooltip': { backgroundColor: 'var(--surface)', border: '1px solid var(--line)', borderRadius: '6px' },
  '.cm-diagnostic-error': { borderLeftColor: 'var(--danger)' },
});

export function createYamlEditor(parent: HTMLElement, doc: string, onChange: (text: string) => void): YamlEditor {
  let silent = false;
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        basicSetup,
        yaml(),
        chrome,
        ...(prefersDark() ? [syntaxHighlighting(oneDarkHighlightStyle)] : []),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && !silent) onChange(update.state.doc.toString());
        }),
      ],
    }),
  });

  return {
    setDoc(text) {
      silent = true;
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
      silent = false;
    },
    showError(line, message) {
      if (!message) {
        view.dispatch(setDiagnostics(view.state, []));
        return;
      }
      const target = view.state.doc.line(Math.min(Math.max(line ?? 1, 1), view.state.doc.lines));
      view.dispatch(setDiagnostics(view.state, [{ from: target.from, to: target.to, severity: 'error', message }]));
    },
  };
}
