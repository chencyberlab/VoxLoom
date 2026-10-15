import { defineConfig } from '@playwright/test';
import { existsSync } from 'node:fs';
// PLAYWRIGHT_CHROME overrides; otherwise system Chrome, else Playwright's own browser.
const chrome=process.env.PLAYWRIGHT_CHROME||(existsSync('/opt/google/chrome/chrome')?'/opt/google/chrome/chrome':undefined);
export default defineConfig({testDir:'tests/ui',workers:1,use:{baseURL:'http://127.0.0.1:1420',viewport:{width:1240,height:900},launchOptions:{executablePath:chrome,args:['--no-sandbox']}},webServer:{command:'npm run dev',url:'http://127.0.0.1:1420',reuseExistingServer:true},reporter:'list'});
