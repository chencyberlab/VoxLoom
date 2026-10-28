import { test, expect, type Page } from '@playwright/test';

test('meeting notes render normally and copy as rich text or Markdown',async({page,context})=>{
  await context.grantPermissions(['clipboard-read','clipboard-write']);
  await page.addInitScript(()=>{
    const meeting={id:'notes',title:'Workshop planning',created_at:1,duration_ms:60000,status:'ready',notes:'Ask about the venue.\n10:30:00 room booked\n\n# Summary\n\n## Overview\nThe workshop covers privacy. [00:00:03]\n\n**Action items**\n- **Sarah** sends the guide on Friday at 09:30.',summary:''};
    Object.assign(window,{isTauri:true,__exports:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      if(command==='plugin:dialog|save')return '/tmp/Meeting notes.docx';
      switch(args?.action){case 'library':return [meeting];case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true},{id:'summary_e2b',present:true}]};case 'settings':return {summary_profile:'1b',summary_style:'meeting'};case 'detail':return {meeting,tracks:[],segments:[{id:1,meeting_id:'notes',start_ms:3000,end_ms:6000,source:'system',text:'The workshop covers privacy.'}]};case 'export':(window as unknown as {__exports:unknown[]}).__exports.push(args.payload);return null;default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');
  // The library marks a recording whose notes hold a summary.
  await expect(page.getByRole('button',{name:'Workshop planning',exact:false}).locator('.tag')).toHaveText('Summary');
  await page.getByRole('button',{name:'Workshop planning',exact:false}).click();
  // A recording opens on its notes: the person's own lines as typed, then the summary.
  await expect(page.getByRole('heading',{name:'Meeting notes.',exact:true})).toBeVisible();
  await expect(page.locator('.meeting-notes p').first()).toHaveText('Ask about the venue.10:30:00 room booked');
  await expect(page.locator('.meeting-notes p').first().locator('br')).toHaveCount(1);
  await expect(page.getByRole('heading',{name:'Summary',exact:true,level:2})).toBeVisible();
  await expect(page.getByRole('heading',{name:'Overview',exact:true})).toBeVisible();
  await expect(page.getByRole('heading',{name:'Action items',exact:true})).toBeVisible();
  await expect(page.locator('.meeting-notes li')).toHaveText('Sarah sends the guide on Friday at 09:30.');
  await expect(page.locator('.meeting-notes')).not.toContainText('[00:00:03]');
  await expect(page.locator('.meeting-notes')).not.toContainText('**');
  await page.getByRole('button',{name:'Copy notes',exact:true}).click();
  await expect(page.getByRole('status')).toContainText('Notes copied');
  const copied=await page.evaluate(async()=>{
    const items=await navigator.clipboard.read();const item=items[0];
    return {plain:await(await item.getType('text/plain')).text(),html:await(await item.getType('text/html')).text()};
  });
  expect(copied.plain).toContain('• Sarah sends the guide on Friday at 09:30.');expect(copied.plain).not.toMatch(/\*\*|##|\[00:00:03\]/);
  expect(copied.plain.startsWith('Ask about the venue.\n10:30:00 room booked\n\nSummary')).toBeTruthy();
  expect(copied.html).toContain('<h3');expect(copied.html).toContain('<strong>Sarah</strong>');
  await expect(page.getByRole('button',{name:'Word document'})).toHaveCount(0);
  await page.getByRole('button',{name:'Copy Markdown',exact:true}).click();
  await expect(page.getByRole('status')).toContainText('Markdown copied');
  const markdown=await page.evaluate(async()=>{const items=await navigator.clipboard.read();return {types:items[0].types,text:await navigator.clipboard.readText()};});
  expect(markdown.text).toBe('Ask about the venue.\n10:30:00 room booked\n\n# Summary\n\n## Overview\n\nThe workshop covers privacy.\n\n## Action items\n\n- **Sarah** sends the guide on Friday at 09:30.');
  expect(markdown.types).not.toContain('text/html');
  await page.setViewportSize({width:900,height:720});expect(await page.locator('.meeting-notes').evaluate(element=>element.scrollWidth<=element.clientWidth)).toBeTruthy();
  await page.screenshot({path:'test-results/meeting-notes.png',fullPage:true});
  await page.getByRole('button',{name:'Transcript',exact:true}).click();await expect(page.getByText('The workshop covers privacy.',{exact:true})).toBeVisible();
  await expect(page.getByLabel('Export transcript').locator('option')).not.toContainText(['MD','JSON']);
});

for(const choice of ['e2b','e4b','none'] as const){
  test(`first-run setup downloads the selected summary model: ${choice}`,async({page})=>{
    // This bridge checks UI requests only. Real provisioning and persistence
    // are covered by the Rust offline setup test with integrity-checked files.
    await page.addInitScript(()=>{
      const models=[{id:'whisper',bytes:147964211,present:false},{id:'embedding',bytes:484622336,present:false},{id:'summary_e2b',bytes:2841481184,present:false,optional:true},{id:'summary_e4b',bytes:4590807392,present:false,optional:true,manual_only:true}];
      Object.assign(window,{isTauri:true,__setupCalls:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
        if(command.startsWith('plugin:event'))return 1;
        switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'1b'};case 'setup':(window as unknown as {__setupCalls:unknown[]}).__setupCalls.push(args.payload);return null;default:return null;}
      }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
    });
    await page.goto('/');
    const dialog=page.getByRole('dialog');
    const model=dialog.getByLabel('Summary model',{exact:true});
    await expect(model).toHaveValue('e2b');
    await expect(model.locator('option')).toHaveText([/Gemma 4 E2B/,/Gemma 4 E4B/]);
    await expect(dialog.getByText('Careful notes that keep to what was said',{exact:false})).toBeVisible();
    if(choice==='none'){
      await dialog.getByLabel('Add optional summaries',{exact:false}).uncheck();
      await expect(model).toBeHidden();
      await expect(dialog.getByText('About 0.8 GB in total',{exact:false})).toBeVisible();
    }else{
      await model.selectOption(choice);
      await expect(dialog.getByText(`About ${{e2b:'3.6',e4b:'5.4'}[choice]} GB in total`,{exact:false})).toBeVisible();
      if(choice==='e4b')await expect(dialog.getByText('Needs about 6 GB of free memory',{exact:false})).toBeVisible();
    }
    await page.setViewportSize({width:900,height:720});
    await expect(dialog.getByRole('button',{name:'Download & set up',exact:true})).toBeInViewport({ratio:1});
    await dialog.getByRole('button',{name:'Download & set up',exact:true}).click();
    await expect.poll(()=>page.evaluate(()=>(window as unknown as {__setupCalls:unknown[]}).__setupCalls)).toEqual([{mode:'recommended',core_only:choice==='none',summary_profile:choice==='none'?'e2b':choice,transcription_profile:'standard',path:null}]);
    if(choice!=='none')await expect(model).toBeDisabled();
    expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth)).toBeTruthy();
    if(choice==='e2b')await page.screenshot({path:'test-results/setup-summary-model.png',fullPage:true});
  });
}

