import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import fs from 'node:fs';

const origin = process.env.XXC_TEST_ADMIN;
const credentials = JSON.parse(fs.readFileSync(process.env.XXC_TEST_LOGIN, 'utf8'));
async function login(page, username = credentials.username) {
  await page.goto(origin + '/admin/login');
  await page.getByLabel('username', {exact: true}).fill(username);
  await page.getByLabel('password', {exact: true}).fill(credentials.password);
  await page.getByRole('button', {name: 'sign in →'}).click();
  await expect(page.getByRole('heading', {level: 1})).toHaveText('repository control');
}

test('admin upload, inspect, stage, review, publish and result without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false, ignoreHTTPSErrors: process.env.XXC_TEST_SELF_SIGNED_PROXY === '1' });
  const page = await context.newPage();
  await login(page);
  await page.getByRole('navigation',{name:'administration',exact:true}).getByRole('link', {name:'uploads',exact:true}).click();
  await page.getByLabel('package file (.deb)').setInputFiles(process.env.XXC_TEST_PACKAGE);
  await page.getByRole('button', {name:'upload and inspect →'}).click();
  await page.getByRole('button', {name:'stage xxc-browser-fixture',exact:true}).click();
  await expect(page.getByRole('heading', {name:'xxc-browser-fixture',exact:true})).toBeVisible();
  await page.getByRole('link', {name:'review publication →',exact:true}).click();
  await expect(page.getByRole('heading', {level:1})).toHaveText('review publication');
  await expect(page.getByText('packages added', {exact:true})).toBeVisible();
  await page.getByRole('button', {name:'publish reviewed changes →'}).click();
  await expect(page.getByRole('heading', {level:1})).toHaveText('job result');
  await expect.poll(async () => { await page.reload(); return page.locator('#job-state').textContent(); }).toBe('succeeded');
  await page.goto(process.env.XXC_TEST_ORIGIN + '/search?q=xxc-browser-fixture');
  await expect(page.getByRole('heading', {name:'xxc-browser-fixture'})).toBeVisible();
  await page.goto(origin + '/admin/');
  await page.getByRole('button', {name:'sign out',exact:true}).click();
  await page.goto(origin + '/admin/');
  await expect(page).toHaveURL(origin + '/admin/login');
  await context.close();
});

test('admin mobile, keyboard, accessibility and CSRF denial', async ({ page }) => {
  fs.mkdirSync('.build/ui', {recursive:true});
  await page.setViewportSize({width:390,height:844});
  await page.goto(origin + '/admin/login');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('link',{name:'skip to content'})).toBeFocused();
  expect((await new AxeBuilder({page}).withTags(['wcag2a','wcag2aa','wcag21aa']).analyze()).violations).toEqual([]);
  await login(page);
  for (const route of ['/admin/', '/admin/uploads', '/admin/publish', '/admin/users', '/admin/trust', '/admin/keys', '/admin/tokens']) {
    await page.goto(origin + route);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
    expect((await new AxeBuilder({page}).withTags(['wcag2a','wcag2aa','wcag21aa']).analyze()).violations).toEqual([]);
  }
  await page.goto(origin + '/admin/users');
  await page.locator('#new-user').fill('fixture-created');
  await page.locator('#new-password').fill(credentials.password);
  await page.locator('#new-role').selectOption('operator');
  await page.getByRole('button',{name:'create user',exact:true}).click();
  const user = page.locator('.user-record').filter({has:page.getByRole('heading',{name:'fixture-created',exact:true})});
  await expect(user).toContainText('operator / enabled');
  await user.getByRole('button',{name:'disable user',exact:true}).click();
  await expect(user).toContainText('operator / disabled');
  await page.goto(origin + '/admin/publish');
  await page.screenshot({path:'.build/ui/admin-mobile.png',fullPage:true});
  const rejected = await page.request.post(origin + '/admin/api/v1/repository/publish', {data:{review_token:'not-reviewed'},headers:{Origin:origin}});
  expect(rejected.status()).toBe(403);
  expect((await page.request.post(origin+'/admin/tokens',{form:{name:'csrf-denied',scopes:'read',suites:'zerotrust',days:'1'},headers:{Origin:origin}})).status()).toBe(403);
  const form = await page.request.post(origin + '/admin/publish', {form:{review_token:'not-reviewed'},headers:{Origin:origin}});
  expect(form.status()).toBe(403);
  await page.setViewportSize({width:1440,height:1100});
  await page.goto(origin + '/admin/');
  await page.screenshot({path:'.build/ui/admin-desktop.png',fullPage:true});
});

