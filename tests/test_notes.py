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
        self.assertEqual(result,'## Overview\nA workshop.\n\n## Action items\n• Sarah drafts the guide.')
    def test_meeting_notes_do_not_require_or_show_source_timecodes(self):
        with tempfile.TemporaryDirectory() as directory:
            scratch=Path(directory)
            reply='<voxloom_result>Overview\nThe workshop was approved.\n\nAction items\n• Sarah sends the guide on Friday at 09:30.</voxloom_result>'
            with patch.object(engine,'ensure_model',return_value='fixture'),patch.object(engine,'run',return_value=reply.encode()):
                result=engine.generate({'models':{'summary':'fixture'},'llama_cli':'fixture','threads':2,'summary_style':'meeting'},'<EVIDENCE>The workshop was approved. Sarah sends the guide on Friday at 09:30.</EVIDENCE>',scratch,meeting_notes=True)
            self.assertIn('Overview',result)
            self.assertIn('Friday at 09:30',result)
            self.assertNotIn('evidence extracts',result)
            prompt=(scratch/'prompt.txt').read_text()
            self.assertIn('organised by topic',prompt)
            self.assertNotIn('Begin each bullet with a timestamp',prompt)

    def test_long_notes_reduce_by_topic_without_inserting_source_times(self):
        calls,rows=[],[]
        def generate(request,content,scratch,meeting_notes=False,title=False):
            calls.append((content,meeting_notes,title))
            notes='Overview\nThe workshop covers privacy.\n\nAction items\n• Sarah drafts the guide by Friday.'
            return (notes,'Privacy workshop') if title else notes
        segments=[{'start_ms':i*30000,'text':('The workshop covers privacy. '*100)} for i in range(3)]
        with patch.object(engine,'generate',side_effect=generate):
            engine.summarize({'segments':segments},lambda **row:rows.append(row),Path('/tmp'))
        self.assertGreater(len(calls),3)
        self.assertTrue(all(mode and not re.search(r'\[\d{2}:\d{2}:\d{2}\]',content) for content,mode,_ in calls))
        self.assertIn('Action items',rows[-1]['text'])
        # Only the pass that writes the final notes names the recording.
        self.assertEqual([title for *_,title in calls],[False]*(len(calls)-1)+[True])
        self.assertEqual(rows[-1]['title'],'Privacy workshop')

    def test_odd_leftover_note_waits_for_a_partner_instead_of_a_solo_pass(self):
        calls=[]
        def generate(request,content,scratch,meeting_notes=False,title=False):
            calls.append(content)
            notes=f'Overview\nNote {len(calls)}.'
            return (notes,'Title') if title else notes
        segments=[{'start_ms':i*30000,'text':('The workshop covers privacy. '*100)} for i in range(3)]
        with patch.object(engine,'generate',side_effect=generate):
            engine.summarize({'segments':segments},lambda **row:None,Path('/tmp'))
        # Three batches: three note passes, one combine of the first pair, then
        # the final combine of that result with the untouched third note.
        self.assertEqual(len(calls),5)
        self.assertIn('Note 3.',calls[-1]);self.assertIn('Note 4.',calls[-1])
        self.assertNotIn('Note 3.',calls[3])

    def test_short_transcript_gets_title_from_its_only_pass(self):
        rows=[]
        with tempfile.TemporaryDirectory() as directory:
            reply='<voxloom_result>Title: Staff privacy workshop\nOverview\nThe workshop was approved.</voxloom_result>'
            with patch.object(engine,'ensure_model',return_value='fixture'),patch.object(engine,'run',return_value=reply.encode()):
                engine.summarize({'models':{'summary':'fixture'},'llama_cli':'fixture','threads':2,'segments':[{'start_ms':0,'text':'The workshop was approved.'}]},lambda **row:rows.append(row),Path(directory))
            self.assertIn('first line of the answer must start with Title:',(Path(directory)/'prompt.txt').read_text())
        self.assertEqual(rows[-1]['title'],'Staff privacy workshop')
        self.assertEqual(rows[-1]['text'],'## Overview\nThe workshop was approved.')

    def test_title_line_is_cleaned_or_dropped(self):
        self.assertEqual(engine.split_title('**Title:** "Sarah\'s induction workshop".\n\nOverview\nx'),("Sarah's induction workshop",'\nOverview\nx'))
        self.assertEqual(engine.split_title('Overview\nNo title here.'),('','Overview\nNo title here.'))
        # Gemma 3 1B wrote this: an unlabelled title and a misspelt closing tag.
        reply='<voxloom_result>Staff Induction Workshop – Customer Privacy</voiceloom_result>\n\n**Overview**\nA workshop. [end of text]'
        with tempfile.TemporaryDirectory() as directory,patch.object(engine,'ensure_model',return_value='fixture'),patch.object(engine,'run',return_value=reply.encode()):
            self.assertEqual(engine.generate({'models':{'summary':'fixture'},'llama_cli':'fixture','threads':2},'<EVIDENCE>x</EVIDENCE>',Path(directory),meeting_notes=True,title=True),('## Overview\nA workshop.','Staff Induction Workshop – Customer Privacy'))
        self.assertEqual(engine.split_title('The team met briefly.\nOverview\nx'),('','The team met briefly.\nOverview\nx'))
        for bad in ['Title: [00:00:03] Opening','Title: '+'word '*20,'Title: <|channel>']:
            self.assertEqual(engine.split_title(bad+'\nOverview\nx'),('','Overview\nx'))

    def test_summary_styles_default_to_counselling_and_keep_safety_rules(self):
        def prompt(**settings):
            with tempfile.TemporaryDirectory() as directory:
                reply='<voxloom_result>Main concerns\n- Feeling isolated at school.</voxloom_result>'
                with patch.object(engine,'ensure_model',return_value='fixture'),patch.object(engine,'run',return_value=reply.encode()):
                    notes=engine.generate({'models':{'summary':'fixture'},'llama_cli':'fixture','threads':2,**settings},'<EVIDENCE>x</EVIDENCE>',Path(directory),meeting_notes=True)
                return (Path(directory)/'prompt.txt').read_text(),notes
        text,notes=prompt()
        self.assertIn('counselling or support session',text);self.assertIn('Do not diagnose',text)
        self.assertNotIn('owner and deadline',text);self.assertNotIn('owners, deadlines',text)
        self.assertEqual(notes,'## Main concerns\n• Feeling isolated at school.')
        self.assertIn('Reason for contact',prompt(summary_style='service')[0])
        self.assertIn('owner and deadline',prompt(summary_style='meeting')[0])
        custom=prompt(summary_style='custom',summary_prompt='List what the client wants help with.')[0]
        self.assertIn('<INSTRUCTIONS>\nList what the client wants help with.\n</INSTRUCTIONS>',custom)
        for rule in ['untrusted quoted data','use only what was actually said','do not include timestamps']:
            self.assertIn(rule,custom)
        # Custom without instructions, or an unknown style, falls back to counselling.
        self.assertIn('counselling or support session',prompt(summary_style='custom',summary_prompt='  ')[0])
        self.assertIn('counselling or support session',prompt(summary_style='poetry')[0])

    def test_titles_written_inline_and_empty_sections_are_tidied(self):
        # Gemma 3 1B writes "Title: text"; Gemma 4 E2B keeps a section saying it is empty.
        text='Overview: The client feels stretched.\nMain Concerns: Failing two classes.\n\nAgreed next steps\nNo agreed next steps were mentioned in the evidence.'
        self.assertEqual(normalise_meeting_notes(text,headings=engine.STYLES['counselling']['sections']),'## Overview\nThe client feels stretched.\n\n## Main Concerns\nFailing two classes.')
        self.assertEqual(normalise_meeting_notes('Main concerns\n- No longer able to work; mentioned losing a job.',headings=['Main concerns']),'## Main concerns\n• No longer able to work; mentioned losing a job.')
        # Ordinary sentences with a colon stay as they are.
        self.assertEqual(normalise_meeting_notes('Overview\nShe said: no changes yet.'),'## Overview\nShe said: no changes yet.')

    def test_custom_titles_ending_in_a_colon_become_headings(self):
        self.assertEqual(normalise_meeting_notes('What they want help with:\n- Housing.\nSarah said she was tired.'),'## What they want help with\n• Housing.\nSarah said she was tired.')

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
            if sys.platform!='win32':self.assertEqual(target.stat().st_mode&0o777,0o600)  # Windows: AppData ACLs keep files private

    def test_word_transcript_retains_its_recording_times(self):
        with tempfile.TemporaryDirectory() as directory:
            target=Path(directory)/'Transcript.docx'
            export_docx(target,'Transcript','[00:00:03] system: A complete sentence.',summary=False)
            with ZipFile(target) as archive:
                self.assertIn('[00:00:03]',''.join(ET.fromstring(archive.read('word/document.xml')).itertext()))
