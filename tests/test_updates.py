import io
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
import urllib.error

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'worker'))
import updates


def answer(body):
    return patch.object(updates.urllib.request,'urlopen',return_value=io.BytesIO(body if isinstance(body,bytes) else json.dumps(body).encode()))


class UpdateCheckTests(unittest.TestCase):
    def test_reports_only_a_strictly_newer_published_version(self):
        for current,newer in [('0.1.20',True),('0.1.21',False),('0.1.22',False),('0.0.99',True)]:
            with answer({'tag_name':'v0.1.21','draft':False}):
                self.assertEqual(updates.check(current),{'latest':'0.1.21','newer':newer})
        # Versions compare as numbers, not text.
        with answer({'tag_name':'v0.1.10'}):
            self.assertEqual(updates.check('0.1.9'),{'latest':'0.1.10','newer':True})

    def test_asks_only_githubs_latest_release_with_the_version_and_nothing_else(self):
        with answer({'tag_name':'v0.1.21'}) as urlopen:
            updates.check('0.1.20')
        request=urlopen.call_args.args[0]
        self.assertEqual(request.full_url,'https://api.github.com/repos/chencyberlab/VoxLoom/releases/latest')
        self.assertEqual(request.get_method(),'GET')
        self.assertIsNone(request.data)
        self.assertEqual(request.get_header('User-agent'),'VoxLoom/0.1.20')
        self.assertEqual(sorted(request.header_items()),[('Accept','application/vnd.github+json'),('User-agent','VoxLoom/0.1.20')])

    def test_failures_become_plain_messages(self):
        def failing(error):
            return patch.object(updates.urllib.request,'urlopen',side_effect=error)
        http=lambda code:urllib.error.HTTPError(updates.LATEST,code,'',{},None)
        for error,message in [(http(404),'No VoxLoom release has been published yet.'),(http(403),'GitHub is busy. Try again in an hour.'),
                              (http(502),'GitHub could not answer just now. Try again later.'),
                              (urllib.error.URLError('offline'),"Couldn't reach GitHub. Check your internet connection and try again."),
                              (TimeoutError(),"Couldn't reach GitHub. Check your internet connection and try again.")]:
            with failing(error):
                self.assertEqual(updates.check('0.1.20'),{'error':message})
        for body in [b'not json',{'name':'no tag'},{'tag_name':'nightly'},{'tag_name':'v0.2.0-beta'},[]]:
            with answer(body):
                self.assertEqual(updates.check('0.1.20'),{'error':"GitHub's answer could not be read. Try again later."})


if __name__=='__main__':
    unittest.main()
