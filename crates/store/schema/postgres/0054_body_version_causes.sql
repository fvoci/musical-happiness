-- How each version of a card's body came to be kept, as its readers are told:
-- 'manual' for one an editor asked for then, 'restored' for the body a version was
-- gone back to, and 'auto' for every other the server kept on its own (when its
-- writers left it, while it was edited on, around a body written whole, before a
-- version was gone back to, the first of an import or a copy). The versions kept
-- before this are 'auto': nothing else kept any then.
ALTER TABLE body_versions ADD COLUMN cause TEXT NOT NULL DEFAULT 'auto'
    CHECK (cause IN ('auto', 'manual', 'restored'));
