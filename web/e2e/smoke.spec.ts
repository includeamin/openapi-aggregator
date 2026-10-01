import { expect, test } from '@playwright/test';

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

  await expect(page.locator('#problem-count')).toHaveText('(1)');
  await page.getByRole('tab', { name: /Problems/ }).click();
  await expect(page.locator('#problems')).toContainText("either 'path' or 'url'");
});

test('an invalid share link falls back to the default example with a notice', async ({ page }) => {
  await page.goto('./#s=not-valid');
  await expect(page.locator('#notice')).toContainText('share link');
  await expect(page.locator('#output')).toContainText('catalog_Item');
});
