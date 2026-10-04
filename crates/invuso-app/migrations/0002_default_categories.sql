-- Default categories (idee.md 4.1 `Category`, EXP-01).
--
-- Fixed ids, so every device has the same rows and a later sync does not
-- duplicate them. `name` is a stable key the app translates while
-- is_default = 1; origin_device_id 'seed' marks rows no device created.

INSERT INTO category
    (id, name, icon, color, is_default, sort_order, created_at, updated_at, origin_device_id)
VALUES
    ('default-food',       'food',       'utensils',      'pale-oak',    1, 1, CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'seed'),
    ('default-groceries',  'groceries',  'shopping-cart', 'muted-teal',  1, 2, CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'seed'),
    ('default-transport',  'transport',  'bus',           'cerulean',    1, 3, CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'seed'),
    ('default-lodging',    'lodging',    'bed',           'dusty-grape', 1, 4, CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'seed'),
    ('default-activities', 'activities', 'ticket',        'thistle',     1, 5, CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'seed'),
    ('default-shopping',   'shopping',   'shopping-bag',  'ash-grey',    1, 6, CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'seed'),
    ('default-health',     'health',     'heart-pulse',   'slate-grey',  1, 7, CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'seed'),
    ('default-other',      'other',      'tag',           'ash-grey',    1, 8, CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'seed');
