-- Card a cash withdrawal was charged to (CASH-03, AP-22).
--
-- idee.md 4.1 `CashMovement` has no field for it; added on the user's
-- decision in AP-22. Empty for every other kind of movement.

ALTER TABLE cash_movement ADD COLUMN payment_method_id TEXT REFERENCES payment_method (id);
