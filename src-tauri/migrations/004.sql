-- 0.1.23: one notes page. A generated summary is part of a recording's notes,
-- which the person can edit, under the title "# Summary". Earlier summaries move
-- below the notes they kept beside them. An unfinished draft from a model's
-- thinking channel was never shown as notes and stays where it is, unused.
UPDATE meetings SET
 notes = notes || CASE WHEN notes='' THEN '' ELSE char(10)||char(10) END || '# Summary' || char(10)||char(10) || trim(summary, ' '||char(9)||char(10)||char(13)),
 summary = ''
WHERE trim(summary, ' '||char(9)||char(10)||char(13)) <> '' AND summary NOT LIKE '%<|channel>%' AND summary NOT LIKE '%<think>%' AND summary NOT LIKE '%Thinking Process:%';
-- Transcribe starts over only when its job starts (`Db::start_over`), so a
-- Transcribe cancelled before then keeps the transcript it would have replaced.
CREATE TABLE IF NOT EXISTS restart_jobs (job_id TEXT PRIMARY KEY REFERENCES jobs(id) ON DELETE CASCADE);
PRAGMA user_version = 4;
