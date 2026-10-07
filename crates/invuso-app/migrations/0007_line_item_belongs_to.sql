-- A deposit or discount that belongs to the article above it (AP-38).
--
-- idee.md 4.1 `LineItem` has no such link; on the user's decision in AP-38
-- a line printed right below its article (`Pfand`, `Rabatt`, `Coupon`) is
-- carried by whoever carries that article. `belongs_to` names the article's
-- line; NULL for every line standing on its own.

ALTER TABLE line_item ADD COLUMN belongs_to TEXT REFERENCES line_item (id);
