import { aggregate, parseConfig } from './engine';
import { DEFAULT_EXAMPLE_ID, EXAMPLES, findExample, type Example } from './examples';
import { latestOnly, runPipeline, type PipelineResult } from './pipeline';
import { errorMessage, fetchText, guardLoader, memoizeLoader } from './resolve';
import { loadSettings, saveSettings } from './settings';
import { decodeShare, encodeShare, MAX_SHARE_LENGTH, shareParam } from './share';
import { fileHues, problemCountLabel, sourceChips, statusOf } from './sources';
import type { Problem, SourceSummary, WorkspaceFile } from './types';
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

const ICONS = {
  pencil: '<path d="M4 20h4L19 9l-4-4L4 16v4Z" />',
  trash: '<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />',
  x: '<path d="M6 6l12 12M18 6 6 18" />',
  alert: '<path d="M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm0 5v5m0 3v.5" />',
} as const;

/** An SVG icon from the constant set above (never user content). */
function icon(name: keyof typeof ICONS): SVGSVGElement {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add('icon');
  svg.innerHTML = ICONS[name];
  return svg;
}

function iconButton(name: keyof typeof ICONS, label: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.className = 'icon-btn';
  b.title = label;
  b.setAttribute('aria-label', label);
  b.append(icon(name));
  b.onclick = onClick;
  return b;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

const INTRO_KEY = 'openapi-aggregator.intro-dismissed';

export async function startApp(): Promise<void> {
  const settings = loadSettings();
  const state = { config: '', files: [] as WorkspaceFile[], selected: 0 };
  let result: PipelineResult = { text: null, format: 'yaml', problems: [] };
  let activeTab = 'merged';
  let activeEditor: 'config' | 'file' = 'config';
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

  function currentSources(): SourceSummary[] {
    try {
      return parseConfig(state.config).sources;
    } catch {
      return [];
    }
  }

  async function run(): Promise<void> {
    const status = $('#status');
    status.dataset.state = 'busy';
    $('#status-label').textContent = 'Merging';
    const next = await pipeline();
    if (!next) return; // superseded by a newer run
    result = next;
    $('#output').textContent = result.text ?? 'No merged spec yet. Open the Problems tab to see what needs fixing.';
    renderProblems(result.problems);
    renderConsent();
    renderStatus();
    renderFiles();
    const configError = result.problems.find((p) => p.kind === 'config');
    configEditor.showError(configError?.line, configError?.message);
    if (activeTab === 'reference' && result.text) void showReference($('#reference'), result.text);
  }

  function renderProblems(problems: Problem[]): void {
    $('#problem-count').textContent = problems.length ? String(problems.length) : '';
    if (!problems.length) {
      $('#problems').replaceChildren(el('li', 'empty', 'No problems. Every source loaded and merged cleanly.'));
      return;
    }
    $('#problems').replaceChildren(
      ...problems.map((p) => {
        const li = el('li', `problem ${p.level}`);
        li.append(icon('alert'), el('span', 'problem-source', p.source ?? (p.kind === 'config' ? 'config' : 'merge')));
        li.append(el('span', 'problem-message', p.message));
        return li;
      }),
    );
  }

  function renderStatus(): void {
    const { state: runState, label } = statusOf(result);
    $('#status').dataset.state = runState;
    $('#status-label').textContent = label;
    $('#status-sources').replaceChildren(
      ...sourceChips(currentSources()).map((chip) => {
        const span = el('span', `chip hue-${chip.hue}`);
        span.title = chip.kind === 'url' ? 'Fetched from a URL' : 'Workspace file';
        span.append(el('span', 'dot'), chip.name);
        return span;
      }),
    );
    $('#status-problems').textContent = problemCountLabel(result.problems.length);
    $('#status-format').textContent = result.format.toUpperCase();
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
    const hues = fileHues(currentSources(), state.files);
    $('#files').replaceChildren(
      ...state.files.map((file, index) => {
        const li = el('li', index === state.selected && activeEditor === 'file' ? 'file selected' : 'file');
        const open = el('button', 'file-open');
        open.type = 'button';
        const hue = hues[index];
        const dot = el('span', hue === null ? 'dot' : `dot hue-${hue}`);
        dot.title = hue === null ? 'Not used by the config' : 'Used by the config';
        open.append(dot, el('span', '', file.name));
        open.onclick = () => selectFile(index, true);
        const rename = iconButton('pencil', `Rename ${file.name}`, () => {
          const next = prompt('File name (as referenced by `path:` in the config)', file.name)?.trim();
          if (next && isNameTaken(state.files, index, next)) {
            notice(`A file named '${next}' already exists.`);
          } else if (next) {
            file.name = next;
            renderFiles();
            schedule();
          }
        });
        const remove = iconButton('trash', `Delete ${file.name}`, () => {
          state.files.splice(index, 1);
          selectFile(Math.min(state.selected, state.files.length - 1), activeEditor === 'file' && state.files.length > 0);
          schedule();
        });
        const actions = el('span', 'file-actions');
        actions.append(rename, remove);
        li.append(open, actions);
        return li;
      }),
    );
    $('#files-empty').hidden = state.files.length > 0;
    const fileTab = $('#file-tab');
    fileTab.hidden = state.files.length === 0;
    fileTab.textContent = state.files[state.selected]?.name ?? '';
    if (!state.files.length && activeEditor === 'file') showEditor('config');
  }

  function showEditor(which: 'config' | 'file'): void {
    activeEditor = which;
    for (const tab of document.querySelectorAll<HTMLElement>('.editor-tabs [role=tab]')) {
      tab.setAttribute('aria-selected', String(tab.dataset.editor === which));
    }
    for (const panel of document.querySelectorAll<HTMLElement>('[data-editor-panel]')) {
      panel.hidden = panel.dataset.editorPanel !== which;
    }
    $('#copy-config').hidden = which !== 'config';
    renderFiles();
  }

  function selectFile(index: number, open = false): void {
    state.selected = Math.max(0, index);
    fileEditor.setDoc(state.files[state.selected]?.content ?? '');
    if (open) showEditor('file');
    else renderFiles();
  }

  function addFile(file: WorkspaceFile): void {
    selectFile(upsertFile(state.files, file), true);
    schedule();
  }

  function loadWorkspace(config: string, files: WorkspaceFile[]): void {
    state.config = config;
    state.files = files.map((f) => ({ ...f }));
    configEditor.setDoc(config);
    selectFile(0);
    showEditor('config');
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
        const row = el('div', 'variable');
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
        keyInput.setAttribute('aria-label', 'Variable name');
        valueInput.setAttribute('aria-label', `Value of ${key}`);
        row.append(keyInput, valueInput, iconButton('x', `Remove ${key}`, () => {
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

  // --- editor tabs ---
  for (const tab of document.querySelectorAll<HTMLButtonElement>('.editor-tabs [role=tab]')) {
    tab.onclick = () => showEditor(tab.dataset.editor as 'config' | 'file');
  }

  // --- output ---
  const outputTabs = document.querySelectorAll<HTMLButtonElement>('.output-tabs [role=tab]');
  for (const tab of outputTabs) {
    tab.onclick = () => {
      activeTab = tab.dataset.tab!;
      for (const t of outputTabs) t.setAttribute('aria-selected', String(t === tab));
      for (const panel of document.querySelectorAll<HTMLElement>('[data-panel]')) {
        panel.hidden = panel.dataset.panel !== activeTab;
      }
      for (const actions of document.querySelectorAll<HTMLElement>('[data-panel-actions]')) {
        actions.hidden = actions.dataset.panelActions !== activeTab;
      }
      if (activeTab === 'reference' && result.text) void showReference($('#reference'), result.text);
    };
  }

  // --- intro ---
  const intro = $('#intro');
  try {
    intro.hidden = localStorage.getItem(INTRO_KEY) === '1';
  } catch {
    intro.hidden = false;
  }
  $('#dismiss-intro').onclick = () => {
    intro.hidden = true;
    try {
      localStorage.setItem(INTRO_KEY, '1');
    } catch {
      // storage blocked: the strip just comes back next visit
    }
  };
  $('#allow-fetch').onclick = () => {
    remoteAllowed = true;
    $('#consent').hidden = true;
    void run();
  };
  $('#copy-config').onclick = async () => {
    await navigator.clipboard.writeText(state.config);
    notice('Config copied. Save it as openapi-aggregator.yaml and run openapi-aggregator.');
  };
  $('#copy-output').onclick = async () => {
    if (!result.text) return;
    await navigator.clipboard.writeText(result.text);
    notice('Merged spec copied.');
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
    notice('Share link copied. It includes the config and files, but not your variables or proxy.');
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
