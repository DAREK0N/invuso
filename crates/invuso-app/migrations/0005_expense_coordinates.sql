-- Device location of an expense (EXP-10, AP-27).
--
-- idee.md 4.1 `Expense` only has the free-text `location`; the coordinates
-- are kept next to it on the user's decision in AP-27, because a place
-- name for them would need an online service (AGENTS.md 7.6). Both are
-- empty unless the user asked for the location.

ALTER TABLE expense ADD COLUMN latitude REAL;
ALTER TABLE expense ADD COLUMN longitude REAL;