test('one notes page: typed in the studio, edited later, a summary added below without losing typing',async({page})=>{
  await page.addInitScript(()=>{
    const w=window as unknown as {__notes:Record<string,string>;__calls:{action:string;payload:Record<string,unknown>}[];__capture:unknown;__segments:unknown[]};
    w.__notes={rec:''};w.__calls=[];w.__capture={meeting_id:'rec',elapsed_ms:60000,paused:false,failed:false,levels:[0,0]};w.__segments=[];
    const meeting=()=>({id:'rec',title:'Check-in',created_at:1,duration_ms:60000,status:'ready',notes:w.__notes.rec,summary:''});
    Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:Record<string,unknown>})=>{
      if(command.startsWith('plugin:event'))return 1;
      const payload=args?.payload||{};
      if(['update','enqueue','stop'].includes(args?.action||''))w.__calls.push({action:args!.action!,payload});
      switch(args?.action){case 'library':return [meeting()];case 'jobs':case 'devices':return [];case 'status':return w.__capture;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};
        case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true},{id:'summary_e2b',present:true}]};case 'settings':return {summary_style:'counselling'};
        case 'detail':return {meeting:meeting(),tracks:[],segments:w.__segments};
        case 'stop':w.__capture=null;w.__segments=[{id:1,meeting_id:'rec',start_ms:0,end_ms:3000,source:'mic',text:'I feel isolated at school.'}];return null;
        case 'update':{
          // As the host does: keep anything added after the notes the page started from.
          const current=w.__notes.rec,from=payload.notes_base as string;let notes=payload.notes as string;
          if(current!==from&&current.startsWith(from))notes+=current.slice(from.length);
          w.__notes.rec=notes;return {notes};}
        default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  const calls=()=>page.evaluate(()=>(window as unknown as {__calls:{action:string;payload:Record<string,unknown>}[]}).__calls);
  await page.goto('/');
  await page.getByRole('navigation').getByRole('button',{name:'New recording',exact:true}).click();
  const pad=page.getByLabel('Notes for this recording',{exact:true});
  await pad.fill('Feels isolated\n- follow up with school');
  await expect.poll(async()=>(await calls()).filter(c=>c.action==='update').at(-1)?.payload).toEqual({id:'rec',notes:'Feels isolated\n- follow up with school',notes_base:''});
  await page.screenshot({path:'test-results/studio-notes.png'});
  // Stop opens the recording on its notes, as typed.
  await page.getByRole('button',{name:'Stop recording',exact:true}).first().click();
  await expect(page.getByRole('heading',{name:'Session notes.',exact:true})).toBeVisible();
  await expect(page.locator('.notes-panel .meeting-notes')).toContainText('Feels isolated');
  await expect(page.locator('.notes-panel .meeting-notes li')).toHaveText('follow up with school');
  // Edit, then Summarise: what was typed is saved before the summary is asked for.
  await page.getByRole('button',{name:'Edit',exact:true}).click();
  const editor=page.getByLabel('Notes',{exact:true});
  await expect(editor).toBeFocused();
  await editor.press('End');await editor.pressSequentially('\nMum to call Friday');
  await page.getByRole('button',{name:'Summarise',exact:true}).click();
  await expect.poll(async()=>(await calls()).filter(c=>['update','enqueue'].includes(c.action)).slice(-2).map(c=>c.action)).toEqual(['update','enqueue']);
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__notes:Record<string,string>}).__notes.rec)).toBe('Feels isolated\n- follow up with school\nMum to call Friday');
  // The summary lands below while the person keeps typing; neither is lost.
  await editor.click();await editor.press('Control+End');await editor.pressSequentially('\nShe smiled at the end');
  await page.evaluate(()=>{const w=window as unknown as {__notes:Record<string,string>};w.__notes.rec+='\n\n# Summary\n\n## Main concerns\n• Feeling isolated at school.';});
  await expect(editor).toHaveValue('Feels isolated\n- follow up with school\nMum to call Friday\nShe smiled at the end\n\n# Summary\n\n## Main concerns\n• Feeling isolated at school.',{timeout:8000});
  await page.getByRole('button',{name:'Done',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__notes:Record<string,string>}).__notes.rec)).toBe('Feels isolated\n- follow up with school\nMum to call Friday\nShe smiled at the end\n\n# Summary\n\n## Main concerns\n• Feeling isolated at school.');
  await expect(page.getByRole('heading',{name:'Summary',exact:true,level:2})).toBeVisible();
  await expect(page.getByRole('heading',{name:'Main concerns',exact:true})).toBeVisible();
  await page.screenshot({path:'test-results/notes-page.png',fullPage:true});
});

test('typing into empty notes keeps the editor open, and the page shows they are saved',async({page})=>{
  await page.addInitScript(()=>{
    const w=window as unknown as {__notes:string};w.__notes='';
    const meeting=()=>({id:'empty',title:'Quiet check-in',created_at:1,duration_ms:60000,status:'ready',notes:w.__notes,summary:''});
    Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:Record<string,unknown>})=>{
      if(command.startsWith('plugin:event'))return 1;
      switch(args?.action){case 'library':return [meeting()];case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};
        case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true},{id:'summary_e2b',present:true}]};case 'settings':return {summary_style:'meeting'};
        case 'detail':return {meeting:meeting(),tracks:[],segments:[]};
        case 'update':w.__notes=args!.payload!.notes as string;return {notes:w.__notes};
        default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');await expect(page.getByRole('button',{name:'Quiet check-in',exact:false}).locator('.tag')).toHaveCount(0);
  await page.getByRole('button',{name:'Quiet check-in',exact:false}).click();
  const editor=page.getByLabel('Notes',{exact:true});
  await editor.click();await editor.pressSequentially('First line');
  // Past the save: the person is still typing in the same field.
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__notes:string}).__notes)).toBe('First line');
  await expect(editor).toBeFocused();await expect(editor).toHaveValue('First line');
  await expect(page.getByRole('button',{name:'Copy notes',exact:true})).toBeVisible();
  await page.getByRole('button',{name:'Done',exact:true}).click();
  await expect(page.locator('.notes-panel .meeting-notes p')).toHaveText('First line');
  await expect(page.getByRole('button',{name:'Edit',exact:true})).toBeVisible();
});

test('summary style defaults to counselling and custom instructions are saved',async({page})=>{
  await page.addInitScript(()=>{
    const meeting={id:'s',title:'Untitled recording',created_at:1,duration_ms:60000,status:'ready',notes:'# Summary\n\n## Main concerns\n• Feeling isolated at school.',summary:''};
    Object.assign(window,{isTauri:true,__calls:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      if(args?.action==='save_settings')(window as unknown as {__calls:unknown[]}).__calls.push(args.payload);
      switch(args?.action){case 'library':return [meeting];case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true},{id:'summary_e2b',present:true}]};case 'settings':return {summary_profile:'1b'};case 'detail':return {meeting,tracks:[],segments:[{id:1,meeting_id:'s',start_ms:0,end_ms:3000,source:'mic',text:'I feel isolated at school.'}]};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');await page.getByRole('button',{name:'Untitled recording',exact:false}).click();
  await expect(page.getByRole('heading',{name:'Session notes.',exact:true})).toBeVisible();
  await expect(page.getByRole('heading',{name:'Main concerns',exact:true})).toBeVisible();
  await expect(page.locator('.meeting-notes')).not.toContainText('##');
  // Progress lives in the top bar, not in a panel below the page.
  await page.getByRole('button',{name:'Summarise',exact:true}).click();
  await expect(page.getByRole('status')).toHaveText('Summary queued. It will be added below your notes; its progress shows at the top.');
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  const style=page.getByLabel('Summary style',{exact:true});
  await expect(style).toHaveValue('counselling');await expect(page.getByText('No diagnosis, labels or advice',{exact:false})).toBeVisible();
  await style.selectOption('custom');
  const box=page.getByLabel('Custom summary instructions',{exact:true});
  await expect(page.getByText('Until you write instructions, the Counselling style is used.',{exact:false})).toBeVisible();
  // Examples for people who have never written instructions: one click fills the box, only after it says it replaces.
  await page.getByText('How to write instructions, with examples',{exact:true}).click();
  await expect(page.getByText('List the headings you want, in order.',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'Use this: Case note',exact:true}).click();
  await expect(box).toHaveValue(/^This is a one-to-one counselling session\. .*Leave out small talk\.$/);
  await expect(page.getByRole('button',{name:'Replace mine with Team meeting',exact:true})).toBeVisible();
  await box.fill('List the goals the client named.');await expect(page.getByText('32 / 2,000',{exact:false})).toBeVisible();
  await page.setViewportSize({width:900,height:720});await page.screenshot({path:'test-results/summary-style.png',fullPage:true});
  await page.getByRole('button',{name:'Save settings',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__calls:Record<string,unknown>[]}).__calls.at(-1))).toMatchObject({summary_style:'custom',summary_prompt:'List the goals the client named.'});
});

