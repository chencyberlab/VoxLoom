-- 0.3.0: no search model. Search reads the transcript's own words (FTS5), so
-- the embedding index and its jobs go. 001.sql no longer creates these tables.
DELETE FROM jobs WHERE kind = 'index';
DROP TABLE IF EXISTS embedding_vectors;
DROP TABLE IF EXISTS embedding_items;
DROP TABLE IF EXISTS model_registry;
PRAGMA user_version = 5;
