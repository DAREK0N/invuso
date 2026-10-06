//! A group as CSV for spreadsheets (DATA-03): one row per expense with
//! what each person paid and carries in the base currency, followed by
//! rows for its line items (user decision in AP-26).

use std::collections::BTreeMap;

use invuso_core::Decimal;
use invuso_core::domain::{Expense, Group, LineItem, Money, PersonId};
use invuso_core::split::SplitMode;

use crate::format::NumberFormat;
use crate::preferences::{category_name, kind_label};
use crate::storage::{Db, StorageError};

/// Field separator and decimal mark. German spreadsheets expect `;` and
/// `,`, English ones `,` and `.` (user decision in AP-26: follow the app
/// language).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvStyle {
    pub separator: char,
    pub decimal: char,
}

impl CsvStyle {
    /// Style of the current app language.
    pub fn current() -> Self {
        Self::for_decimal(NumberFormat::current().decimal)
    }

    fn for_decimal(decimal: char) -> Self {
        if decimal == ',' {
            Self {
                separator: ';',
                decimal: ',',
            }
        } else {
            Self {
                separator: ',',
                decimal: '.',
            }
        }
    }
}

/// File name for a group's export, e.g. `Japan Reise 2026-10-06.csv`;
/// characters file systems refuse are replaced.
pub fn csv_file_name(group: &Group, today: &str) -> String {
    let name: String = group
        .name
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_matches('.');
    if name.is_empty() {
        format!("invuso {today}.csv")
    } else {
        format!("{name} {today}.csv")
    }
}

/// The group's expenses as CSV text: UTF-8 with byte order mark (so Excel
/// reads umlauts), CRLF line ends (RFC 4180).
pub fn group_csv(db: &Db, group: &Group, style: CsvStyle) -> Result<String, StorageError> {
    let expenses = db.group_expenses(&group.id)?;
    let categories: BTreeMap<_, _> = db
        .all_categories()?
        .into_iter()
        .map(|category| (category.id.clone(), category_name(&category)))
        .collect();

    // Members first in their order, then anyone who left but still shows
    // up in an expense.
    let mut people: Vec<PersonId> = db
        .group_members(&group.id)?
        .into_iter()
        .map(|member| member.person.id)
        .collect();
    for expense in &expenses {
        let involved = expense
            .payments
            .iter()
            .map(|payment| &payment.person_id)
            .chain(expense.split.participants().iter())
            .cloned()
            .collect::<Vec<_>>();
        for id in involved {
            if !people.contains(&id) {
                people.push(id);
            }
        }
    }
    let names: BTreeMap<PersonId, String> = db
        .people_any(&people)?
        .into_iter()
        .map(|(id, person)| (id, person.name))
        .collect();
    let name_of = |id: &PersonId| names.get(id).cloned().unwrap_or_default();

    let base = group.base_currency.code();
    let mut header = vec![
        t!("export.type").to_string(),
        t!("export.date").to_string(),
        t!("export.title").to_string(),
        t!("export.category").to_string(),
        t!("export.amount").to_string(),
        t!("export.currency").to_string(),
        t!("export.rate", currency = base).to_string(),
        t!("export.amount_base", currency = base).to_string(),
        t!("export.split").to_string(),
        t!("export.quantity").to_string(),
        t!("export.item_kind").to_string(),
        t!("export.assigned").to_string(),
    ];
    for id in &people {
        let name = name_of(id);
        header.push(t!("export.paid", name = name, currency = base).to_string());
        header.push(t!("export.share", name = name, currency = base).to_string());
    }

    let mut rows = vec![header];
    for expense in &expenses {
        let rate = match &expense.fx_rate_id {
            Some(id) => db
                .archived_rate(id, expense.total.currency(), group.base_currency)?
                .map(|quote| decimal_text(quote.rate.value(), style)),
            None => None,
        };
        let paid = expense.payments_in_base()?;
        let shares = expense.shares_in_base()?;
        let mut row = vec![
            t!("export.row_expense").to_string(),
            date_time(&expense.occurred_at),
            text_cell(&expense.title),
            expense
                .category_id
                .as_ref()
                .and_then(|id| categories.get(id))
                .map(|name| text_cell(name))
                .unwrap_or_default(),
            amount_text(expense.total, style),
            expense.total.currency().code().to_string(),
            rate.unwrap_or_default(),
            amount_text(expense.total_in_base, style),
            split_label(&expense.split),
            String::new(),
            String::new(),
            String::new(),
        ];
        for id in &people {
            let cell = |figures: &BTreeMap<PersonId, i64>| {
                figures
                    .get(id)
                    .map(|minor| amount_text(Money::new(*minor, group.base_currency), style))
                    .unwrap_or_default()
            };
            row.push(cell(&paid));
            row.push(cell(&shares));
        }
        rows.push(row);
        for item in &expense.line_items {
            rows.push(item_row(expense, item, people.len(), style, &name_of));
        }
    }

    let mut csv = String::from("\u{feff}");
    for row in rows {
        let cells: Vec<String> = row.iter().map(|cell| quoted(cell, style)).collect();
        csv.push_str(&cells.join(&style.separator.to_string()));
        csv.push_str("\r\n");
    }
    Ok(csv)
}