test('first-run setup offers Standard or Accurate transcription',async({page})=>{
  await page.addInitScript(()=>{
    const models=[{id:'whisper',bytes:147964211,present:false},{id:'parakeet',bytes:600408576,present:false,optional:true,manual_only:true},{id:'embedding',bytes:484622336,present:false},{id:'summary_e2b',bytes:2841481184,present:false,optional:true},{id:'summary_e4b',bytes:4590807392,present:false,optional:true,manual_only:true}];
    Object.assign(window,{isTauri:true,__setupCalls:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'e2b'};case 'setup':(window as unknown as {__setupCalls:unknown[]}).__setupCalls.push(args.payload);return null;default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');
  const dialog=page.getByRole('dialog');
  const choice=dialog.getByLabel('Transcription model',{exact:true});
  await expect(choice).toHaveValue('standard');
  await expect(choice.locator('option')).toHaveText([/Standard · Fast default · 148 MB/,/Accurate · Fewer misheard words · 600 MB/]);
  const transcriptionSize=dialog.locator('.welcome-model',{has:page.getByLabel('Transcription model',{exact:true})}).locator('.welcome-size');
  await expect(transcriptionSize).toHaveText('148 MB');
  await expect(dialog.getByText('About 3.6 GB in total',{exact:false})).toBeVisible();
  await choice.selectOption('accurate');
  await expect(dialog.getByText('Uses about 1 GB of memory while it writes each transcript.',{exact:false})).toBeVisible();
  // Accurate is installed instead of Standard, not as well: 148 MB fewer than Standard + Accurate.
  await expect(dialog.getByText('About 4.1 GB in total',{exact:false})).toBeVisible();
  await expect(transcriptionSize).toHaveText('600 MB');
  await page.setViewportSize({width:900,height:720});
  await page.setViewportSize({width:900,height:720});
  await expect(dialog.getByRole('button',{name:'Download & set up',exact:true})).toBeInViewport({ratio:1});
  await dialog.getByRole('button',{name:'Download & set up',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__setupCalls:unknown[]}).__setupCalls)).toEqual([{mode:'recommended',core_only:false,summary_profile:'e2b',transcription_profile:'accurate',path:null}]);
  await expect(choice).toBeDisabled();
  expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth)).toBeTruthy();
  await page.screenshot({path:'test-results/setup-transcription-model.png',fullPage:true});
});

