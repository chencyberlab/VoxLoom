// Visual tour for design work: every page in light and dark, with sample data.
// Opt-in: TOUR_OUT=/some/folder npx playwright test tour
import { test } from '@playwright/test';

const OUT = process.env.TOUR_OUT || '';

for (const theme of ['light', 'dark'] as const) {
  test(`tour ${theme}`, async ({ page }) => {
    test.skip(!OUT, 'Set TOUR_OUT to capture the visual tour');
    test.setTimeout(120000);
    await page.addInitScript((t) => {
      localStorage.setItem('voxloom-appearance', t);
      const meetings = [
        { id: 'a', title: 'Student check-in', created_at: 1792100000, duration_ms: 153000, status: 'ready', notes: 'Seemed tired but open.\n- ask about the study group\n\n# Summary\n\n## Overview\nThe client described a demanding first year in Canada.\n\n## Main concerns\n• Failing two classes.\n• Few chances to meet people.\n\n## Strengths and supports\n• Learning English and interpreting for parents.' },
        { id: 'b', title: 'Untitled recording', created_at: 1792000000, duration_ms: 264000, status: 'ready', notes: '', summary: '' },
      ];
      const segments = Array.from({ length: 8 }, (_, i) => ({ id: i, meeting_id: 'a', start_ms: i * 9000, end_ms: i * 9000 + 6000, source: i % 2 ? 'system' : 'mic', text: ['Yeah, I guess I have to figure out things for myself and for my parents.', 'You have a lot to manage right now.', 'I have to learn and improve my English for school.', 'It sounds like you have a lot of responsibilities.', 'And I don\'t have any friend.', 'Sounds like you had hoped for things to be different.', 'My teacher just told me I\'m failing two classes.', 'Is that correct? Yeah.'][i] }));
      const models = [
        { id: 'whisper', name: 'Whisper', bytes: 147964211, disk_bytes: 147964211, present: true },
        { id: 'parakeet', name: 'Parakeet TDT 0.6B v2', bytes: 600408576, disk_bytes: 0, present: false, optional: true, manual_only: true },
        { id: 'embedding', name: 'EmbeddingGemma', bytes: 484622336, disk_bytes: 484622336, present: true },
        { id: 'summary_e2b', name: 'E2B', bytes: 2841481184, disk_bytes: 2841481184, present: true, optional: true },
        { id: 'summary_e4b', name: 'E4B', bytes: 4590807392, disk_bytes: 0, present: false, optional: true, manual_only: true },
        { id: 'summary_1b', name: 'Gemma 3 · 1B · QAT Q4_0', bytes: 720425600, disk_bytes: 720425600, present: false, retired: true },
      ];
      const hits = segments.slice(0, 4).map((s, i) => ({ meeting_id: 'a', title: 'Student check-in', created_at: 1792100000, start_ms: s.start_ms, end_ms: s.end_ms, text: s.text, modality: i % 2 ? 'audio' : 'text', source: s.source, cosine: 0.8 - i * 0.1 }));
      Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { transformCallback: () => 1, convertFileSrc: (p: string) => p, invoke: async (command: string, args: { action?: string; payload?: { id?: string } }) => {
        if (command.startsWith('plugin:event')) return 1;
        switch (args?.action) {
          case 'library': return meetings;
          case 'jobs': return [{ id: 'j', meeting_id: 'a', kind: 'summarize', status: 'running', progress: 0.42, error: null }];
          case 'devices': return [{ name: 'mic', label: 'Built-in microphone', monitor: false, default: true }, { name: 'mon', label: 'Speakers monitor', monitor: true, default: true }];
          case 'status': return null;
          case 'runtime_status': return { inference: true, whisper: true, llama: true, disk_free_bytes: 6.3e10 };
          case 'models': return { models };
          case 'settings': return { summary_profile: 'e2b', summary_style: 'counselling', transcription_profile: 'standard', appearance: t, threads: '2', language: 'en' };
          case 'detail': return { meeting: meetings.find(m => m.id === (args.payload?.id || 'a')), tracks: [{ id: 't', meeting_id: 'a', source: 'mic', path: 'x.flac', offset_ms: 0 }], segments };
          case 'search': return hits;
          default: return null;
        }
      } }, __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} } });
    }, theme);
    const shot = async (name: string) => { await page.waitForTimeout(250); await page.screenshot({ path: `${OUT}/${theme}-${name}.png`, fullPage: true }); };
    await page.setViewportSize({ width: 1280, height: 860 });
    await page.goto('/');
    await shot('home');
    await page.getByRole('button', { name: /^All recordings/ }).click(); await shot('library');
    await page.getByRole('button', { name: 'Student check-in', exact: false }).first().click(); await shot('detail-notes');
    await page.getByRole('button', { name: 'Transcript', exact: true }).click(); await shot('detail-transcript');
    await page.getByRole('button', { name: 'New recording', exact: true }).first().click(); await shot('studio');
    await page.getByRole('button', { name: 'Search', exact: true }).first().click();
    await page.getByRole('textbox').first().fill('failing classes'); await page.keyboard.press('Enter'); await shot('search');
    await page.getByRole('button', { name: 'Settings', exact: true }).click(); await shot('settings');
    await page.getByRole('button', { name: 'Model setup', exact: true }).click(); await shot('setup');
    await page.keyboard.press('Escape');
    await page.setViewportSize({ width: 900, height: 720 }); await page.getByRole('button', { name: 'Home', exact: true }).click(); await shot('home-narrow');
    await page.setViewportSize({ width: 390, height: 92 }); await page.goto('/?mini=1'); await shot('mini');
  });
}
