//! The group's timeline: expenses and settlements in one list (GRP-26),
//! filtered and searched (GRP-24, GRP-25).

use std::collections::{BTreeMap, BTreeSet};

use invuso_core::domain::{CategoryId, GroupId, PaymentMethodId, PersonId, Settlement, local_date};

use crate::storage::{Db, StorageError, TimelineEntry};

/// A settlement as the timeline shows it, with the names it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineSettlement {
    pub settlement: Settlement,
    /// `None` if the person is unknown, e.g. removed from the database.
    pub from: Option<String>,
    pub to: Option<String>,
    pub method: Option<String>,
}

/// One row of the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineItem {
    Expense(TimelineEntry),
    Settlement(TimelineSettlement),
}

impl TimelineItem {
    pub fn occurred_at(&self) -> &str {
        match self {
            Self::Expense(entry) => &entry.occurred_at,
            Self::Settlement(item) => &item.settlement.occurred_at,
        }
    }

    /// Local day and time, newest sorting last: the order of the timeline
    /// reversed.
    fn sort_key(&self) -> (&str, &str) {
        let at = self.occurred_at();
        (local_date(at), at.get(11..19).unwrap_or_default())
    }
}

/// What the timeline is narrowed to. Several values of one kind match any
/// of them; different kinds must all match (user decision in AP-29).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimelineFilter {
    /// Taking part: paying, sharing, assigned a line; for a settlement,
    /// sending or receiving it.
    pub people: BTreeSet<PersonId>,
    /// Settlements have no category, so they are left out when set.
    pub categories: BTreeSet<CategoryId>,
    pub methods: BTreeSet<PaymentMethodId>,
    /// First and last day, `YYYY-MM-DD`, both included.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Only expenses with a receipt; leaves out settlements.
    pub receipt_only: bool,
    /// Text searched in, ignoring case (GRP-25); blank searches nothing.
    pub query: String,
}

impl TimelineFilter {
    /// Whether anything but the search narrows the list.
    pub fn has_filters(&self) -> bool {
        !self.people.is_empty()
            || !self.categories.is_empty()
            || !self.methods.is_empty()
            || self.from.is_some()
            || self.to.is_some()
            || self.receipt_only
    }

    /// Whether the list is narrowed at all, by filters or the search.
    pub fn is_active(&self) -> bool {
        self.has_filters() || !self.query.trim().is_empty()
    }

    pub fn matches(&self, item: &TimelineItem) -> bool {
        let day = local_date(item.occurred_at());
        if self.from.as_deref().is_some_and(|from| day < from)
            || self.to.as_deref().is_some_and(|to| day > to)
        {
            return false;
        }
        let query = self.query.trim().to_lowercase();
        match item {
            TimelineItem::Expense(entry) => {
                (self.people.is_empty() || !self.people.is_disjoint(&entry.people))
                    && (self.categories.is_empty()
                        || entry
                            .category_id
                            .as_ref()
                            .is_some_and(|c| self.categories.contains(c)))
                    && (self.methods.is_empty() || !self.methods.is_disjoint(&entry.method_ids))
                    && (!self.receipt_only || entry.has_receipt)
                    && contains(
                        &query,
                        [entry.title.as_str()]
                            .into_iter()
                            .chain(entry.merchant.as_deref())
                            .chain(entry.item_texts.iter().map(String::as_str)),
                    )
            }
            TimelineItem::Settlement(item) => {
                let settlement = &item.settlement;
                self.categories.is_empty()
                    && !self.receipt_only
                    && (self.people.is_empty()
                        || self.people.contains(&settlement.from)
                        || self.people.contains(&settlement.to))
                    && (self.methods.is_empty()
                        || settlement
                            .payment_method_id
                            .as_ref()
                            .is_some_and(|m| self.methods.contains(m)))
                    && contains(
                        &query,
                        [&item.from, &item.to, &item.method, &settlement.note]
                            .into_iter()
                            .filter_map(|text| text.as_deref()),
                    )
            }
        }
    }
}

