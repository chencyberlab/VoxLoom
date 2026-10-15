import { test, expect } from '@playwright/test';

test('meeting notes render normally, copy rich text and export Word',async({page,context})=>{
  await context.grantPermissions(['clipboard-read','clipboard-write']);
  await page.addInitScript(()=>{
    const meeting={id:'notes',title:'Workshop planning',created_at:1,duration_ms:60000,status:'ready',notes:'',summary:'## Overview\nThe workshop covers privacy. [00:00:03]\n\n**Action items**\n- **Sarah** sends the guide on Friday at 09:30.'};
    Object.assign(window,{isTauri:true,__exports:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
      if(command.startsWith('plugin:event'))return 1;
      if(command==='plugin:dialog|save')return '/tmp/Meeting notes.docx';
      switch(args?.action){case 'library':return [meeting];case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models:[{id:'whisper',present:true},{id:'embedding',present:true},{id:'summary',present:true}]};case 'settings':return {summary_profile:'1b'};case 'detail':return {meeting,tracks:[],segments:[{id:1,meeting_id:'notes',start_ms:3000,end_ms:6000,source:'system',text:'The workshop covers privacy.'}]};case 'export':(window as unknown as {__exports:unknown[]}).__exports.push(args.payload);return null;default:return null;}
    }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
  });
  await page.goto('/');await page.getByRole('button',{name:'Workshop planning',exact:false}).click();
  await page.getByRole('button',{name:'Summary',exact:true}).click();
  await expect(page.getByRole('heading',{name:'Meeting notes.',exact:true})).toBeVisible();
  await expect(page.getByRole('heading',{name:'Overview',exact:true})).toBeVisible();
  await expect(page.getByRole('heading',{name:'Action items',exact:true})).toBeVisible();
  await expect(page.locator('.meeting-notes li')).toHaveText('Sarah sends the guide on Friday at 09:30.');
  await expect(page.locator('.meeting-notes')).not.toContainText('[00:00:03]');
  await expect(page.locator('.meeting-notes')).not.toContainText('**');
  await page.getByRole('button',{name:'Copy notes',exact:true}).click();
  await expect(page.getByRole('status')).toContainText('Meeting notes copied');
  const copied=await page.evaluate(async()=>{
    const items=await navigator.clipboard.read();const item=items[0];
    return {plain:await(await item.getType('text/plain')).text(),html:await(await item.getType('text/html')).text()};
  });
  expect(copied.plain).toContain('• Sarah sends the guide on Friday at 09:30.');expect(copied.plain).not.toMatch(/\*\*|##|\[00:00:03\]/);
  expect(copied.html).toContain('<h3');expect(copied.html).toContain('<strong>Sarah</strong>');
  await page.getByRole('button',{name:'Word document',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__exports:unknown[]}).__exports)).toEqual([{id:'notes',path:'/tmp/Meeting notes.docx',format:'summary_docx',track_id:''}]);
  await page.setViewportSize({width:900,height:720});expect(await page.locator('.meeting-notes').evaluate(element=>element.scrollWidth<=element.clientWidth)).toBeTruthy();
  await page.screenshot({path:'test-results/meeting-notes.png',fullPage:true});
  await page.getByRole('button',{name:'Transcript',exact:true}).click();await expect(page.getByText('The workshop covers privacy.',{exact:true})).toBeVisible();
  await expect(page.getByLabel('Export transcript').locator('option')).not.toContainText(['MD','JSON']);
});

for(const choice of ['1b','e2b','none'] as const){
  test(`first-run setup downloads the selected summary model: ${choice}`,async({page})=>{
    // This bridge checks UI requests only. Real provisioning and persistence
    // are covered by the Rust offline setup test with integrity-checked files.
    await page.addInitScript(()=>{
      const models=[{id:'whisper',bytes:147964211,present:false},{id:'embedding',bytes:484622336,present:false},{id:'summary',bytes:720425600,present:false,optional:true},{id:'summary_e2b',bytes:2841481184,present:false,optional:true,manual_only:true}];
      Object.assign(window,{isTauri:true,__setupCalls:[],__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(command:string,args:{action?:string;payload?:unknown})=>{
        if(command.startsWith('plugin:event'))return 1;
        switch(args?.action){case 'library':case 'jobs':case 'devices':return [];case 'status':return null;case 'runtime_status':return {inference:true,whisper:true,llama:true,disk_free_bytes:1e10};case 'models':return {models};case 'settings':return {summary_profile:'1b'};case 'setup':(window as unknown as {__setupCalls:unknown[]}).__setupCalls.push(args.payload);return null;default:return null;}
      }},__TAURI_EVENT_PLUGIN_INTERNALS__:{unregisterListener:()=>{}}});
    });
    await page.goto('/');
    const dialog=page.getByRole('dialog');
    const model=dialog.getByLabel('Summary model',{exact:true});
    await expect(model).toHaveValue('1b');
    await expect(dialog.getByText('Recommended for a lighter setup',{exact:false})).toBeVisible();
    if(choice==='none'){
      await dialog.getByLabel('Add optional summaries',{exact:false}).uncheck();
      await expect(model).toBeHidden();
      await expect(dialog.getByText('About 0.8 GB in total',{exact:false})).toBeVisible();
    }else{
      await model.selectOption(choice);
      await expect(dialog.getByText(`About ${choice==='e2b'?'3.6':'1.5'} GB in total`,{exact:false})).toBeVisible();
      if(choice==='e2b')await expect(dialog.getByText('Uses more memory and may take longer',{exact:false})).toBeVisible();
    }
    await page.setViewportSize({width:900,height:720});
    await expect(dialog.getByRole('button',{name:'Download & set up',exact:true})).toBeInViewport({ratio:1});
    await dialog.getByRole('button',{name:'Download & set up',exact:true}).click();
    await expect.poll(()=>page.evaluate(()=>(window as unknown as {__setupCalls:unknown[]}).__setupCalls)).toEqual([{mode:'recommended',core_only:choice==='none',summary_profile:choice==='none'?'1b':choice,path:null}]);
    if(choice!=='none')await expect(model).toBeDisabled();
    expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth)).toBeTruthy();
    if(choice==='e2b')await page.screenshot({path:'test-results/setup-summary-model.png',fullPage:true});
  });
}

