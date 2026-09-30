import { expect, test } from '@playwright/test';

test('cataloga, filtra, organiza, le, anota e cria backup pela interface', async ({ page }) => {
  page.on('dialog', dialog => dialog.accept());
  await page.goto('/');

  await expect(page).toHaveTitle(/Estante Livre/);
  await expect(page.locator('#empty-state')).toBeVisible();

  await page.locator('#scan-button').click();
  await expect(page.locator('#library-summary')).toHaveText('3 livros encontrados', { timeout: 30_000 });
  await expect(page.locator('#book-grid .book-card')).toHaveCount(3);

  // Exercita controles com uma pagina menor usando os mesmos dados sinteticos.
  await page.evaluate(async () => { state.catalog.limit = 2; await loadBooks(); });
  await expect(page.locator('#book-grid .book-card')).toHaveCount(2);
  await expect(page.locator('#catalog-page-label')).toHaveText('1–2 de 3');
  await expect(page.locator('#catalog-previous')).toBeDisabled();
  await page.locator('#catalog-next').click();
  await expect(page.locator('#book-grid .book-card')).toHaveCount(1);
  await expect(page.locator('#catalog-page-label')).toHaveText('3–3 de 3');
  await expect(page.locator('#catalog-next')).toBeDisabled();
  await page.locator('#catalog-previous').click();
  await expect(page.locator('#book-grid .book-card')).toHaveCount(2);
  await page.locator('#search').fill('Sertao');
  await expect(page.locator('#book-grid .book-card')).toHaveCount(1);
  await expect(page.locator('#book-grid')).toContainText('Grande Sertao Vereda');
  await expect(page.locator('#catalog-pagination')).toBeHidden();
  await page.evaluate(() => { state.catalog.limit = 60; });
  await page.locator('#clear-filters').click();
  await expect(page.locator('#book-grid .book-card')).toHaveCount(3);

  await page.locator('#settings-button').click();
  await expect(page.locator('#settings-dialog')).toBeVisible();
  await expect(page.locator('#health-grid')).toContainText('3');
  await page.locator('#collection-form input[name="name"]').fill('Favoritos E2E');
  await page.locator('#collection-form button[type="submit"]').click();
  await expect.poll(() => page.locator('#collection-list input[name="name"]')
    .evaluateAll(inputs => inputs.map(input => input.value)))
    .toContain('Favoritos E2E');
  await page.locator('[data-close="settings-dialog"]').click();

  await page.locator('#search').fill('Sertao');
  await expect(page.locator('#book-grid .book-card')).toHaveCount(1);
  await page.locator('#book-grid .book-card').click();
  await expect(page.locator('#book-dialog')).toBeVisible();
  await page.getByRole('checkbox', { name: 'Favoritos E2E' }).check();
  await page.locator('#read-book').click();
  await expect(page.locator('#reader')).toBeVisible();
  await expect(page.frameLocator('iframe[title="Leitor de texto"]').locator('body'))
    .toContainText('Fixture local para validar catalogacao');
  await expect(page.frameLocator('iframe[title="Leitor de texto"]').locator('body')).toHaveCSS('margin', '0px');
  await expect(page.frameLocator('iframe[title="Leitor de texto"]').locator('pre')).toHaveCSS('white-space', 'pre-wrap');

  // Dados legados devem aparecer como texto mesmo sem passar pela API atual.
  await page.route('**/api/books/*/notes', route => route.fulfill({
    json: { notes: [{ id: 999, content: 'Nota legada', updated_at: '2026-09-30',
      location: { type: 'page', page: '<b id="injected-note">HTML legado</b>' } }] },
  }));
  await page.evaluate(() => loadNotes());
  await expect(page.locator('#notes-list')).toContainText('<b id="injected-note">HTML legado</b>');
  await expect(page.locator('#injected-note')).toHaveCount(0);
  await page.unroute('**/api/books/*/notes');
  await page.locator('#percent-input').fill('37');
  await page.locator('#percent-input').dispatchEvent('change');

  await page.locator('#notes-toggle').click();
  await page.locator('#note-form textarea').fill('Anotacao criada pelo teste de navegador.');
  await page.locator('#note-form button[type="submit"]').click();
  await expect(page.locator('#notes-list')).toContainText('Anotacao criada pelo teste de navegador.');
  await page.locator('#reader-close').click();
  await expect(page.locator('#reading-desk')).toBeVisible();
  await page.reload();
  await page.locator('#reading-desk-list [data-book-id]').first().click();
  await page.locator('#read-book').click();
  await expect(page.locator('#percent-input')).toHaveValue('37');
  await page.locator('#reader-close').click();
  await expect(page.locator('#reader')).toBeHidden();

  await page.locator('#settings-button').click();
  await page.locator('summary', { hasText: 'Backup e restauração' }).click();
  await page.locator('#create-backup').click();
  await expect(page.locator('#backup-list .backup-row')).toHaveCount(1);

  await page.setViewportSize({ width: 390, height: 844 });
  const overflows = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
  expect(overflows).toBe(false);
});