/// Whether one of `texts` contains `query`, already lowercase; an empty
/// query is in everything.
fn contains<'a>(query: &str, mut texts: impl Iterator<Item = &'a str>) -> bool {
    query.is_empty() || texts.any(|text| text.to_lowercase().contains(query))
}

/// The group's expenses and settlements, newest first; within the same
/// minute an expense comes before a settlement.
pub fn group_timeline(db: &Db, group: &GroupId) -> Result<Vec<TimelineItem>, StorageError> {
    let expenses = db.group_timeline(group)?;
    let settlements = db.group_settlements(group)?;
    let people = db.people_any(settlements.iter().flat_map(|s| [&s.from, &s.to]))?;
    let methods: BTreeMap<PaymentMethodId, String> = db
        .payment_methods()?
        .into_iter()
        .map(|method| (method.id, method.name))
        .collect();
    let mut settlements: Vec<TimelineItem> = settlements
        .into_iter()
        .map(|settlement| {
            TimelineItem::Settlement(TimelineSettlement {
                from: people.get(&settlement.from).map(|p| p.name.clone()),
                to: people.get(&settlement.to).map(|p| p.name.clone()),
                method: settlement
                    .payment_method_id
                    .as_ref()
                    .and_then(|id| methods.get(id))
                    .cloned(),
                settlement,
            })
        })
        .collect();
    // Stored order compares the text with its offset; the timeline goes by
    // local day and time like the expenses.
    settlements.sort_by(|a, b| b.sort_key().cmp(&a.sort_key()));

    let mut items = Vec::with_capacity(expenses.len() + settlements.len());
    let mut settlements = settlements.into_iter().peekable();
    for expense in expenses.into_iter().map(TimelineItem::Expense) {
        while let Some(settlement) = settlements.next_if(|s| s.sort_key() > expense.sort_key()) {
            items.push(settlement);
        }
        items.push(expense);
    }
    items.extend(settlements);
    Ok(items)
}

