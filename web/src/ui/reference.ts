interface ReferenceApp {
  updateConfiguration(config: { content: string }): void;
}

let app: ReferenceApp | undefined;
let loading: Promise<void> | undefined;

/** Render `content` with Scalar. The library is loaded on first use only. */
export async function showReference(el: HTMLElement, content: string): Promise<void> {
  if (app) {
    app.updateConfiguration({ content });
    return;
  }
  loading ??= (async () => {
    const { createApiReference } = await import('@scalar/api-reference');
    await import('@scalar/api-reference/style.css');
    app = createApiReference(el, { content }) as unknown as ReferenceApp;
  })();
  await loading;
}
