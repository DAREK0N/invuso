-- Recognized text boxes of a receipt (OCR-01, AP-18).
--
-- `ocr_raw_text` keeps the plain rows (idee.md 4.1); the parser, the review
-- screen (AP-19) and marking a position in the photo (OCR-37) need every
-- fragment with its box and confidence, stored here as JSON:
-- {"skew_degrees", "fragments": [{"text", "box": [left, top, right, bottom],
-- "confidence"}]}. Boxes are in the photo turned back by skew_degrees around
-- its centre, so the printed rows are level.

ALTER TABLE receipt ADD COLUMN ocr_boxes TEXT;