/// The items `filter` lets through, in their order.
pub fn filter_timeline(items: &[TimelineItem], filter: &TimelineFilter) -> Vec<TimelineItem> {
    items
        .iter()
        .filter(|item| filter.matches(item))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use invuso_core::Decimal;
    use invuso_core::domain::{
        Currency, ExpenseSource, Group, LineItem, LineItemKind, Money, PaymentMethodKind,
        item_lines,
    };
    use invuso_core::split::SplitMode;

    use super::*;
    use crate::storage::{
        NewExpense, NewExpensePayment, NewGroup, NewPaymentMethod, NewPerson, NewSettlement,
        Profile,
    };

    fn eur() -> Currency {
        Currency::from_code("EUR").unwrap()
    }

    struct Setup {
        db: Db,
        group: Group,
        me: PersonId,
        anna: PersonId,
        ben: PersonId,
        visa: PaymentMethodId,
    }

    fn setup() -> Setup {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: eur(),
            target_language: "de".into(),
        })
        .unwrap();
        let me = db.me().unwrap().unwrap().id;
        let person = |name: &str| {
            db.create_person(NewPerson {
                name: name.into(),
                color: "thistle".into(),
                is_me: false,
                note: None,
            })
            .unwrap()
            .id
        };
        let (anna, ben) = (person("Anna"), person("Ben"));
        let group = db
            .create_group(NewGroup {
                name: "Japan Reise".into(),
                icon: "plane".into(),
                color: "cerulean".into(),
                base_currency: eur(),
                start_date: None,
                end_date: None,
                target_language: None,
            })
            .unwrap();
        db.add_group_member(&group.id, &anna).unwrap();
        db.add_group_member(&group.id, &ben).unwrap();
        let visa = db
            .create_payment_method(NewPaymentMethod {
                name: "Visa".into(),
                kind: PaymentMethodKind::CreditCard,
                owner_person_id: Some(anna.clone()),
                last4: None,
                account: Default::default(),
                color: "cerulean".into(),
                icon: "credit-card".into(),
            })
            .unwrap()
            .id;
        Setup {
            db,
            group,
            me,
            anna,
            ben,
            visa,
        }
    }

    impl Setup {
        /// Paid by `payer`, shared equally by `between`.
        fn expense(
            &self,
            title: &str,
            at: &str,
            payer: &PersonId,
            method: Option<&PaymentMethodId>,
            between: &[&PersonId],
        ) -> NewExpense {
            NewExpense {
                group_id: Some(self.group.id.clone()),
                title: title.into(),
                category_id: None,
                occurred_at: at.into(),
                total: Money::new(1_200, eur()),
                payments: vec![NewExpensePayment {
                    person_id: payer.clone(),
                    payment_method_id: method.cloned(),
                    amount_minor: 1_200,
                }],
                split: SplitMode::Equal(between.iter().map(|p| (*p).clone()).collect()),
                receipt_id: None,
                line_items: Vec::new(),
                source: ExpenseSource::Manual,
                note: None,
                location: None,
                coordinates: None,
                own_rate: None,
            }
        }

        fn save(&self, new: NewExpense) {
            let rate = self.db.latest_rate(eur(), eur()).unwrap().unwrap();
            self.db.create_expense(new, &rate).unwrap();
        }

        fn settle(&self, from: &PersonId, to: &PersonId, at: &str, note: Option<&str>) {
            self.db
                .create_settlement(NewSettlement {
                    group_id: self.group.id.clone(),
                    from: from.clone(),
                    to: to.clone(),
                    amount: Money::new(500, eur()),
                    payment_method_id: None,
                    occurred_at: at.into(),
                    note: note.map(str::to_string),
                })
                .unwrap();
        }

        fn titles(&self, filter: &TimelineFilter) -> Vec<String> {
            let items = group_timeline(&self.db, &self.group.id).unwrap();
            filter_timeline(&items, filter)
                .iter()
                .map(|item| match item {
                    TimelineItem::Expense(entry) => entry.title.clone(),
                    TimelineItem::Settlement(s) => format!(
                        "{} → {}",
                        s.from.as_deref().unwrap_or("?"),
                        s.to.as_deref().unwrap_or("?")
                    ),
                })
                .collect()
        }
    }

    fn line(original: &str, translated: Option<&str>, assigned: &[&PersonId]) -> LineItem {
        LineItem {
            original_text: original.into(),
            translated_text: translated.map(str::to_string),
            user_text: None,
            quantity: Decimal::ONE,
            unit_price_minor: None,
            total_minor: 600,
            kind: LineItemKind::Article,
            assigned_to: assigned
                .iter()
                .map(|p| ((*p).clone(), Decimal::ONE))
                .collect(),
            ocr_confidence: None,
            edited_by_user: false,
            attached: false,
        }
    }

    /// Japan Reise: hotel paid by Ich for Ich and Anna; ramen paid by Anna
    /// with Visa, one line assigned to Ben; taxi paid by Anna; Ben pays Ich
    /// back.
    fn japan() -> Setup {
        let s = setup();
        s.save(s.expense(
            "Hotel",
            "2026-10-01T15:00:00+09:00",
            &s.me,
            None,
            &[&s.me, &s.anna],
        ));
        let lines = vec![
            line("味噌ラーメン", Some("Miso-Ramen"), &[&s.ben]),
            line("餃子", Some("Gyoza"), &[]),
        ];
        s.save(NewExpense {
            split: SplitMode::Items {
                participants: [(s.me.clone(), Decimal::ONE)].into(),
                items: item_lines(&lines),
            },
            line_items: lines,
            ..s.expense(
                "Ichiran",
                "2026-10-02T20:00:00+09:00",
                &s.anna,
                Some(&s.visa),
                &[],
            )
        });
        s.save(s.expense(
            "Taxi",
            "2026-10-03T09:00:00+09:00",
            &s.anna,
            None,
            &[&s.anna, &s.me],
        ));
        s.settle(
            &s.ben,
            &s.me,
            "2026-10-03T12:00:00+09:00",
            Some("Bar zurück"),
        );
        s
    }

    #[test]
    fn settlements_sort_in_by_local_day_and_time() {
        let s = japan();
        // Earlier the same local day than the taxi, but later in UTC text.
        s.settle(&s.anna, &s.me, "2026-10-03T08:00:00+02:00", None);
        assert_eq!(
            s.titles(&TimelineFilter::default()),
            ["Ben → Ich", "Taxi", "Anna → Ich", "Ichiran", "Hotel"]
        );
    }

    #[test]
    fn search_ignores_case_and_looks_into_lines() {
        let s = japan();
        let search = |query: &str| {
            s.titles(&TimelineFilter {
                query: query.into(),
                ..Default::default()
            })
        };
        // Translation of a line.
        assert_eq!(search("ramen"), ["Ichiran"]);
        // Original text of a line.
        assert_eq!(search("餃子"), ["Ichiran"]);
        assert_eq!(search("  TAXI "), ["Taxi"]);
        // Settlements are found by their note and names.
        assert_eq!(search("bar zurück"), ["Ben → Ich"]);
        assert_eq!(search("ben"), ["Ben → Ich"]);
        assert!(search("Sushi").is_empty());
        assert_eq!(search("").len(), 4);
    }

    #[test]
    fn search_finds_a_line_correction_and_the_merchant() {
        let s = japan();
        let receipt = s.db.create_receipt("receipts/r.jpg", None).unwrap();
        let mut corrected = line("ｺｰﾋｰ", None, &[]);
        corrected.user_text = Some("Kaffee".into());
        s.save(NewExpense {
            receipt_id: Some(receipt.id.clone()),
            line_items: vec![corrected],
            ..s.expense(
                "Frühstück",
                "2026-10-04T08:00:00+09:00",
                &s.me,
                None,
                &[&s.me],
            )
        });
        let search = |query: &str| {
            s.titles(&TimelineFilter {
                query: query.into(),
                ..Default::default()
            })
        };
        assert_eq!(search("kaffee"), ["Frühstück"]);
        assert_eq!(
            s.titles(&TimelineFilter {
                receipt_only: true,
                ..Default::default()
            }),
            ["Frühstück"]
        );
    }

    #[test]
    fn person_filter_matches_payers_shares_lines_and_settlements() {
        let s = japan();
        let only = |people: &[&PersonId]| {
            s.titles(&TimelineFilter {
                people: people.iter().map(|p| (*p).clone()).collect(),
                ..Default::default()
            })
        };
        // Ben only carries an assigned line and sent the settlement.
        assert_eq!(only(&[&s.ben]), ["Ben → Ich", "Ichiran"]);
        // Anna paid the taxi and the ramen and shares the hotel.
        assert_eq!(only(&[&s.anna]), ["Taxi", "Ichiran", "Hotel"]);
        // Several people: any of them.
        assert_eq!(only(&[&s.ben, &s.anna]).len(), 4);
    }

    #[test]
    fn method_category_and_receipt_filters_leave_out_what_lacks_them() {
        let s = japan();
        assert_eq!(
            s.titles(&TimelineFilter {
                methods: [s.visa.clone()].into(),
                ..Default::default()
            }),
            ["Ichiran"]
        );
        // No expense has a category, and settlements never do.
        assert!(
            s.titles(&TimelineFilter {
                categories: [CategoryId::new("default-food")].into(),
                ..Default::default()
            })
            .is_empty()
        );
        assert!(
            s.titles(&TimelineFilter {
                receipt_only: true,
                ..Default::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn date_range_includes_both_days_and_combines_with_other_filters() {
        let s = japan();
        let range = TimelineFilter {
            from: Some("2026-10-02".into()),
            to: Some("2026-10-03".into()),
            ..Default::default()
        };
        assert_eq!(s.titles(&range), ["Ben → Ich", "Taxi", "Ichiran"]);
        assert_eq!(
            s.titles(&TimelineFilter {
                people: [s.ben.clone()].into(),
                query: "ramen".into(),
                ..range.clone()
            }),
            ["Ichiran"]
        );
        assert_eq!(
            s.titles(&TimelineFilter {
                from: Some("2026-10-03".into()),
                ..Default::default()
            }),
            ["Ben → Ich", "Taxi"]
        );
        assert!(range.is_active() && range.has_filters());
        assert!(!TimelineFilter::default().is_active());
    }
}
