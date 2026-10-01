import { expect, test } from '@playwright/test';
import { encodeShare } from '../src/share';

test('default example merges, reports no problems, and renders the reference', async ({ page }) => {
  await page.goto('./');
  await expect(page.locator('#output')).toContainText('catalog_Item');
  await expect(page.locator('#problem-count')).toHaveText('');

  await page.getByRole('tab', { name: 'API reference' }).click();
  await expect(page.locator('#reference')).toContainText('Shop API', { timeout: 20_000 });
});

test('editing the config re-runs the merge and surfaces config errors', async ({ page }) => {
  await page.goto('./');
  await expect(page.locator('#output')).toContainText('catalog_Item');

  await page.locator('#config-editor .cm-content').click();
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.type('sources: [{name: x}]');

  await expect(page.locator('#problem-count')).toHaveText('1');
  await page.getByRole('tab', { name: /Problems/ }).click();
  await expect(page.locator('#problems')).toContainText("either 'path' or 'url'");
});

test('an invalid share link falls back to the default example with a notice', async ({ page }) => {
  await page.goto('./#s=not-valid');
  await expect(page.locator('#notice')).toContainText('share link');
  await expect(page.locator('#output')).toContainText('catalog_Item');
});

test('a shared config does not fetch remote URLs until the user allows it', async ({ page }) => {
  let hits = 0;
  await page.route('https://remote.test/**', (route) => {
    hits += 1;
    return route.fulfill({
      status: 200,
      headers: { 'access-control-allow-origin': '*' },
      body: "openapi: 3.0.3\ninfo: {title: Remote API, version: '1'}\npaths: {}\n",
    });
  });
  const encoded = await encodeShare({
    v: 1,
    config: 'sources:\n  - name: remote\n    url: https://remote.test/spec?t=${TOKEN}\n',
    files: [],
  });

  await page.goto(`./#s=${encoded}`);
  await expect(page.locator('#consent')).toBeVisible();
  await expect(page.locator('#consent')).toContainText('remote.test');
  expect(hits).toBe(0);

  await page.evaluate(() =>
    localStorage.setItem('openapi-aggregator.settings', JSON.stringify({ proxy: '', variables: { TOKEN: 't' } })),
  );
  await page.reload();
  await page.getByRole('button', { name: 'Allow fetching' }).click();
  await expect(page.locator('#output')).toContainText('title: Remote API');
  expect(hits).toBe(1);
});
