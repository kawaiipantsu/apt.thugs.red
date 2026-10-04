import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import fs from 'node:fs';

test('home, search, package, setup and directory navigation without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false, ignoreHTTPSErrors: process.env.XXC_TEST_SELF_SIGNED_PROXY === '1' });
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
  const response = await request.get(process.env.XXC_TEST_ADMIN + '/admin/api/v1/status', {
    headers: { 'X-Remote-User': 'administrator', 'X-Forwarded-For': '127.0.0.1' },
  });
  expect(response.status()).toBe(401);
  expect((await request.post(process.env.XXC_TEST_ADMIN + '/admin/api/v1/repository/publish')).status()).toBe(401);
});


test('scrollbar changes preserve horizontal layout', async ({page}) => {
  await page.setViewportSize({width:1440,height:1200});
  await page.goto('/about');
  const before=await page.locator('.brand').boundingBox();
  expect(await page.evaluate(()=>getComputedStyle(document.documentElement).scrollbarGutter)).toBe('stable');
  await page.evaluate(()=>{const spacer=document.createElement('div');spacer.style.height='3000px';document.body.append(spacer);});
  const after=await page.locator('.brand').boundingBox();
  expect(after.x).toBe(before.x);expect(after.width).toBe(before.width);
});


test('public API guide is readable, accessible and offers machine references', async ({browser, page, request}) => {
  await page.goto('/');
  await page.getByRole('navigation',{name:'main',exact:true}).getByRole('link',{name:'/api',exact:true}).click();
  await expect(page.getByRole('heading',{level:1})).toHaveText('build. stage. publish.');
  await page.getByRole('navigation',{name:'API guide contents',exact:true}).getByRole('link',{name:'CI example',exact:true}).click();
  await expect(page).toHaveURL(/\/api#ci-example$/);
  const first=page.locator('.doc-code [data-copy]').first();
  await expect(first).toBeVisible();
  await first.click();
  await expect(page.locator('#copy-status')).toHaveText(/copied to clipboard|select and copy/);
  for (const width of [1440,390]) {
    await page.setViewportSize({width,height:900});
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();
    expect((await new AxeBuilder({page}).withTags(['wcag2a','wcag2aa','wcag21aa']).analyze()).violations).toEqual([]);
    if(width===390) {
      const sample=page.locator('.doc-code pre').filter({hasText:'set +x'});
      await sample.focus();await page.keyboard.press('ArrowRight');
      await expect.poll(()=>sample.evaluate(node=>node.scrollLeft)).toBeGreaterThan(0);
    }
  }
  const markdown=await request.get('/api/guide.md');
  expect(markdown.status()).toBe(200);
  expect(markdown.headers()['content-type']).toContain('text/markdown');
  expect(await markdown.text()).toContain('Integration checklist for coding agents');
  const spec=await request.get('/api/openapi.json');
  expect(spec.status()).toBe(200);
  expect((await spec.json()).paths['/api/v1/uploads'].post.requestBody).toBeTruthy();
  expect((await request.post('/api/v1/uploads',{data:'not a package'})).status()).toBe(404);
  const context=await browser.newContext({javaScriptEnabled:false,ignoreHTTPSErrors:process.env.XXC_TEST_SELF_SIGNED_PROXY==='1'});
  const plain=await context.newPage();await plain.goto(process.env.XXC_TEST_ORIGIN+'/api');
  await expect(plain.getByRole('heading',{name:'Workflow and responses',exact:true})).toBeVisible();
  await expect(plain.locator('.doc-code').filter({hasText:'set +x'})).toBeVisible();
  await expect(plain.getByRole('link',{name:'OpenAPI JSON ↓',exact:true})).toBeVisible();
  await context.close();
});
