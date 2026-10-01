import { aggregate, parseConfig } from './engine';
import { DEFAULT_EXAMPLE_ID, EXAMPLES, findExample, type Example } from './examples';
import { latestOnly, runPipeline, type PipelineResult } from './pipeline';
import { errorMessage, fetchText, guardLoader, memoizeLoader } from './resolve';
import { loadSettings, saveSettings } from './settings';
import { decodeShare, encodeShare, MAX_SHARE_LENGTH, shareParam } from './share';
import type { Problem, WorkspaceFile } from './types';
import { createYamlEditor } from './ui/editor';
import { showReference } from './ui/reference';
import { fileNameFromUrl, isNameTaken, uniqueFileName, upsertFile } from './workspace';

const NEW_FILE = `openapi: 3.0.3
info:
  title: New API
  version: 1.0.0
paths: {}
`;

function $<T extends HTMLElement = HTMLElement>(selector: string): T {
  const el = document.querySelector<T>(selector);
  if (!el) throw new Error(`missing element ${selector}`);
  return el;
}

function button(label: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.textContent = label;
  b.onclick = onClick;
  return b;
}

export async function startApp(): Promise<void> {
  const settings = loadSettings();
  const state = { config: '', files: [] as WorkspaceFile[], selected: 0 };
  let result: PipelineResult = { text: null, format: 'yaml', problems: [] };
  let activeTab = 'merged';
  let timer: ReturnType<typeof setTimeout> | undefined;
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  // Configs from share links may not fetch remote URLs until the user allows it.
  let remoteAllowed = true;
  const newLoader = () =>
    guardLoader(
      memoizeLoader((url, headers) => fetchText(url, headers, settings)),
      () => remoteAllowed,
    );
  let load = newLoader();
  const pipeline = latestOnly(() => runPipeline(state.config, state.files, { parseConfig, aggregate, load }));

  const configEditor = createYamlEditor($('#config-editor'), '', (text) => {
    state.config = text;
    schedule();
  });
  const fileEditor = createYamlEditor($('#file-editor'), '', (text) => {
    const file = state.files[state.selected];
    if (file) {
      file.content = text;
      schedule();
    }
  });

  function notice(message: string): void {
    const el = $('#notice');
    el.textContent = message;
    el.hidden = false;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (el.hidden = true), 8000);
  }

  function schedule(): void {
    clearTimeout(timer);
    timer = setTimeout(run, 300);
  }

  async function run(): Promise<void> {
    const next = await pipeline();
    if (!next) return; // superseded by a newer run
    result = next;
    $('#output').textContent = result.text ?? 'No output. See the Problems tab.';
    renderProblems(result.problems);
    renderConsent();
    const configError = result.problems.find((p) => p.kind === 'config');
    configEditor.showError(configError?.line, configError?.message);
    if (activeTab === 'reference' && result.text) void showReference($('#reference'), result.text);
  }

  function renderProblems(problems: Problem[]): void {
    $('#problem-count').textContent = problems.length ? `(${problems.length})` : '';
    $('#problems').replaceChildren(
      ...problems.map((p) => {
        const li = document.createElement('li');
        li.className = p.level;
        li.textContent = (p.source ? `[${p.source}] ` : '') + p.message;
        return li;
      }),
    );
  }

  function renderConsent(): void {
    const blocked = !remoteAllowed && result.problems.some((p) => p.kind === 'blocked');
    $('#consent').hidden = !blocked;
    if (!blocked) return;
    let hosts: string[] = [];
    try {
      hosts = parseConfig(state.config).sources.flatMap((s) => {
        try {
          return s.url ? [new URL(s.url).host] : [];
        } catch {
          return [s.url ?? ''];
        }
      });
    } catch {
      // config error: nothing to list
    }
    $('#consent-text').textContent =
      `This shared config fetches from ${[...new Set(hosts)].join(', ')}. ` +
      'Your saved variables would be sent there. Review the config first.';
  }

  function renderFiles(): void {
    $('#files').replaceChildren(
      ...state.files.map((file, index) => {
        const li = document.createElement('li');
        if (index === state.selected) li.className = 'selected';
        const name = button(file.name, () => selectFile(index));
        name.className = 'file-name';
        const rename = button('Rename', () => {
          const next = prompt('File name (as referenced by `path:` in the config)', file.name)?.trim();
          if (next && isNameTaken(state.files, index, next)) {
            notice(`A file named '${next}' already exists.`);
          } else if (next) {
            file.name = next;
            renderFiles();
            schedule();
          }
        });
        const remove = button('Delete', () => {
          state.files.splice(index, 1);
          selectFile(Math.min(state.selected, state.files.length - 1));
          schedule();
        });
        li.append(name, rename, remove);
        return li;
      }),
    );
    $('#file-editor').hidden = state.files.length === 0;
  }

  function selectFile(index: number): void {
    state.selected = Math.max(0, index);
    fileEditor.setDoc(state.files[state.selected]?.content ?? '');
    renderFiles();
  }

  function addFile(file: WorkspaceFile): void {
    selectFile(upsertFile(state.files, file));
    schedule();
  }

  function loadWorkspace(config: string, files: WorkspaceFile[]): void {
    state.config = config;
    state.files = files.map((f) => ({ ...f }));
    configEditor.setDoc(config);
    selectFile(0);
    void run();
  }

  function loadExample(example: Example): void {
    remoteAllowed = true;
    $<HTMLSelectElement>('#examples').value = example.id;
    loadWorkspace(example.config, example.files);
  }

  // --- examples ---
  const select = $<HTMLSelectElement>('#examples');
  select.replaceChildren(...EXAMPLES.map((e) => new Option(e.title, e.id)));
  select.onchange = () => {
    history.replaceState(null, '', location.pathname);
    loadExample(findExample(select.value));
  };

  // --- workspace actions ---
  $('#add-file').onclick = () => {
    const name = prompt('File name', 'specs/new.yaml')?.trim();
    if (name) addFile({ name: uniqueFileName(state.files, name), content: NEW_FILE });
  };
  $<HTMLInputElement>('#upload').onchange = async (event) => {
    const input = event.target as HTMLInputElement;
    for (const file of Array.from(input.files ?? [])) addFile({ name: `specs/${file.name}`, content: await file.text() });
    input.value = '';
  };
  $('#import-url').onclick = async () => {
    const url = prompt('Spec URL')?.trim();
    if (!url) return;
    try {
      addFile({ name: `specs/${fileNameFromUrl(url)}`, content: await fetchText(url, {}, settings) });
    } catch (e) {
      notice(errorMessage(e));
    }
  };

  // --- settings ---
  const proxy = $<HTMLInputElement>('#proxy');
  proxy.value = settings.proxy;
  proxy.onchange = () => {
    settings.proxy = proxy.value.trim();
    settingsChanged();
  };

  function settingsChanged(): void {
    saveSettings(settings);
    load = newLoader();
    schedule();
  }

  function renderVariables(): void {
    $('#variables').replaceChildren(
      ...Object.entries(settings.variables).map(([key, value]) => {
        const row = document.createElement('div');
        row.className = 'row';
        const keyInput = Object.assign(document.createElement('input'), { value: key, placeholder: 'NAME' });
        const valueInput = Object.assign(document.createElement('input'), { value, placeholder: 'value', type: 'password' });
        const update = () => {
          delete settings.variables[key];
          if (keyInput.value.trim()) settings.variables[keyInput.value.trim()] = valueInput.value;
          settingsChanged();
          renderVariables();
        };
        keyInput.onchange = update;
        valueInput.onchange = update;
        row.append(keyInput, valueInput, button('Remove', () => {
          delete settings.variables[key];
          settingsChanged();
          renderVariables();
        }));
        return row;
      }),
    );
  }
  $('#add-variable').onclick = () => {
    const name = prompt('Variable name (used as ${NAME})')?.trim();
    if (name) {
      settings.variables[name] = '';
      settingsChanged();
      renderVariables();
    }
  };
  renderVariables();

  // --- output ---
  for (const tab of document.querySelectorAll<HTMLButtonElement>('[role=tab]')) {
    tab.onclick = () => {
      activeTab = tab.dataset.tab!;
      for (const t of document.querySelectorAll<HTMLButtonElement>('[role=tab]')) {
        t.setAttribute('aria-selected', String(t === tab));
      }
      for (const panel of document.querySelectorAll<HTMLElement>('[data-panel]')) {
        panel.hidden = panel.dataset.panel !== activeTab;
      }
      if (activeTab === 'reference' && result.text) void showReference($('#reference'), result.text);
    };
  }
  $('#allow-fetch').onclick = () => {
    remoteAllowed = true;
    $('#consent').hidden = true;
    void run();
  };
  $('#copy-config').onclick = async () => {
    await navigator.clipboard.writeText(state.config);
    notice('Config copied. Run it with: openapi-aggregator -c openapi-aggregator.yaml');
  };
  $('#copy-output').onclick = async () => {
    if (result.text) await navigator.clipboard.writeText(result.text);
  };
  $('#download-output').onclick = () => {
    if (!result.text) return;
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([result.text], { type: 'text/plain' }));
    a.download = `openapi.${result.format}`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  // --- sharing ---
  $('#share').onclick = async () => {
    const encoded = await encodeShare({ v: 1, config: state.config, files: state.files });
    if (encoded.length > MAX_SHARE_LENGTH) {
      notice('This workspace is too large for a link. Download the merged spec and copy the config instead.');
      return;
    }
    const url = `${location.origin}${location.pathname}#s=${encoded}`;
    history.replaceState(null, '', url);
    await navigator.clipboard.writeText(url);
    notice('Share link copied. It contains the config and workspace files, never your variables or proxy.');
  };

  // --- startup ---
  const shared = shareParam(location.hash);
  if (shared) {
    try {
      const payload = await decodeShare(shared);
      remoteAllowed = false;
      select.value = '';
      loadWorkspace(payload.config, payload.files);
      return;
    } catch {
      notice('That share link could not be read, so the default example was loaded instead.');
    }
  }
  loadExample(findExample(DEFAULT_EXAMPLE_ID));
}
