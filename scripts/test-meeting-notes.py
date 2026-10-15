#!/usr/bin/env python3
"""Real local meeting-note generation; synthetic facts, no model downloads."""
import argparse
import json
from pathlib import Path
import re
import sys
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'worker'))
import engine

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--models',type=Path,required=True)
parser.add_argument('--model',choices=['1b','e2b'])
parser.add_argument('--artifacts',type=Path)
args=parser.parse_args()
texts=[
    'We are planning a staff induction workshop about customer privacy and accessible communication.',
    'The group agreed to keep the workshop to forty five minutes and include a hands on privacy exercise.',
    'Sarah will draft the facilitator guide by Friday. David will book the meeting room. No deadline was agreed for the room booking.',
    'The team still needs to confirm accessible video captions before publishing the workshop materials.',
]
for filename in ['gemma-3-1b-it-qat-Q4_0.gguf','gemma-4-E2B-it-Q4_0.gguf']:
    if args.model and (args.model=='e2b') != ('E2B' in filename):continue
    path=args.models/filename
    if not path.is_file():
        print(json.dumps({'model':filename,'result':'SKIPPED: model files not provisioned'}),flush=True)
        continue
    with tempfile.TemporaryDirectory(prefix='voxloom-meeting-notes-') as directory:
        original_run=engine.run
        if args.artifacts:
            args.artifacts.mkdir(parents=True,exist_ok=True)
            def traced(argv,**kwargs):
                result=original_run(argv,**kwargs)
                if Path(argv[0]).name=='llama-completion':(args.artifacts/(filename+'.txt')).write_bytes(result)
                return result
            engine.run=traced
        rows=[]
        started=time.monotonic()
        engine.summarize({'models':{'summary':str(path)},'llama_cli':str(ROOT/'.runtimes/llama-build/bin/llama-completion'),'threads':2,'segments':[{'start_ms':i*30000,'text':text} for i,text in enumerate(texts)]},lambda **row:rows.append(row),Path(directory))
        result=rows[-1]['text']
        assert rows[-1]['type']=='summary'
        assert not re.search(r'\[\d{2}:\d{2}:\d{2}\]|<\|channel>|Thinking Process:',result)
        for word in ['workshop','privacy','Sarah','Friday','David']:
            assert word.lower() in result.lower(),(word,result)
        print(json.dumps({'model':filename,'result':'PASS','seconds':time.monotonic()-started,'synthetic_meeting_notes':result},indent=2),flush=True)
        engine.run=original_run
