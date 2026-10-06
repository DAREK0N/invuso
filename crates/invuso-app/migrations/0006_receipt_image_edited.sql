-- Corrected version of a receipt photo (RCP-05, OCR-04, AP-34).
--
-- idee.md 4.1 `Receipt.image_paths` only knows the originals; on the user's
-- decision in AP-34 the turned, cropped and perspective-corrected copy is
-- kept next to its original. `path` stays the untouched original
-- (AGENTS.md 7.4); recognition, thumbnail and viewer use `edited_path`
-- where there is one.

ALTER TABLE receipt_image ADD COLUMN edited_path TEXT;
