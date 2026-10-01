import { EditorView, basicSetup } from 'codemirror';
import { EditorState } from '@codemirror/state';
import { yaml } from '@codemirror/lang-yaml';
import { setDiagnostics } from '@codemirror/lint';
import { oneDark } from '@codemirror/theme-one-dark';

export interface YamlEditor {
  /** Replace the document without triggering `onChange`. */
  setDoc(text: string): void;
  /** Show (or clear, when `message` is undefined) an error on a 1-based line. */
  showError(line: number | undefined, message: string | undefined): void;
}

function prefersDark(): boolean {
  return globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false;
}

export function createYamlEditor(parent: HTMLElement, doc: string, onChange: (text: string) => void): YamlEditor {
  let silent = false;
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        basicSetup,
        yaml(),
        ...(prefersDark() ? [oneDark] : []),
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
