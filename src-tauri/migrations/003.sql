-- 0.1.22: recordings are transcribed after Stop, no longer while recording.
-- A queued pass of the old live kind becomes an ordinary transcription, which
-- resumes from the same checkpoint (asr_progress); a recording keeps one.
DELETE FROM jobs WHERE kind='transcribe_live' AND (status NOT IN ('pending','running')
 OR meeting_id IN (SELECT meeting_id FROM jobs WHERE kind='transcribe' AND status IN ('pending','running')));
UPDATE jobs SET kind='transcribe' WHERE kind='transcribe_live';
PRAGMA user_version = 3;
