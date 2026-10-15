import json
from pathlib import Path
import re
import sys
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET
from zipfile import ZipFile

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'worker'))
import engine
from export_docx import export_docx
from notes import clean_meeting_notes, normalise_meeting_notes


class MeetingNotesTests(unittest.TestCase):
    def test_plain_finished_notes_require_native_completion_not_partial_text(self):
        self.assertEqual(engine.final_response('Overview\nApproved workshop. [end of text]',allow_plain=True),'Overview\nApproved workshop.')
        for reply in ['Overview\ncut off','<|channel>thought unfinished [end of text]']:
            with self.assertRaises(ValueError):engine.final_response(reply,allow_plain=True)

    def test_normalise_outputs_plain_notes_and_omits_empty_or_unraised_questions(self):
        result=normalise_meeting_notes('**Overview**\nA workshop.\n\n**Action items**\n- **Sarah** drafts the guide.\n\nOpen questions\nA made up question?\n\nDecisions\nNone.',include_questions=False)
        self.assertEqual(result,'Overview\nA workshop.\n\nAction items\n• Sarah drafts the guide.')
    def test_meeting_notes_do_not_require_or_show_source_timecodes(self):
        with tempfile.TemporaryDirectory() as directory:
            scratch=Path(directory)
            reply='<voxloom_result>Overview\nThe workshop was approved.\n\nAction items\n• Sarah sends the guide on Friday at 09:30.</voxloom_result>'
            with patch.object(engine,'ensure_model',return_value='fixture'),patch.object(engine,'run',return_value=reply.encode()):
                result=engine.generate({'models':{'summary':'fixture'},'llama_cli':'fixture','threads':2},'<EVIDENCE>The workshop was approved. Sarah sends the guide on Friday at 09:30.</EVIDENCE>',scratch,meeting_notes=True)
            self.assertIn('Overview',result)
            self.assertIn('Friday at 09:30',result)
            self.assertNotIn('evidence extracts',result)
            prompt=(scratch/'prompt.txt').read_text()
            self.assertIn('organised by topic',prompt)
            self.assertNotIn('Begin each bullet with a timestamp',prompt)

    def test_long_notes_reduce_by_topic_without_inserting_source_times(self):
        calls,rows=[],[]
        def generate(request,content,scratch,meeting_notes=False):
            calls.append((content,meeting_notes))
            return 'Overview\nThe workshop covers privacy.\n\nAction items\n• Sarah drafts the guide by Friday.'
        segments=[{'start_ms':i*30000,'text':('The workshop covers privacy. '*100)} for i in range(3)]
        with patch.object(engine,'generate',side_effect=generate):
            engine.summarize({'segments':segments},lambda **row:rows.append(row),Path('/tmp'))
        self.assertGreater(len(calls),3)
        self.assertTrue(all(mode and not re.search(r'\[\d{2}:\d{2}:\d{2}\]',content) for content,mode in calls))
        self.assertIn('Action items',rows[-1]['text'])

    def test_cleanup_keeps_actual_deadline(self):
        self.assertEqual(clean_meeting_notes('Overview\n[00:00:03] Plan agreed.\n- [00:00:15] Call Friday at 09:30.'),'Overview\n Plan agreed.\n- Call Friday at 09:30.')

    def test_word_document_contains_real_headings_bullets_and_safe_text(self):
        with tempfile.TemporaryDirectory(prefix='voxloom word café ') as directory:
            target=Path(directory)/'Meeting notes.docx'
            export_docx(target,'Planning & review <team>','## Overview\nA privacy workshop. [00:00:03]\n\n**Action items**\n- **Sarah** sends the guide on Friday at 09:30.')
            with ZipFile(target) as archive:
                self.assertIsNone(archive.testzip())
                for name in archive.namelist(): ET.fromstring(archive.read(name))
                document=ET.fromstring(archive.read('word/document.xml'))
                namespace={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
                text=''.join(document.itertext())
                self.assertIn('Planning & review <team>',text)
                self.assertIn('Sarah sends the guide',text)
                self.assertNotIn('[00:00:03]',text)
                self.assertNotIn('**',text)
                self.assertNotIn('##',text)
                self.assertTrue(document.findall('.//w:pStyle[@w:val="Heading1"]',namespace))
                self.assertTrue(document.findall('.//w:numPr',namespace))
                self.assertTrue(document.findall('.//w:b',namespace))
                self.assertNotIn(b'TargetMode="External"',archive.read('word/_rels/document.xml.rels'))
            self.assertEqual(target.stat().st_mode&0o777,0o600)

    def test_word_transcript_retains_its_recording_times(self):
        with tempfile.TemporaryDirectory() as directory:
            target=Path(directory)/'Transcript.docx'
            export_docx(target,'Transcript','[00:00:03] system: A complete sentence.',summary=False)
            with ZipFile(target) as archive:
                self.assertIn('[00:00:03]',''.join(ET.fromstring(archive.read('word/document.xml')).itertext()))
