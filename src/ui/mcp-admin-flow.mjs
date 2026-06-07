import { chromium } from 'playwright';

const browser = await chromium.launch({ headless: false, slowMo: 600 });
const page = await browser.newPage();

await page.goto('http://127.0.0.1:3211/');
await page.evaluate(() => sessionStorage.setItem('lite-harness-master-key', 'sk-local'));

await page.goto('http://127.0.0.1:3211/mcp-servers/');
await page.waitForLoadState('networkidle');
await page.screenshot({ path: '/tmp/01-mcp-servers.png' });
console.log('01: MCP Servers page');

await page.getByRole('button', { name: /add server/i }).first().click();
await page.waitForTimeout(600);
await page.screenshot({ path: '/tmp/02-modal.png' });
console.log('02: Modal open');

await page.locator('input[placeholder="my-mcp-server"]').fill('gmail');
await page.locator('input[placeholder="Human-readable shortname"]').fill('gmail');
await page.locator('textarea[placeholder="What this MCP server provides…"]').fill('Gmail inbox access via Composio');
await page.locator('input[placeholder*="example.com"]').fill('https://mcp.composio.dev/gmail');
await page.locator('#mcp-auth-type').selectOption('bearer_token');
await page.locator('input[type="checkbox"]').first().check();
await page.waitForTimeout(400);
await page.locator('input[placeholder="MY_API_KEY, MY_SECRET"]').fill('COMPOSIO_API_KEY');
await page.locator('input[placeholder*="docs.example.com"]').fill('https://composio.dev/docs/api-key');

await page.screenshot({ path: '/tmp/03-form-filled.png' });
console.log('03: Form filled');

// Button is "Add server" when creating
await page.getByRole('button', { name: 'Add server' }).click();
await page.waitForTimeout(2000);
await page.screenshot({ path: '/tmp/04-saved.png' });
console.log('04: Server created');

await browser.close();