/// A line item below its expense: text, amount in the expense's currency,
/// quantity, kind and who it is assigned to.
fn item_row(
    expense: &Expense,
    item: &LineItem,
    people: usize,
    style: CsvStyle,
    name_of: &impl Fn(&PersonId) -> String,
) -> Vec<String> {
    let assigned = if item.assigned_to.is_empty() {
        t!("export.everyone").to_string()
    } else {
        item.assigned_to
            .iter()
            .map(|(id, weight)| {
                if *weight == Decimal::ONE {
                    name_of(id)
                } else {
                    format!("{} ({})", name_of(id), decimal_text(*weight, style))
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut row = vec![
        t!("export.row_item").to_string(),
        String::new(),
        text_cell(item.text()),
        String::new(),
        amount_text(
            Money::new(item.total_minor, expense.total.currency()),
            style,
        ),
        expense.total.currency().code().to_string(),
        String::new(),
        String::new(),
        String::new(),
        decimal_text(item.quantity, style),
        kind_label(item.kind),
        text_cell(&assigned),
    ];
    row.resize(row.len() + 2 * people, String::new());
    row
}

fn split_label(split: &SplitMode) -> String {
    match split {
        SplitMode::Equal(_) => t!("expense.split_mode_equal"),
        SplitMode::Weights(_) => t!("expense.split_mode_weights"),
        SplitMode::Percent(_) => t!("expense.split_mode_percent"),
        SplitMode::Exact(_) => t!("expense.split_mode_exact"),
        SplitMode::Items { .. } => t!("expense.split_mode_items"),
    }
    .to_string()
}

/// `2026-10-04T19:30:00+09:00` → `2026-10-04 19:30`, the local time the
/// expense happened, in a form spreadsheets read as a date.
fn date_time(occurred_at: &str) -> String {
    match (occurred_at.get(..10), occurred_at.get(11..16)) {
        (Some(date), Some(time)) => format!("{date} {time}"),
        _ => occurred_at.to_string(),
    }
}

/// An amount without grouping, with exactly the currency's decimals:
/// `-1234,50`, `1000`.
fn amount_text(money: Money, style: CsvStyle) -> String {
    let exponent = money.currency().exponent();
    let minor = money.amount_minor().unsigned_abs();
    let sign = if money.is_negative() { "-" } else { "" };
    if exponent == 0 {
        return format!("{sign}{minor}");
    }
    let scale = 10_u64.pow(exponent);
    format!(
        "{sign}{}{}{:0width$}",
        minor / scale,
        style.decimal,
        minor % scale,
        width = exponent as usize
    )
}

/// A rate, quantity or weight with all its digits: `0,0061`, `1,5`.
fn decimal_text(value: Decimal, style: CsvStyle) -> String {
    value
        .normalize()
        .to_string()
        .replace('.', &style.decimal.to_string())
}

/// Text from the user or a receipt: a leading `=`, `+`, `-` or `@` would
/// make a spreadsheet run it as a formula, so it gets a leading `'`.
fn text_cell(text: &str) -> String {
    if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_string()
    }
}

/// Quotes a cell that contains the separator, a quote or a line break.
fn quoted(cell: &str, style: CsvStyle) -> String {
    if cell.contains([style.separator, '"', '\n', '\r']) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use invuso_core::domain::{Currency, ExpenseSource, LineItemKind};

    use super::*;
    use crate::storage::{NewExpense, NewExpensePayment, NewGroup, NewPerson};

    const GERMAN: CsvStyle = CsvStyle {
        separator: ';',
        decimal: ',',
    };

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn person(db: &Db, name: &str) -> PersonId {
        db.create_person(NewPerson {
            name: name.into(),
            color: "thistle".into(),
            is_me: false,
            note: None,
        })
        .unwrap()
        .id
    }

    fn cells(line: &str) -> Vec<&str> {
        line.split(';').collect()
    }

    #[test]
    fn styles_follow_the_decimal_mark() {
        assert_eq!(CsvStyle::for_decimal(','), GERMAN);
        assert_eq!(
            CsvStyle::for_decimal('.'),
            CsvStyle {
                separator: ',',
                decimal: '.'
            }
        );
    }

    #[test]
    fn amounts_keep_the_currency_decimals() {
        assert_eq!(
            amount_text(Money::new(123_450, cur("EUR")), GERMAN),
            "1234,50"
        );
        assert_eq!(amount_text(Money::new(-5, cur("EUR")), GERMAN), "-0,05");
        assert_eq!(amount_text(Money::new(3200, cur("JPY")), GERMAN), "3200");
        assert_eq!(amount_text(Money::new(1234, cur("KWD")), GERMAN), "1,234");
        let english = CsvStyle::for_decimal('.');
        assert_eq!(amount_text(Money::new(1250, cur("USD")), english), "12.50");
        assert_eq!(decimal_text(Decimal::new(61_000, 7), GERMAN), "0,0061");
    }

    #[test]
    fn cells_are_quoted_and_formulas_defused() {
        assert_eq!(quoted("Bier; Wein", GERMAN), "\"Bier; Wein\"");
        assert_eq!(quoted("\"Spezial\"", GERMAN), "\"\"\"Spezial\"\"\"");
        assert_eq!(quoted("a, b", GERMAN), "a, b");
        assert_eq!(text_cell("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(text_cell("-10 % Rabatt"), "'-10 % Rabatt");
        assert_eq!(text_cell("Ramen"), "Ramen");
    }

    #[test]
    fn file_names_are_safe() {
        let mut group = Group {
            id: invuso_core::domain::GroupId::new("g"),
            name: "Japan/Reise: 2026".into(),
            icon: "plane".into(),
            color: "cerulean".into(),
            base_currency: cur("EUR"),
            start_date: None,
            end_date: None,
            target_language: None,
            archived: false,
        };
        assert_eq!(
            csv_file_name(&group, "2026-10-06"),
            "Japan_Reise_ 2026 2026-10-06.csv"
        );
        group.name = "..".into();
        assert_eq!(csv_file_name(&group, "2026-10-06"), "invuso 2026-10-06.csv");
    }

    #[test]
    fn a_group_exports_expenses_shares_and_items() {
        let db = Db::open_in_memory().unwrap();
        let anna = person(&db, "Anna");
        let ben = person(&db, "Ben");
        let group = db
            .create_group(NewGroup {
                name: "Japan Reise".into(),
                icon: "plane".into(),
                color: "cerulean".into(),
                base_currency: cur("EUR"),
                start_date: None,
                end_date: None,
                target_language: None,
            })
            .unwrap();
        db.add_group_member(&group.id, &anna).unwrap();
        db.add_group_member(&group.id, &ben).unwrap();
        let rate = db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let everyone: BTreeSet<PersonId> = [anna.clone(), ben.clone()].into();
        db.create_expense(
            NewExpense {
                group_id: Some(group.id.clone()),
                title: "Essen".into(),
                category_id: None,
                occurred_at: "2026-10-04T19:30:00+09:00".into(),
                total: Money::new(1001, cur("EUR")),
                payments: vec![NewExpensePayment {
                    person_id: anna.clone(),
                    payment_method_id: None,
                    amount_minor: 1001,
                }],
                split: SplitMode::Equal(everyone),
                receipt_id: None,
                line_items: vec![LineItem {
                    original_text: "Ramen".into(),
                    quantity: Decimal::ONE,
                    total_minor: 1001,
                    kind: LineItemKind::Article,
                    assigned_to: [(ben.clone(), Decimal::new(2, 0))].into(),
                    ..LineItem::default()
                }],
                source: ExpenseSource::Manual,
                note: None,
                location: None,
                coordinates: None,
                own_rate: None,
            },
            &rate,
        )
        .unwrap();

        let csv = group_csv(&db, &group, GERMAN).unwrap();
        assert!(csv.starts_with('\u{feff}'));
        let lines: Vec<&str> = csv.trim_end().split("\r\n").collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(cells(lines[0]).len(), 12 + 4);

        let expense = cells(lines[1]);
        assert_eq!(&expense[1..3], ["2026-10-04 19:30", "Essen"]);
        assert_eq!(&expense[4..8], ["10,01", "EUR", "", "10,01"]);
        assert!(!expense[8].is_empty());
        // Anna paid everything; the odd cent goes to one of the two.
        assert_eq!((expense[12], expense[14]), ("10,01", ""));
        let shares: BTreeSet<&str> = [expense[13], expense[15]].into();
        assert_eq!(shares, ["5,00", "5,01"].into());

        let item = cells(lines[2]);
        assert_eq!(item[2], "Ramen");
        assert_eq!(item[4], "10,01");
        assert_eq!(item[9], "1");
        assert_eq!(item[11], "Ben (2)");
        assert_eq!(item.len(), 16);
    }
    #[test]
    fn a_foreign_expense_shows_its_rate_in_the_base_currency() {
        let db = Db::open_in_memory().unwrap();
        let anna = person(&db, "Anna");
        let group = db
            .create_group(NewGroup {
                name: "Japan".into(),
                icon: "plane".into(),
                color: "cerulean".into(),
                base_currency: cur("EUR"),
                start_date: None,
                end_date: None,
                target_language: None,
            })
            .unwrap();
        db.add_group_member(&group.id, &anna).unwrap();
        db.archive_rates(
            "frankfurter",
            1,
            &[crate::storage::NewExchangeRate {
                rate: invuso_core::fx::Rate::new(cur("EUR"), cur("JPY"), Decimal::new(160, 0))
                    .unwrap(),
                rate_date: "2026-10-03".into(),
            }],
        )
        .unwrap();
        let rate = db
            .rate_on(cur("JPY"), cur("EUR"), "2026-10-03")
            .unwrap()
            .unwrap();
        db.create_expense(
            NewExpense {
                group_id: Some(group.id.clone()),
                title: "Ramen".into(),
                category_id: None,
                occurred_at: "2026-10-03T12:00:00+09:00".into(),
                total: Money::new(3000, cur("JPY")),
                payments: vec![NewExpensePayment {
                    person_id: anna.clone(),
                    payment_method_id: None,
                    amount_minor: 3000,
                }],
                split: SplitMode::Equal([anna.clone()].into()),
                receipt_id: None,
                line_items: Vec::new(),
                source: ExpenseSource::Manual,
                note: None,
                location: None,
                coordinates: None,
                own_rate: None,
            },
            &rate,
        )
        .unwrap();

        let csv = group_csv(&db, &group, GERMAN).unwrap();
        let row = csv.trim_end().split("\r\n").nth(1).unwrap();
        // 3 000 ¥ at 1 ¥ = 0,00625 € is 18,75 €.
        assert_eq!(&cells(row)[4..8], ["3000", "JPY", "0,00625", "18,75"]);
    }
}
