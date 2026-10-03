import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import fs from 'node:fs';

test('home, search, package, setup and directory navigation without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(process.env.XXC_TEST_ORIGIN);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('packages from the edge.');
  await page.getByLabel('find a package').fill('searchable');
  await page.getByRole('button', { name: 'search →' }).click();
  await page.getByRole('heading', { name: 'xxc-fixture' }).getByRole('link').click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('xxc-fixture');
  await expect(page.getByText('sudo apt install xxc-fixture', { exact: true })).toBeVisible();
  await page.getByRole('navigation').getByRole('link', { name: '/repository' }).click();
  await page.getByRole('link', { name: 'dists/', exact: true }).click();
  await page.getByRole('link', { name: 'zerotrust/', exact: true }).click();
  await expect(page.getByRole('link', { name: 'InRelease', exact: true })).toBeVisible();
  await page.getByRole('navigation').getByRole('link', { name: '/setup' }).click();
  await expect(page.getByText('Types: deb', { exact: false })).toBeVisible();
  await context.close();
});

test('desktop and mobile keyboard access, contrast and layout', async ({ page }) => {
  fs.mkdirSync('.build/ui', { recursive: true });
  for (const [name, width, height] of [['desktop', 1440, 1100], ['mobile', 390, 844]]) {
    await page.setViewportSize({ width, height });
    await page.goto('/');
    await page.keyboard.press('Tab');
    await expect(page.getByRole('link', { name: 'skip to content' })).toBeFocused();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
    expect(results.violations).toEqual([]);
    await page.getByRole('heading', { level: 1 }).click();
    await page.screenshot({ path: `.build/ui/${name}.png`, fullPage: true });
  }
  for (const path of ['/packages/xxc-fixture', '/repo/', '/help', '/about']) {
    await page.goto(path);
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa']).analyze();
    expect(results.violations).toEqual([]);
  }
});

test('private routes and spoofed identity remain unavailable', async ({ request }) => {
  expect((await request.get('/api/v1/status')).status()).toBe(404);
  const response = await request.get(process.env.XXC_TEST_ADMIN + '/api/v1/status', {
    headers: { 'X-Remote-User': 'administrator', 'X-Forwarded-For': '127.0.0.1' },
  });
  expect(response.status()).toBe(401);
  expect((await request.post(process.env.XXC_TEST_ADMIN + '/api/v1/repository/publish')).status()).toBe(401);
});