test('viewer can browse administration but cannot stage or manage users', async ({page}) => {
  await login(page, credentials.viewer);
  await page.goto(origin + '/admin/uploads');
  await expect(page.getByRole('button',{name:'upload and inspect →'})).toHaveCount(0);
  expect((await page.goto(origin+'/admin/tokens')).status()).toBe(403);
  const trust = await page.goto(origin + '/admin/trust');
  expect(trust.status()).toBe(403);
  const response = await page.goto(origin + '/admin/users');
  expect(response.status()).toBe(403);
  await expect(page.getByRole('heading',{level:1})).toHaveText('403 / request failed');
});


test('remote OpenPGP generation and public export without JavaScript', async ({browser}) => {
  const context = await browser.newContext({javaScriptEnabled:false, ignoreHTTPSErrors: process.env.XXC_TEST_SELF_SIGNED_PROXY === '1'});
  const page = await context.newPage();
  await login(page);
  await page.goto(origin + '/admin/keys');
  await page.getByLabel('public name',{exact:true}).fill('Fixture browser archive');
  await page.getByLabel('public email',{exact:true}).fill('archive@example.invalid');
  await page.getByRole('button',{name:'generate in XXC Trust →'}).click();
  await page.getByRole('link',{name:'next page →'}).click();
  await expect(page.getByRole('heading',{name:'Fixture browser archive',exact:true})).toBeVisible();
  const link=await page.getByRole('link',{name:'public armor',exact:true}).getAttribute('href');
  const exported=await page.request.get(origin+link);
  expect(exported.status()).toBe(200);
  expect(await exported.text()).toContain('BEGIN PGP PUBLIC KEY BLOCK');
  const refused=await page.request.post(origin+'/admin/api/v1/keys',{data:{name:'forbidden'},headers:{Origin:origin}});
  expect(refused.status()).toBe(403);
  await context.close();
});


test('project tokens are shown once, scoped and revocable', async ({page}) => {
  await login(page);
  await page.goto(origin+'/admin/tokens');
  await page.getByLabel('project name',{exact:true}).fill('browser fixture pipeline');
  await page.getByLabel('allowed suites (space separated)').fill('zerotrust');
  await page.getByRole('button',{name:'create token →',exact:true}).click();
  const secret=await page.getByLabel('one-time bearer token').inputValue();
  expect(secret.startsWith('xxc_aptd_')).toBeTruthy();
  // Use a separate request context: mixed cookies and bearer credentials are refused.
  const ctx=await page.context().browser().newContext({ignoreHTTPSErrors:process.env.XXC_TEST_SELF_SIGNED_PROXY==='1'});
  const url=origin+'/admin/api/v1/packages?suite=zerotrust';
  expect((await ctx.request.get(url,{headers:{Authorization:'Bearer '+secret}})).status()).toBe(200);
  await page.goto(origin+'/admin/tokens');
  expect(await page.content()).not.toContain(secret);
  await page.getByRole('button',{name:'revoke browser fixture pipeline',exact:true}).click();
  expect((await ctx.request.get(url,{headers:{Authorization:'Bearer '+secret}})).status()).toBe(401);
  await ctx.close();
});

test('traffic chart has keyboard inspection and readable data', async ({page}) => {
  await login(page);
  await page.goto(origin+'/admin/?days=7&metric=requests');
  const chart=page.locator('.interactive-chart');
  await chart.focus();await page.keyboard.press('End');
  await expect(page.locator('.chart-readout')).toContainText('requests');
  await page.getByText('daily values',{exact:true}).click();
  await expect(page.locator('.chart-data tbody tr')).toHaveCount(7);
});