test('accurate transcription is an explicit download and setting',async({page})=>{
  await page.addInitScript(()=>{
    const models=[{id:'whisper',bytes:147964211,present:true},{id:'whisper_accurate',bytes:264477561,present:false,optional:true,manual_only:true},{id:'embedding',bytes:484622336,present:true},{id:'summary',bytes:720425600,present:true,optional:true}];
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
  await expect(accurate).toContainText('264 MB');
  await accurate.getByRole('button',{name:'Download',exact:true}).click();
  await expect.poll(async()=>(await calls()).find(c=>c.action==='setup')?.payload).toMatchObject({mode:'download',model:'whisper_accurate'});
});

test('honest empty library, navigation, local assets and responsive layout',async({page})=>{
  const outbound:string[]=[];page.on('request',r=>{if(!r.url().startsWith('http://127.0.0.1:1420')&&!r.url().startsWith('data:'))outbound.push(r.url());});
  await page.goto('/');await expect(page.getByRole('heading',{name:'Good conversations. Nothing forgotten.'})).toBeVisible();
  await expect(page.getByText('A space for your next conversation.')).toBeVisible();await expect(page.getByText('Browser preview')).toBeVisible();
  await page.getByRole('button',{name:'New recording',exact:true}).first().click();await expect(page.getByRole('heading',{name:'Recording studio.'})).toBeVisible();
  await expect(page.getByRole('button',{name:'Start recording',exact:true})).toBeDisabled();await expect(page.getByText('Summaries are always on demand.',{exact:false})).toBeVisible();
  await page.getByRole('button',{name:'Settings',exact:true}).click();await page.getByLabel('Summary model',{exact:true}).selectOption('e2b');await expect(page.getByText('E2B uses more memory',{exact:false})).toBeVisible();
  await page.getByRole('button',{name:'Model setup',exact:true}).click();await expect(page.getByRole('dialog')).toBeVisible();await expect(page.getByRole('button',{name:'Download & set up',exact:true})).toBeVisible();await expect(page.getByRole('button',{name:'Install search runtime',exact:true})).toBeHidden();await expect(page.getByLabel('Add optional summaries',{exact:false})).toBeChecked();
  await page.getByRole('button',{name:'Close model setup'}).click();await page.getByRole('button',{name:'Search',exact:true}).first().click();await expect(page.getByRole('heading',{name:'Find the moment.'})).toBeVisible();
  await page.setViewportSize({width:900,height:720});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();expect(outbound).toEqual([]);
  await page.screenshot({path:'test-results/search.png',fullPage:true});
});

test('home visual and compact recorder have real actions and labels',async({page})=>{
  await page.goto('/');await page.screenshot({path:'test-results/home.png',fullPage:true});
  await page.goto('/?mini=1');await expect(page.getByRole('button',{name:'Expand recorder'})).toBeVisible();await expect(page.getByRole('button',{name:'Quit VoxLoom and release models'})).toBeVisible();
  await page.getByRole('button',{name:'Expand recorder'}).click();await expect(page.getByRole('alert')).toContainText('desktop application');
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
  await page.getByRole('button',{name:'Play recording',exact:true}).click();
  await expect.poll(()=>page.locator('audio').getAttribute('src')).toContain('second.wav');
  await page.getByLabel('Recording timeline').fill('300');await expect.poll(()=>page.locator('audio').getAttribute('src')).toContain('first.wav');
  await page.getByRole('button',{name:'Summary',exact:true}).click();
  await expect(page.getByText('This saved summary is an unfinished draft.',{exact:false})).toBeVisible();
  await expect(page.getByText('Thinking Process: private draft from an old build',{exact:false})).toHaveCount(0);
  await expect(page.getByRole('button',{name:'Export',exact:true})).toHaveCount(0);
});
