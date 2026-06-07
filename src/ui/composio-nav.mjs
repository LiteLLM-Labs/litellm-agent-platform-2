import { chromium } from 'playwright';

const browser = await chromium.launchPersistentContext(
  '/Users/krrishdholakia/Library/Application Support/Google/Chrome/Default',
  {
    headless: false,
    channel: 'chrome',
    args: ['--no-sandbox'],
  }
);

const page = await browser.newPage();
await page.goto('https://dashboard.composio.dev/krrishdholakia_workspace/krrishdholakia_workspace_first_project/auth-configs?create=true&toolkit=gmail');
await page.waitForLoadState('networkidle');
await page.screenshot({ path: '/tmp/composio-auth.png' });
console.log('screenshot saved to /tmp/composio-auth.png');
await browser.close();
