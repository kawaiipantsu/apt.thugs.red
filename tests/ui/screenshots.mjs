// Documentation captures use the disposable integration harness, never a live deployment.
import { chromium, expect } from '@playwright/test';
import fs from 'node:fs';

const publicOrigin = process.env.XXC_TEST_ORIGIN;
const adminOrigin = process.env.XXC_TEST_ADMIN;
const credentials = JSON.parse(fs.readFileSync(process.env.XXC_TEST_LOGIN, 'utf8'));
const output = '.build/screenshots';
fs.mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined });
try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    deviceScaleFactor: 1,
    reducedMotion: 'reduce',
  });
  const origins = new Set([publicOrigin, adminOrigin]);
  await context.route('**/*', route => origins.has(new URL(route.request().url()).origin)
    ? route.continue() : route.abort());
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  async function visit(origin, path, heading) {
    const response = await page.goto(origin + path);
    expect(response.status()).toBe(200);
    if (heading) await expect(page.getByRole('heading', { level: 1 })).toHaveText(heading);
    await page.locator('main').waitFor();
  }
  async function capture(name, fullPage = true) {
    await page.evaluate(() => document.fonts.ready);
    await expect(page.locator('input[type="password"]')).toHaveCount(0);
    await page.screenshot({
      path: `${output}/${name}.png`,
      fullPage,
      animations: 'disabled',
      // Even disposable signing fingerprints are hidden in distributed images.
      mask: [
        page.locator('dl > div').filter({ has: page.locator('dt').filter({ hasText: /fingerprint/i }) }).locator('dd'),
        page.locator('section[aria-label="keys records"] p'),
      ],
      maskColor: '#34383e',
    });
  }
  await visit(publicOrigin, '/', 'packages from the edge.');
  await capture('public-home');
  await page.setViewportSize({ width: 390, height: 844 });
  await capture('public-mobile');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await visit(publicOrigin, '/packages');
  await capture('public-packages');
  await visit(publicOrigin, '/packages/xxc-fixture', 'xxc-fixture');
  await capture('public-package');
  await visit(publicOrigin, '/repo/dists/zerotrust/');
  await expect(page.getByRole('link', { name: 'InRelease', exact: true })).toBeVisible();
  await capture('public-repository');
  await visit(publicOrigin, '/help');
  await capture('public-setup');
  await visit(publicOrigin, '/api', 'build. stage. publish.');
  await capture('public-api', false);

  await visit(adminOrigin, '/admin/login');
  await expect(page.getByLabel('password', { exact: true })).toHaveValue('');
  await page.screenshot({ path: `${output}/admin-login.png`, fullPage: true });
  await page.getByLabel('username', { exact: true }).fill(credentials.username);
  await page.getByLabel('password', { exact: true }).fill(credentials.password);
  await page.getByRole('button', { name: 'sign in →' }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('repository control');
  await capture('admin-dashboard');
  await page.setViewportSize({width:390,height:844});
  await capture('admin-statistics-mobile');
  await page.setViewportSize({width:1440,height:1000});
  await visit(adminOrigin,'/admin/tokens');
  await capture('admin-tokens');
  await visit(adminOrigin, '/admin/keys');
  await capture('admin-keys');
  await visit(adminOrigin, '/admin/uploads');
  await page.getByLabel('package file (.deb)').setInputFiles(process.env.XXC_TEST_PACKAGE);
  await page.getByRole('button', { name: 'upload and inspect →' }).click();
  await expect(page.getByRole('button', { name: 'stage xxc-browser-fixture', exact: true })).toBeVisible();
  await capture('admin-uploads');
  await page.getByRole('button', { name: 'stage xxc-browser-fixture', exact: true }).click();
  await page.getByRole('link', { name: 'review publication →', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('review publication');
  await capture('admin-publish');
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  await capture('admin-mobile');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole('button', { name: 'publish reviewed changes →' }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('job result');
  await expect.poll(async () => {
    await page.reload();
    return page.locator('#job-state').textContent();
  }).toBe('succeeded');
  await capture('admin-job');
  expect(errors).toEqual([]);
  await context.close();
  console.log(`Captured 16 fixture screenshots in ${output}; review before copying into docs/screenshots.`);
} finally {
  await browser.close();
}