test('model setup shows the saved transcription choice',async({page})=>{
  await page.addInitScript(()=>{
    const models=[{id:'whisper',bytes:147964211,present:true},{id:'parakeet',bytes:600408576,present:true,optional:true,manual_only:true},{id:'embedding',bytes:484622336,present:true},{id:'summary_e2b',bytes:2841481184,present:true,optional:true}];
    Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'e2b',transcription_profile:'accurate'};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');
  await expect(page.getByRole('dialog')).toBeHidden();
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  await page.getByRole('button',{name:'Model setup',exact:true}).click();
  const dialog=page.getByRole('dialog');
  await expect(dialog.getByLabel('Transcription model',{exact:true})).toHaveValue('accurate');
  await expect(dialog.locator('.welcome-model',{has:page.getByLabel('Transcription model',{exact:true})}).locator('.welcome-size')).toHaveText('Ready');
});

test('updates are checked only when asked, and only the release page opens',async({page})=>{
  await page.addInitScript(()=>{
    const models=[{id:'whisper',bytes:147964211,present:true},{id:'embedding',bytes:484622336,present:true},{id:'summary_e2b',bytes:2841481184,present:true,optional:true}];
    // Newer, then up to date, then offline.
    const answers:unknown[]=[{current:'0.1.20',latest:'0.9.0',newer:true},{current:'0.1.20',latest:'0.1.20',newer:false},"Couldn't reach GitHub. Check your internet connection and try again."];
    Object.assign(window,{isTauri:true,__calls:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      if(args?.action==='check_updates'||args?.action==='open_release')(window as unknown as {__calls:unknown[]}).__calls.push({action:args.action,payload:args.payload});
      switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'e2b'};
        case 'check_updates':{const answer=answers.shift();if(typeof answer==='string')throw answer;return answer;}
        default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  const calls=()=>page.evaluate(()=>(window as unknown as {__calls:{action:string;payload:unknown}[]}).__calls);
  await page.goto('/');
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  const status=page.locator('.setting-row',{hasText:'Updates'}).locator('small[aria-live="polite"]');
  await expect(status).toContainText('Checking asks GitHub once; nothing from your library is sent.');
  await page.waitForTimeout(500);
  expect(await calls()).toEqual([]);
  const check=page.getByRole('button',{name:'Check for updates',exact:true});
  await check.click();
  await expect(status).toHaveText('VoxLoom 0.9.0 is available. Download it and install it over this version; your recordings, models and settings stay.');
  await page.getByRole('button',{name:'Get VoxLoom 0.9.0',exact:true}).click();
  await expect.poll(calls).toEqual([{action:'check_updates',payload:{}},{action:'open_release',payload:{version:'0.9.0'}}]);
  await check.click();
  await expect(status).toHaveText(/^You have the latest version, \d+\.\d+\.\d+\.$/);
  await expect(page.getByRole('button',{name:/Get VoxLoom/})).toBeHidden();
  await check.click();
  await expect(status).toHaveText("Couldn't reach GitHub. Check your internet connection and try again.");
  await expect(page.getByRole('button',{name:/Get VoxLoom/})).toBeHidden();
  await expect(check).toBeEnabled();
});

test('Accurate alone is a complete setup, and Standard can be added later',async({page})=>{
  await page.addInitScript(()=>{
    const models=[{id:'whisper',bytes:147964211,present:false},{id:'parakeet',bytes:600408576,present:true,optional:true,manual_only:true},{id:'embedding',bytes:484622336,present:true},{id:'summary_e2b',bytes:2841481184,present:true,optional:true}];
    Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'e2b',transcription_profile:'accurate'};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');
  await page.getByRole('button',{name:'New recording',exact:false}).first().click();
  await expect(page.getByText('Offline transcription',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  await expect(page.getByText('Accurate uses about 1 GB of memory',{exact:false})).toBeVisible();
  // Models and settings have loaded, and the missing Standard model asked for no setup.
  await expect(page.getByRole('dialog')).toBeHidden();
  await page.getByLabel('Transcription model',{exact:true}).selectOption('standard');
  await expect(page.getByText('Download “Transcription” in Local intelligence below first. Until then, Accurate is used.',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'Model setup',exact:true}).click();
  // Setup opens on the choice just made: Standard alone is what it would download.
  const dialog=page.getByRole('dialog');const transcriptionSize=dialog.locator('.welcome-model',{has:page.getByLabel('Transcription model',{exact:true})}).locator('.welcome-size');
  await expect(transcriptionSize).toHaveText('148 MB');
  await dialog.getByLabel('Transcription model',{exact:true}).selectOption('accurate');
  await expect(transcriptionSize).toHaveText('Ready');
  await expect(dialog.getByRole('button',{name:'Continue to VoxLoom',exact:true})).toBeVisible();
});

test('with Accurate alone, removing it and an empty transcript name the model actually in use',async({page})=>{
  await page.addInitScript(()=>{
    const meeting={id:'a',title:'Before setup',created_at:1,duration_ms:60000,status:'ready',notes:'',summary:''};
    const models=[{id:'whisper',bytes:147964211,present:false},{id:'parakeet',bytes:600408576,disk_bytes:600408576,present:true,optional:true,manual_only:true},{id:'embedding',bytes:484622336,present:true}];
    Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      switch(args?.action){case 'library':return [meeting];case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'e2b',transcription_profile:'accurate'};case 'detail':return {meeting,tracks:[],segments:[]};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');
  await page.getByRole('button',{name:'Before setup',exact:false}).click();
  await page.getByRole('button',{name:'Transcript',exact:true}).click();
  await expect(page.getByText('Runs on this device with Accurate transcription.',{exact:true})).toBeVisible();
  await expect(page.getByText('Whisper',{exact:false})).toHaveCount(0);
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  const accurate=page.locator('article.model',{hasText:'Transcription · more accurate'});
  await expect(accurate).not.toContainText('Optional upgrade');
  await accurate.getByRole('button',{name:'Remove Transcription · more accurate',exact:true}).click();
  // Standard is not on disk: removing Accurate leaves nothing to transcribe, and the dialog says so.
  const dialog=page.getByRole('dialog');
  await expect(dialog).toContainText('Transcription stops until you download a transcription model again.');
  await expect(dialog).not.toContainText('goes back to Standard');
});

test('accurate transcription is an explicit download and setting',async({page})=>{
  await page.addInitScript(()=>{
    const models=[{id:'whisper',bytes:147964211,present:true},{id:'parakeet',bytes:600408576,present:false,optional:true,manual_only:true},{id:'embedding',bytes:484622336,present:true},{id:'summary_e2b',bytes:2841481184,present:true,optional:true}];
    Object.assign(window,{isTauri:true,__calls:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      if(args?.action==='save_settings'||args?.action==='setup')(window as unknown as {__calls:unknown[]}).__calls.push({action:args.action,payload:args.payload});
      switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'1b',transcription_profile:'standard'};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  const choice=page.getByLabel('Transcription model',{exact:true});
  await expect(choice).toHaveValue('standard');
  await choice.selectOption('accurate');
  await expect(page.getByText('Until then, Standard is used.',{exact:false})).toBeVisible();
  await page.getByRole('button',{name:'Save settings',exact:true}).click();
  const calls=()=>page.evaluate(()=>(window as unknown as {__calls:{action:string;payload:Record<string,unknown>}[]}).__calls);
  await expect.poll(async()=>(await calls()).find(c=>c.action==='save_settings')?.payload.transcription_profile).toBe('accurate');
  const accurate=page.locator('article.model',{hasText:'Transcription · more accurate'});
  await expect(accurate).toContainText('600 MB');
  await accurate.getByRole('button',{name:'Download',exact:true}).click();
  await expect.poll(async()=>(await calls()).find(c=>c.action==='setup')?.payload).toMatchObject({mode:'download',model:'parakeet'});
});

test('each part plays from the host\'s copy, from memory when the webview cannot stream it',async({page})=>{
  // WebKitGTK refuses media from asset: (MEDIA_ERR_SRC_NOT_SUPPORTED); an unreadable response stands in for that here.
  await page.addInitScript(()=>{
    const meeting={id:'fixture',title:'Playback fixture',created_at:1,duration_ms:1000,status:'ready',notes:'',summary:''};
    const tracks=[{id:'one',meeting_id:'fixture',source:'mic',path:'/data/recordings/fixture/s0000-mic-000000.flac',offset_ms:0}];
    const wav=new Uint8Array(44+32000);const view=new DataView(wav.buffer);const text=(at:number,value:string)=>[...value].forEach((c,i)=>{wav[at+i]=c.charCodeAt(0);});
    text(0,'RIFF');view.setUint32(4,wav.length-8,true);text(8,'WAVEfmt ');view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,1,true);view.setUint32(24,16000,true);view.setUint32(28,32000,true);view.setUint16(32,2,true);view.setUint16(34,16,true);text(36,'data');view.setUint32(40,32000,true);
    const calls:unknown[]=[];
    Object.assign(window,{isTauri:true,__calls:calls,__TAURI_INTERNALS__:{transformCallback:()=>1,convertFileSrc:(path:string)=>`/fixtures/${path.split('/').pop()}`,invoke:async(command:string,args:{action?:string;payload?:{id?:string};id?:string})=>{
      if(command.startsWith('plugin:event'))return 1;
      if(command==='playback_bytes'){calls.push(['bytes',args.id]);return wav.buffer.slice(0);}
      switch(args?.action){case 'library':return [meeting];case 'jobs':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true}]};case 'settings':return {};case 'devices':return [];case 'detail':return {meeting,tracks,segments:[]};case 'playback':calls.push(['playback',args.payload?.id]);return {path:'/cache/playback/fixture/one.wav'};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.route('**/fixtures/one.wav',route=>route.fulfill({status:200,contentType:'audio/wav',body:'not audio'}));
  await page.goto('/');await page.getByRole('button',{name:'Playback fixture',exact:false}).click();
  await expect.poll(()=>page.locator('audio').getAttribute('src')).toMatch(/^blob:/);
  expect(await page.evaluate(()=>(window as unknown as {__calls:unknown[]}).__calls)).toEqual([['playback','one'],['bytes','one']]);
  await page.getByRole('button',{name:'Play recording',exact:true}).click();
  await expect(page.getByRole('button',{name:'Pause playback',exact:true})).toBeVisible();
});

for(const [stage,delays] of [['while the host prepares the part',{playback:700,bytes:0}],['while its bytes are on their way',{playback:0,bytes:900}]] as const){
  test(`Play pressed ${stage} starts the part once it is ready`,async({page})=>{
    await page.addInitScript(delays=>{
      const meeting={id:'fixture',title:'Playback fixture',created_at:1,duration_ms:1000,status:'ready',notes:'',summary:''};
      const tracks=[{id:'one',meeting_id:'fixture',source:'mic',path:'/data/recordings/fixture/s0000-mic-000000.flac',offset_ms:0}];
      const wav=new Uint8Array(44+32000);const view=new DataView(wav.buffer);const text=(at:number,value:string)=>[...value].forEach((c,i)=>{wav[at+i]=c.charCodeAt(0);});
      text(0,'RIFF');view.setUint32(4,wav.length-8,true);text(8,'WAVEfmt ');view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,1,true);view.setUint32(24,16000,true);view.setUint32(28,32000,true);view.setUint16(32,2,true);view.setUint16(34,16,true);text(36,'data');view.setUint32(40,32000,true);
      const later=(ms:number)=>new Promise(r=>setTimeout(r,ms));
      Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{transformCallback:()=>1,convertFileSrc:(path:string)=>`/fixtures/${path.split('/').pop()}`,invoke:async(command:string,args:{action?:string})=>{
        if(command.startsWith('plugin:event'))return 1;
        if(command==='playback_bytes'){await later(delays.bytes);return wav.buffer.slice(0);}
        switch(args?.action){case 'library':return [meeting];case 'jobs':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true}]};case 'settings':return {};case 'devices':return [];case 'detail':return {meeting,tracks,segments:[]};case 'playback':await later(delays.playback);return {path:'/cache/playback/fixture/one.wav'};default:return null;}
      }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
    },delays);
    // As in WebKitGTK, the asset: URL cannot be played; the bytes can.
    await page.route('**/fixtures/one.wav',route=>route.fulfill({status:200,contentType:'audio/wav',body:'not audio'}));
    await page.goto('/');await page.getByRole('button',{name:'Playback fixture',exact:false}).click();
    if(delays.bytes)await expect.poll(()=>page.locator('audio').getAttribute('src')).toContain('/fixtures/one.wav');
    await page.getByRole('button',{name:'Play recording',exact:true}).click();
    await expect(page.getByRole('button',{name:'Pause playback',exact:true})).toBeVisible();
    await expect.poll(()=>page.locator('audio').evaluate(a=>{const audio=a as HTMLAudioElement;return audio.src.startsWith('blob:')&&audio.played.length>0;})).toBe(true);
    await expect(page.getByRole('alert')).toHaveCount(0);
  });
}

// A host stand-in for the playback tests below: one 2-second part whose asset: URL cannot be played (as in WebKitGTK).
const playbackHost=async(page:Page,options:{bytesDelay:number;failFirstCopy:boolean;failFirstBytes?:boolean})=>{
  await page.addInitScript(options=>{
    const meeting={id:'fixture',title:'Playback fixture',created_at:1,duration_ms:2000,status:'ready',notes:'',summary:''};
    const tracks=[{id:'one',meeting_id:'fixture',source:'mic',path:'/data/recordings/fixture/s0000-mic-000000.flac',offset_ms:0}];
    const segments=[{id:1,start_ms:1000,end_ms:1900,text:'A row to jump to.',source:'mic'}];
    const wav=new Uint8Array(44+64000);const view=new DataView(wav.buffer);const text=(at:number,value:string)=>[...value].forEach((c,i)=>{wav[at+i]=c.charCodeAt(0);});
    text(0,'RIFF');view.setUint32(4,wav.length-8,true);text(8,'WAVEfmt ');view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,1,true);view.setUint32(24,16000,true);view.setUint32(28,32000,true);view.setUint16(32,2,true);view.setUint16(34,16,true);text(36,'data');view.setUint32(40,64000,true);
    const calls:string[]=[];let copies=0;let reads=0;
    Object.assign(window,{isTauri:true,__calls:calls,__TAURI_INTERNALS__:{transformCallback:()=>1,convertFileSrc:(path:string)=>`/fixtures/${path.split('/').pop()}`,invoke:async(command:string,args:{action?:string})=>{
      if(command.startsWith('plugin:event'))return 1;
      if(command==='playback_bytes'){calls.push('bytes');await new Promise(r=>setTimeout(r,options.bytesDelay));if(options.failFirstBytes&&reads++===0)throw 'This part of the recording could not be sent for playback.';return wav.buffer.slice(0);}
      switch(args?.action){case 'library':return [meeting];case 'jobs':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true}]};case 'settings':return {};case 'devices':return [];case 'detail':return {meeting,tracks,segments};
        case 'playback':calls.push('playback');if(options.failFirstCopy&&copies++===0)throw 'This part of the recording could not be read for playback.';return {path:'/cache/playback/fixture/one.wav'};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  },options);
  await page.route('**/fixtures/one.wav',route=>route.fulfill({status:200,contentType:'audio/wav',body:'not audio'}));
  await page.goto('/');await page.getByRole('button',{name:'Playback fixture',exact:false}).click();
};
const startedFromMemory=(page:Page)=>page.locator('audio').evaluate(a=>{const audio=a as HTMLAudioElement;return audio.src.startsWith('blob:')&&audio.played.length>0&&!audio.paused;});

test('a transcript jump while the part is loading shows Pause and starts it',async({page})=>{
  await playbackHost(page,{bytesDelay:2500,failFirstCopy:false});
  await page.getByRole('button',{name:'Transcript',exact:true}).click();
  await page.locator('.timestamp').first().click();
  // The jump has asked the part to start: the button says so at once, while the bytes are still on the way.
  await expect(page.getByRole('button',{name:'Pause playback',exact:true})).toBeVisible({timeout:500});
  expect(await page.locator('audio').evaluate(a=>(a as HTMLAudioElement).src.startsWith('blob:'))).toBe(false);
  await expect.poll(()=>startedFromMemory(page),{timeout:8000}).toBe(true);
  expect(await page.locator('audio').evaluate(a=>(a as HTMLAudioElement).currentTime)).toBeGreaterThanOrEqual(1);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('after a part could not be read, Play asks the host again',async({page})=>{
  await playbackHost(page,{bytesDelay:0,failFirstCopy:true});
  await expect(page.getByRole('alert')).toContainText('could not be read for playback');
  await expect(page.getByRole('button',{name:'Play recording',exact:true})).toBeVisible();
  await page.getByRole('button',{name:'Play recording',exact:true}).click();
  await expect.poll(()=>startedFromMemory(page),{timeout:8000}).toBe(true);
  expect(await page.evaluate(()=>(window as unknown as {__calls:string[]}).__calls.filter(c=>c==='playback').length)).toBe(2);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

const callCount=(page:Page,call:string)=>page.evaluate(call=>(window as unknown as {__calls:string[]}).__calls.filter(c=>c===call).length,call);
for(const [failed,options,call] of [['copy',{bytesDelay:0,failFirstCopy:true},'playback'],['bytes',{bytesDelay:0,failFirstCopy:false,failFirstBytes:true},'bytes']] as const){
  test(`after a part's ${failed} failed, a transcript jump on that part asks for it again and starts there`,async({page})=>{
    await playbackHost(page,options);
    await expect(page.getByRole('alert')).toContainText('could not be');
    await expect(page.getByRole('button',{name:'Play recording',exact:true})).toBeVisible();
    await page.getByRole('button',{name:'Transcript',exact:true}).click();
    await page.locator('.timestamp').first().click();
    await expect.poll(()=>callCount(page,call),{timeout:4000}).toBe(2);
    await expect.poll(()=>startedFromMemory(page),{timeout:8000}).toBe(true);
    expect(await page.locator('audio').evaluate(a=>(a as HTMLAudioElement).currentTime)).toBeGreaterThanOrEqual(1);
    await expect(page.getByRole('button',{name:'Pause playback',exact:true})).toBeVisible();
    await expect(page.getByRole('alert')).toHaveCount(0);
  });
}

test('Pause pressed while a failed part is asked for again leaves it paused',async({page})=>{
  await playbackHost(page,{bytesDelay:1500,failFirstCopy:true});
  await expect(page.getByRole('alert')).toContainText('could not be read for playback');
  await page.getByRole('button',{name:'Transcript',exact:true}).click();
  await page.locator('.timestamp').first().click();
  await page.getByRole('button',{name:'Pause playback',exact:true}).click();
  await expect(page.getByRole('button',{name:'Play recording',exact:true})).toBeVisible({timeout:500});
  await expect.poll(()=>page.locator('audio').evaluate(a=>(a as HTMLAudioElement).src.startsWith('blob:')),{timeout:4000}).toBe(true);
  await page.waitForTimeout(1500);
  // Still at the row, never started: the 2-second part would have ended by now if it had played.
  expect(await page.locator('audio').evaluate(a=>{const audio=a as HTMLAudioElement;return {played:audio.played.length,at:audio.currentTime};})).toEqual({played:0,at:1});
  await expect(page.getByRole('button',{name:'Play recording',exact:true})).toBeVisible();
  expect(await callCount(page,'playback')).toBe(2);
});

test('progress stays visible at the top while scrolling a long transcript',async({page})=>{
  await page.addInitScript(()=>{
    const meeting={id:'long',title:'Long conversation',created_at:1,duration_ms:3600000,status:'ready',notes:'',summary:''};
    const segments=Array.from({length:120},(_,i)=>({id:i,meeting_id:'long',start_ms:i*30000,end_ms:i*30000+5000,source:'mic',text:`Sentence number ${i} of a long conversation.`}));
    Object.assign(window,{isTauri:true,__calls:[],__jobs:[{id:'sum',meeting_id:'long',kind:'summarize',status:'running',progress:0.4,error:null},{id:'idx',meeting_id:'long',kind:'index',status:'pending',progress:0,error:null}],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      const w=window as unknown as {__calls:unknown[];__jobs:unknown[]};
      switch(args?.action){case 'library':return [meeting];case 'jobs':return w.__jobs;case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true},{id:'summary_e2b',present:true}]};case 'settings':return {summary_profile:'1b'};case 'detail':return {meeting,tracks:[],segments};
        case 'cancel':case 'enqueue':w.__calls.push({action:args.action,payload:args.payload});return null;default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.setViewportSize({width:900,height:720});
  await page.goto('/');await page.getByRole('button',{name:'Long conversation',exact:false}).click();
  await page.getByRole('button',{name:'Transcript',exact:true}).click();
  const chip=page.locator('.activity-chip');
  await expect(chip).toContainText('Creating summary · 40%');await expect(chip).toContainText('+1 more');
  await page.evaluate(()=>window.scrollTo(0,document.body.scrollHeight));
  await expect(page.getByText('Sentence number 119 of a long conversation.')).toBeInViewport();
  await expect(chip).toBeInViewport({ratio:1});
  await chip.click();
  const panel=page.getByRole('region',{name:'Local processing'});
  await expect(panel).toBeInViewport();await expect(panel).toContainText('Long conversation · Waiting');
  await page.screenshot({path:'test-results/activity.png'});
  const calls=()=>page.evaluate(()=>(window as unknown as {__calls:unknown[]}).__calls);
  await panel.locator('.job-row',{hasText:'Preparing search'}).getByRole('button',{name:'Cancel',exact:true}).click();
  await expect.poll(calls).toEqual([{action:'cancel',payload:{id:'idx'}}]);
  await page.keyboard.press('Escape');await expect(panel).toBeHidden();
  await page.evaluate(()=>{(window as unknown as {__jobs:unknown[]}).__jobs=[{id:'sum',meeting_id:'long',kind:'summarize',status:'completed',progress:1,error:null}];});
  await expect(chip).toContainText('All done');
  await expect(chip).toBeHidden({timeout:8000});
  await page.evaluate(()=>{(window as unknown as {__jobs:unknown[]}).__jobs=[{id:'bad',meeting_id:'long',kind:'transcribe',status:'failed',progress:0,error:'Local model not installed.'}];});
  await expect(chip).toContainText('Transcribing failed');await expect(chip).toHaveClass(/is-failed/);
  await chip.click();await expect(panel).toContainText('Long conversation: Local model not installed.');
  await panel.getByRole('button',{name:'Retry',exact:true}).click();
  await expect.poll(calls).toEqual([{action:'cancel',payload:{id:'idx'}},{action:'enqueue',payload:{id:'long',kind:'transcribe'}},{action:'cancel',payload:{id:'bad'}}]);
});

test('nothing is transcribed while recording; the transcript is written after Stop',async({page})=>{
  await page.addInitScript(()=>{
    const meetings=[{id:'rec',title:'Team call',created_at:2,duration_ms:0,status:'recording',notes:'',summary:''},{id:'old',title:'Older meeting',created_at:1,duration_ms:60000,status:'ready',notes:'',summary:''}];
    Object.assign(window,{isTauri:true,__capture:{meeting_id:'rec',elapsed_ms:60000,paused:false,failed:false,levels:[0,0]},
      __jobs:[{id:'t',meeting_id:'old',kind:'transcribe',status:'pending',progress:0,error:null},{id:'sum',meeting_id:'old',kind:'summarize',status:'pending',progress:0,error:null}],
      __TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string})=>{
      if(command.startsWith('plugin:event'))return 1;
      const w=window as unknown as {__capture:unknown;__jobs:unknown[]};
      switch(args?.action){case 'library':return meetings;case 'jobs':return w.__jobs;case 'devices':return [];case 'status':return w.__capture;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true}]};case 'settings':return {};
        case 'detail':return {meeting:meetings[0],tracks:[],segments:[]};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');
  // Queued work waits for the recording: no spinner, no progress, no claim that anything runs.
  const chip=page.locator('.activity-chip');
  await expect(chip).toHaveText('Waits until you stop recording+1 more');
  await expect(chip.locator('.spin')).toHaveCount(0);await expect(chip.locator('progress')).toHaveCount(0);
  await chip.click();const panel=page.getByRole('region',{name:'Local processing'});
  await expect(panel).toContainText('Nothing runs while you record, so your computer stays quiet. Transcription starts when you stop.');
  await expect(panel).toContainText('Older meeting · After you stop recording');
  await page.keyboard.press('Escape');
  await page.getByRole('navigation').getByRole('button',{name:'New recording',exact:true}).click();
  await expect(page.getByText('Recording. The transcript is written when you stop.')).toBeVisible();
  await expect(page.locator('.transcript-panel .transcript-list')).toHaveCount(0);
  await page.screenshot({path:'test-results/recording-waits.png'});
  // A new object, as every status reply is: the same one changed in place would not re-render.
  await page.evaluate(()=>{const w=window as unknown as {__capture:object};w.__capture={...w.__capture,paused:true};});
  await expect(page.getByText('Paused. The transcript is written when you stop.')).toBeVisible();
  // After Stop the recording's own pass runs, and its page shows how far it is.
  await page.evaluate(()=>{const w=window as unknown as {__capture:unknown;__jobs:unknown[]};w.__capture=null;w.__jobs=[{id:'r',meeting_id:'rec',kind:'transcribe',status:'running',progress:0.4,error:null}];});
  await expect(chip).toHaveText('Transcribing · 40%');
  await page.getByRole('button',{name:/All recordings/}).click();
  await page.getByText('Team call').first().click();
  // It opens on its notes, which say the transcript is on its way and hold Summarise until it is done.
  await expect(page.getByText('Writing the transcript… 40%. Summarise works once it is done.')).toBeVisible();
  await expect(page.getByRole('button',{name:'Summarise',exact:true})).toBeDisabled();
  await page.getByRole('button',{name:'Transcript',exact:true}).click();
  await expect(page.getByText('Writing the transcript… 40%. Rows appear as each part is finished.')).toBeVisible();
  await expect(page.getByLabel('Transcription progress')).toHaveAttribute('value','0.4');
  await expect(page.getByRole('button',{name:'Transcribe this recording'})).toHaveCount(0);
});

test('a blocked microphone points to its Windows setting, and device changes show while recording',async({page})=>{
  await page.addInitScript(()=>{
    const blocked='Windows is blocking VoxLoom from using the microphone. Turn on Settings → Privacy & security → Microphone → Let desktop apps access your microphone, then start again.';
    Object.assign(window,{isTauri:true,__calls:[],__capture:null,__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string})=>{
      if(command.startsWith('plugin:event'))return 1;
      const w=window as unknown as {__calls:string[];__capture:unknown};w.__calls.push(args?.action||command);
      switch(args?.action){case 'library':case 'jobs':return [];case 'devices':return [{name:'mic-1',label:'Headset Microphone',monitor:false,default:true},{name:'out-1',label:'Speakers',monitor:true,default:true}];case 'status':return w.__capture;case 'record':throw blocked;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true}]};case 'settings':return {};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');await page.getByRole('navigation').getByRole('button',{name:'New recording'}).click();
  await page.getByRole('button',{name:'Start recording'}).click();
  await expect(page.getByRole('alert')).toContainText('Let desktop apps access your microphone');
  await page.getByRole('button',{name:'Open microphone settings'}).click();
  expect(await page.evaluate(()=>(window as unknown as {__calls:string[]}).__calls)).toContain('open_microphone_settings');
  await expect(page.getByRole('alert')).toBeVisible();
  // While recording, the backend reports a disconnected device or the one it moved to.
  await page.evaluate(()=>{(window as unknown as {__capture:unknown}).__capture={meeting_id:'rec',elapsed_ms:5000,paused:false,failed:false,levels:[-30,-90],warning:'The microphone was disconnected. Recording continues; this part stays silent until a microphone is available.'};});
  await expect(page.getByRole('status').filter({hasText:'The microphone was disconnected'})).toBeVisible();
  await page.evaluate(()=>{(window as unknown as {__capture:{warning?:string}}).__capture.warning=undefined;});
  await expect(page.getByText('The microphone was disconnected')).toHaveCount(0);
});

test('downloaded models can be removed to free space, keeping recordings',async({page})=>{
  // Mocked bridge: real file removal and its safety checks are covered by the
  // Rust removing_models test.
  await page.addInitScript(()=>{
    let models=[{id:'whisper',name:'Whisper',bytes:147964211,disk_bytes:147964211,present:true},{id:'embedding',name:'Embedding',bytes:484622336,disk_bytes:484622336,present:true},{id:'summary_e2b',name:'E2B',bytes:2841481184,disk_bytes:0,present:false,optional:true},{id:'summary_e4b',name:'E4B',bytes:4590807392,disk_bytes:1000000000,present:false,optional:true,manual_only:true},{id:'summary_1b',name:'Gemma 3 · 1B · QAT Q4_0',filename:'gemma-3-1b-it-qat-Q4_0.gguf',bytes:720425600,disk_bytes:720425600,present:false,retired:true}];
    Object.assign(window,{isTauri:true,__removed:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:{ids:string[]}})=>{
      if(command.startsWith('plugin:event'))return 1;
      switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'e4b'};
        case 'remove_models':{const ids=args.payload!.ids;(window as unknown as {__removed:string[][]}).__removed.push(ids);const freed=models.filter(m=>ids.includes(m.id)).reduce((sum,m)=>sum+m.disk_bytes,0);models=models.filter(m=>!(m.retired&&ids.includes(m.id))).map(m=>ids.includes(m.id)?{...m,disk_bytes:0,present:false}:m);return {freed_bytes:freed};}
        default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  await expect(page.getByLabel('Summary model',{exact:true})).toHaveValue('e4b');
  await expect(page.getByText('Download “Summaries · highest quality” in Local intelligence below first.',{exact:false})).toBeVisible();
  const e4b=page.locator('article.model',{hasText:'Summaries · highest quality'});
  await expect(e4b).toContainText('4.59 GB');await expect(e4b).toContainText('Incomplete download');
  await expect(page.locator('article.model',{hasText:/^Summaries\d/}).getByRole('button',{name:'Remove',exact:false})).toHaveCount(0);
  await expect(page.getByText('Downloaded models use 2.35 GB.',{exact:false})).toBeVisible();
  const removed=()=>page.evaluate(()=>(window as unknown as {__removed:string[][]}).__removed);
  // A model earlier versions installed: it can only be removed, never downloaded.
  const old=page.locator('article.model',{hasText:'Gemma 3 · 1B'});
  await expect(old).toContainText('No longer used by VoxLoom');await expect(old.getByRole('button',{name:'Download',exact:true})).toHaveCount(0);
  await old.getByRole('button',{name:'Remove Gemma 3 · 1B · QAT Q4_0',exact:true}).click();
  await expect(page.getByRole('dialog')).toContainText('VoxLoom no longer uses this model.');
  await page.getByRole('dialog').getByRole('button',{name:'Remove model',exact:true}).click();
  await expect.poll(removed).toEqual([['summary_1b']]);await expect(old).toHaveCount(0);
  await expect(page.getByText('Downloaded models use 1.63 GB.',{exact:false})).toBeVisible();
  await e4b.getByRole('button',{name:'Remove Summaries · highest quality',exact:true}).click();
  const dialog=page.getByRole('dialog');
  await expect(dialog.getByRole('heading')).toHaveText('Remove “Summaries · highest quality”?');
  await expect(dialog).toContainText('This frees 1.00 GB on this device. Your recordings, transcripts and summaries stay.');
  await dialog.getByRole('button',{name:'Keep',exact:true}).click();
  await expect(dialog).toBeHidden();expect(await removed()).toEqual([['summary_1b']]);
  await e4b.getByRole('button',{name:'Remove Summaries · highest quality',exact:true}).click();
  await dialog.getByRole('button',{name:'Remove model',exact:true}).click();
  await expect.poll(removed).toEqual([['summary_1b'],['summary_e4b']]);
  await expect(page.getByRole('status')).toContainText('1.00 GB freed');
  await expect(e4b).toContainText('Not downloaded');
  await page.getByRole('button',{name:'Remove all models',exact:true}).click();
  await expect(dialog.getByRole('heading')).toHaveText('Remove all models?');
  await expect(dialog).toContainText('Transcription, search and summaries stop until you download the models again');
  await page.setViewportSize({width:900,height:720});await page.screenshot({path:'test-results/remove-models.png',fullPage:true});
  await dialog.getByRole('button',{name:'Remove models',exact:true}).click();
  await expect.poll(removed).toEqual([['summary_1b'],['summary_e4b'],['whisper','embedding']]);
  await expect(page.getByText('No models are stored on this device.',{exact:true})).toBeVisible();
  await expect(page.getByRole('button',{name:'Remove all models',exact:true})).toBeDisabled();
});

test('appearance follows the desktop until chosen, then persists',async({page})=>{
  await page.addInitScript(()=>{
    Object.assign(window,{isTauri:true,__saved:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      // The library keeps the saved choice across a reload, like the real settings table.
      if(args?.action==='save_settings'){(window as unknown as {__saved:unknown[]}).__saved.push(args.payload);sessionStorage.setItem('saved-appearance',(args.payload as {appearance:string}).appearance);}
      switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true}]};case 'settings':return {appearance:sessionStorage.getItem('saved-appearance')||'system'};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.emulateMedia({colorScheme:'dark'});
  await page.goto('/');
  const theme=()=>page.evaluate(()=>document.documentElement.dataset.theme??'system');
  const background=()=>page.evaluate(()=>getComputedStyle(document.documentElement).backgroundColor);
  expect(await theme()).toBe('system');expect(await background()).toBe('rgb(19, 20, 18)');
  // The sidebar button cycles System → Light → Dark → System.
  const cycle=page.getByRole('button',{name:/^Appearance: System/});
  await cycle.click();expect(await theme()).toBe('light');expect(await background()).toBe('rgb(244, 242, 236)');
  await page.getByRole('button',{name:/^Appearance: Light/}).click();expect(await theme()).toBe('dark');
  const saved=()=>page.evaluate(()=>(window as unknown as {__saved:unknown[]}).__saved);
  await expect.poll(saved).toEqual([{appearance:'light'},{appearance:'dark'}]);
  // Saved locally too, so the next launch paints the right theme before the app loads.
  await page.reload();expect(await theme()).toBe('dark');
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  const group=page.getByRole('group',{name:'Appearance'});
  await expect(group.getByRole('button',{name:'Dark'})).toHaveAttribute('aria-pressed','true');
  await group.getByRole('button',{name:'System'}).click();expect(await theme()).toBe('system');
  await expect(page.getByRole('button',{name:/^Appearance: System/})).toBeVisible();
});

test('honest empty library, navigation, local assets and responsive layout',async({page})=>{
  const outbound:string[]=[];page.on('request',r=>{if(!r.url().startsWith('http://127.0.0.1:1420')&&!r.url().startsWith('data:'))outbound.push(r.url());});
  await page.goto('/');await expect(page.getByRole('heading',{name:'Good conversations. Nothing forgotten.'})).toBeVisible();
  await expect(page.getByText('A space for your next conversation.')).toBeVisible();await expect(page.getByText('Browser preview')).toBeVisible();
  await page.getByRole('button',{name:'New recording',exact:true}).first().click();await expect(page.getByRole('heading',{name:'Recording studio.'})).toBeVisible();
  await expect(page.getByRole('button',{name:'Start recording',exact:true})).toBeDisabled();await expect(page.getByText('Summaries are always on demand.',{exact:false})).toBeVisible();
  await page.getByRole('button',{name:'Settings',exact:true}).click();await page.getByLabel('Summary model',{exact:true}).selectOption('e4b');await expect(page.getByText('E4B is the largest option',{exact:false})).toBeVisible();
  await page.getByRole('button',{name:'Model setup',exact:true}).click();await expect(page.getByRole('dialog')).toBeVisible();await expect(page.getByRole('button',{name:'Download & set up',exact:true})).toBeVisible();await expect(page.getByRole('button',{name:'Install search runtime',exact:true})).toBeHidden();await expect(page.getByLabel('Add optional summaries',{exact:false})).toBeChecked();
  await page.getByRole('button',{name:'Close model setup'}).click();await page.getByRole('button',{name:'Search',exact:true}).first().click();await expect(page.getByRole('heading',{name:'Find the moment.'})).toBeVisible();
  await page.setViewportSize({width:900,height:720});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();expect(outbound).toEqual([]);
  await page.screenshot({path:'test-results/search.png',fullPage:true});
});

// The floating recorder: a bubble that opens a bar of controls on click, moves with a drag, and closes by itself.
const floatingHost=async(page:Page,capture:Record<string,unknown>|null)=>{
  await page.addInitScript(capture=>{
    const calls:unknown[]=[];
    Object.assign(window,{isTauri:true,__calls:calls,__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      if(args?.action==='float_size'||args?.action==='float_move')calls.push([args.action,args.payload]);
      switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return capture;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[]};case 'settings':return {};case 'float_size':return {left:false};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  },capture);
};
const floatCalls=(page:Page)=>page.evaluate(()=>(window as unknown as {__calls:[string,Record<string,unknown>][]}).__calls);

test('the floating bubble opens and closes its controls, and shows the time while recording',async({page})=>{
  await floatingHost(page,{meeting_id:'m',elapsed_ms:754000,paused:false,failed:false,levels:[]});
  await page.setViewportSize({width:64,height:64});await page.goto('/?mini=1');
  const bubble=page.getByRole('button',{name:'Recording, 12:34. Show recorder controls'});
  await expect(bubble).toHaveText('12:34');await expect(bubble).toHaveClass(/is-live/);await expect(bubble).toHaveAttribute('title','Recording · 00:12:34');
  await expect(page.getByRole('button',{name:'Stop recording'})).toHaveCount(0);
  await bubble.click();
  await page.setViewportSize({width:200,height:64});
  await expect(page.getByRole('button',{name:'Stop recording'})).toBeVisible();await expect(page.getByRole('button',{name:'Pause'})).toBeVisible();
  expect(await page.locator('.mini').evaluate(e=>e.scrollWidth<=e.clientWidth)).toBe(true);
  await page.getByRole('button',{name:'Recording, 12:34. Hide recorder controls'}).click();
  await expect(page.getByRole('button',{name:'Stop recording'})).toHaveCount(0);
  expect(await floatCalls(page)).toEqual([['float_size',{bar:true}],['float_size',{bar:false}]]);
});

test('dragging the floating bubble moves the window and opens nothing',async({page})=>{
  await floatingHost(page,null);
  await page.setViewportSize({width:64,height:64});await page.goto('/?mini=1');
  await page.getByRole('button',{name:'VoxLoom recorder. Show recorder controls'}).waitFor();
  // Where the page sits on the screen, as pointer events report it (headless Chrome adds its hidden toolbar).
  await page.evaluate(()=>window.addEventListener('pointerdown',e=>Object.assign(window,{__offset:{x:e.screenX-e.clientX,y:e.screenY-e.clientY}}),{once:true}));
  await page.mouse.move(32,32);await page.mouse.down();
  // Under 5 px is still a click; past it the window follows the pointer.
  await page.mouse.move(35,33);await page.mouse.move(52,40,{steps:4});await page.mouse.up();
  await expect.poll(async()=>(await floatCalls(page)).filter(c=>c[0]==='float_move').length).toBeGreaterThan(0);
  const calls=await floatCalls(page);
  expect(calls.some(c=>c[0]==='float_size')).toBe(false);
  // Screen position = pointer on screen minus where it pressed inside the window.
  const last=calls.filter(c=>c[0]==='float_move').at(-1)![1];
  const offset=await page.evaluate(()=>(window as unknown as {__offset:{x:number;y:number}}).__offset);
  expect(last).toEqual({x:offset.x+52-32,y:offset.y+40-32});
  await expect(page.getByRole('button',{name:'Expand recorder'})).toHaveCount(0);
  // The next plain click still opens the controls.
  await page.getByRole('button',{name:'VoxLoom recorder. Show recorder controls'}).click();
  await expect(page.getByRole('button',{name:'Expand recorder'})).toBeVisible();
});

test('the floating controls close by themselves 4 s after the pointer leaves',async({page})=>{
  await page.clock.install();
  await floatingHost(page,null);
  await page.setViewportSize({width:200,height:64});await page.goto('/?mini=1');
  await page.getByRole('button',{name:'VoxLoom recorder. Show recorder controls'}).click();
  await expect(page.getByRole('button',{name:'Expand recorder'})).toBeVisible();
  await page.locator('.mini').dispatchEvent('pointerout',{relatedTarget:null});
  await page.clock.runFor(3000);
  await expect(page.getByRole('button',{name:'Expand recorder'})).toBeVisible();
  await page.clock.runFor(1500);
  await expect(page.getByRole('button',{name:'Expand recorder'})).toHaveCount(0);
  expect((await floatCalls(page)).at(-1)).toEqual(['float_size',{bar:false}]);
});

test('home visual and compact recorder have real actions and labels',async({page})=>{
  await page.goto('/');await page.screenshot({path:'test-results/home.png',fullPage:true});
  // Outside the desktop app, opening the bubble's controls already reports that it needs the app.
  await page.goto('/?mini=1');await page.getByRole('button',{name:'VoxLoom recorder. Show recorder controls'}).click();await expect(page.getByRole('alert')).toContainText('desktop application');
  await expect(page.getByRole('button',{name:'Expand recorder'})).toBeAttached();await expect(page.getByRole('button',{name:'Quit VoxLoom and release models'})).toBeAttached();
  await page.screenshot({path:'test-results/mini.png'});
});

test('continuous playback advances across stored audio chunks and seeks globally',async({page})=>{
  // Explicit bridge fixture for UI control flow only; real inference/capture is
  // exercised by the separate Rust/engine acceptance tests.
  await page.addInitScript(()=>{
    const meeting={id:'fixture',title:'Playback fixture',created_at:1,duration_ms:2000,status:'ready',notes:'',summary:'<|channel>thought\nThinking Process: private draft from an old build'};
    const tracks=[{id:'one',meeting_id:'fixture',source:'imported',path:'first.wav',offset_ms:0},{id:'two',meeting_id:'fixture',source:'imported',path:'second.wav',offset_ms:1000}];
    Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{transformCallback:()=>1,convertFileSrc:(path:string)=>`/fixtures/${path}`,invoke:async(command:string,args:{action?:string})=>{
      if(command.startsWith('plugin:event'))return 1;
      switch(args?.action){case 'library':return [meeting];case 'jobs':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true}]};case 'settings':return {};case 'devices':return [];case 'detail':return {meeting,tracks,segments:[]};default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  const wav=Buffer.alloc(44+32000);wav.write('RIFF',0);wav.writeUInt32LE(wav.length-8,4);wav.write('WAVEfmt ',8);wav.writeUInt32LE(16,16);wav.writeUInt16LE(1,20);wav.writeUInt16LE(1,22);wav.writeUInt32LE(16000,24);wav.writeUInt32LE(32000,28);wav.writeUInt16LE(2,32);wav.writeUInt16LE(16,34);wav.write('data',36);wav.writeUInt32LE(32000,40);
  await page.route('**/fixtures/*.wav',route=>route.fulfill({status:200,contentType:'audio/wav',body:wav}));
  await page.goto('/');await page.getByRole('button',{name:'Playback fixture',exact:false}).click();
  await expect(page.getByLabel('Audio source',{exact:true})).toHaveValue('imported');await expect(page.getByLabel('Recording timeline')).toHaveAttribute('max','2000');
  // Every label the button shows from Play to the end: no flicker where one part hands over to the next.
  await page.locator('.play-button').evaluate(button=>{const labels:string[]=[];(window as unknown as {__labels:string[]}).__labels=labels;new MutationObserver(()=>labels.push(button.getAttribute('aria-label')||'')).observe(button,{attributes:true,attributeFilter:['aria-label']});});
  await page.getByRole('button',{name:'Play recording',exact:true}).click();
  await expect.poll(()=>page.locator('audio').getAttribute('src')).toContain('second.wav');
  // The next part really plays, and the button shows when it stops.
  await expect.poll(()=>page.locator('audio').evaluate(a=>{const audio=a as HTMLAudioElement;return audio.src.includes('second.wav')&&audio.played.length>0;})).toBe(true);
  await expect(page.getByRole('button',{name:'Play recording',exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as unknown as {__labels:string[]}).__labels)).toEqual(['Pause playback','Play recording']);
  await page.getByLabel('Recording timeline').fill('300');await expect.poll(()=>page.locator('audio').getAttribute('src')).toContain('first.wav');
  await page.getByRole('button',{name:'Notes',exact:true}).click();
  await expect(page.getByLabel('Notes',{exact:true})).toHaveValue('');
  await expect(page.getByText('Thinking Process: private draft from an old build',{exact:false})).toHaveCount(0);
  await expect(page.getByRole('button',{name:'Export',exact:true})).toHaveCount(0);
});
