interface ReferenceApp {
  updateConfiguration(config: { content: string }): void;
}

let app: ReferenceApp | undefined;
let loading: Promise<void> | undefined;
let latest = '';
let shown = '';

async function load(el: HTMLElement): Promise<void> {
  const { createApiReference } = await import('@scalar/api-reference');
  await import('@scalar/api-reference/style.css');
  app = createApiReference(el, {
    content: latest,
    showDeveloperTools: 'never',
    agent: { disabled: true },
    telemetry: false,
    withDefaultFonts: false,
    darkMode: globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false,
    hideDarkModeToggle: true,
    customCss: '.scalar-app { --scalar-font: var(--font-sans); --scalar-font-code: var(--font-mono); }',
  }) as unknown as ReferenceApp;
  shown = latest;
}

/** Render `content` with Scalar. The library is loaded on first use only. */
export async function showReference(el: HTMLElement, content: string): Promise<void> {
  latest = content;
  if (!app) {
    loading ??= load(el).catch((error: unknown) => {
      loading = undefined; // let the next call retry
      throw error;
    });
    await loading;
  }
  // Content may have changed while the library was loading.
  if (app && shown !== latest) {
    app.updateConfiguration({ content: latest });
    shown = latest;
  }
}
