use std::collections::{BTreeMap, BTreeSet};

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCircleAlert, LdPin, LdPlus, LdTrash2, LdTriangleAlert},
};
use invuso_core::domain::{Group, GroupId, GroupMember, Money, Person, PersonId};
use invuso_core::split::GroupSummary;

use super::form::GroupNotFound;
use super::settle::{DebtRow, SettleChoices, SettleDraft, SettleSheet, person_label};
use super::{DeleteGroupSheet, group_subtitle, mark_active};
use crate::Route;
use crate::components::{
    Avatar, AvatarEntry, AvatarSize, AvatarStack, Button, ButtonVariant, CardSection, EmptyState,
    GroupIcon, LinkRow, MoneyText, OwnBalance, TopBar,
};
use crate::format::{NumberFormat, format_money};
use crate::services::summary::group_summary;
use crate::state::{DataRevision, Toaster};
use crate::storage::{Db, StorageError};

/// Everything the overview shows of one group.
#[derive(Debug, Clone, PartialEq)]
struct Overview {
    group: Group,
    members: Vec<GroupMember>,
    summary: GroupSummary,
    /// Everyone in `summary`, also people removed from the group or deleted
    /// since, who still count with their expenses.
    people: BTreeMap<PersonId, Person>,
    me: Option<PersonId>,
    /// Marked as the active group (GRP-05).
    active: bool,
    choices: SettleChoices,
}

/// `/groups/:id`: total spent, own balance, who owes whom, who paid and who
/// owes most, and everyone's paid / share / balance (GRP-10..14, SPL-03,
/// SPL-04), with links to the expenses (GRP-20), members, debts and
/// settlements, and editing. Tapping a debt marks it as paid (GRP-15).
#[component]
pub fn GroupOverview(id: String) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut confirm_delete = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    let group_id = use_memo(use_reactive!(|id| GroupId::new(id)));
    let data = use_memo(move || {
        revision.track();
        load(&db, &group_id()).map_err(|e| e.to_string())
    });

    rsx! {
        TopBar { title: t!("page.group_overview").to_string(), show_back: true }
        match &*data.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("summary.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(None) => rsx! { GroupNotFound {} },
            Ok(Some(overview)) => {
                let group = overview.group.clone();
                rsx! {
                    div { class: "mx-4 flex flex-col gap-5 pt-6 safe-area-x",
                        div { class: "flex flex-col items-center gap-3 text-center",
                            GroupIcon { icon: group.icon.clone(), color: group.color.clone(), large: true }
                            h2 { class: "text-2xl font-semibold break-words text-floral-white-50", "{group.name}" }
                            p { class: "text-sm text-floral-white-400", {group_subtitle(&group)} }
                            ActiveMark { group: group.clone(), active: overview.active }
                        }
                        Totals { overview: overview.clone() }
                        div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                            LinkRow {
                                label: t!("group.expenses").to_string(),
                                onclick: {
                                    let id = group.id.as_str().to_string();
                                    move |_| {
                                        nav.push(Route::GroupTimeline { id: id.clone() });
                                    }
                                },
                                span { class: "text-sm tabular-nums text-floral-white-400",
                                    "{overview.summary.expense_count + overview.summary.skipped_count}"
                                }
                            }
                            LinkRow {
                                label: t!("page.group_members").to_string(),
                                onclick: {
                                    let id = group.id.as_str().to_string();
                                    move |_| {
                                        nav.push(Route::GroupMembers {
                                            id: id.clone(),
                                            setup: false,
                                        });
                                    }
                                },
                                AvatarStack { people: avatars(&overview.members), max: 3 }
                            }
                            LinkRow {
                                label: t!("page.group_settle").to_string(),
                                onclick: {
                                    let id = group.id.as_str().to_string();
                                    move |_| {
                                        nav.push(Route::GroupSettle {
                                            id: id.clone(),
                                            record: false,
                                        });
                                    }
                                },
                            }
                            LinkRow {
                                label: t!("common.edit").to_string(),
                                onclick: {
                                    let id = group.id.as_str().to_string();
                                    move |_| {
                                        nav.push(Route::GroupEdit { id: id.clone() });
                                    }
                                },
                            }
                        }
                        if overview.summary.expense_count > 0 {
                            Balances { overview: overview.clone() }
                        }
                        Button {
                            variant: ButtonVariant::Danger,
                            class: "w-full",
                            onclick: move |_| {
                                delete_error.set(None);
                                confirm_delete.set(true);
                            },
                            Icon { icon: LdTrash2, class: "h-5 w-5" }
                            {t!("group.delete").to_string()}
                        }
                    }
                    if confirm_delete() {
                        DeleteGroupSheet {
                            group,
                            error: delete_error(),
                            on_deleted: move |_| {
                                confirm_delete.set(false);
                                // The undo toast stays visible on the list.
                                if nav.can_go_back() {
                                    nav.go_back();
                                } else {
                                    nav.replace(Route::GroupList {});
                                }
                            },
                            on_error: move |message| delete_error.set(Some(message)),
                            on_close: move |_| confirm_delete.set(false),
                        }
                    }
                }
            }
        }
    }
}

fn load(db: &Db, id: &GroupId) -> Result<Option<Overview>, StorageError> {
    let Some(group) = db.group(id)? else {
        return Ok(None);
    };
    let members = db.group_members(id)?;
    let summary = group_summary(db, &group)?;
    let people = db.people_any(summary.people.keys())?;
    let me = db.me()?.map(|person| person.id);
    let choices = SettleChoices::load(db, id)?;
    let active = db
        .active_group()?
        .is_some_and(|active| active.id == group.id);
    Ok(Some(Overview {
        group,
        members,
        summary,
        people,
        me,
        active,
        choices,
    }))
}

/// "Aktive Gruppe" if the group is the active one, otherwise the button
/// to make it so (GRP-05).
#[component]
fn ActiveMark(group: Group, active: bool) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();

    if active {
        return rsx! {
            span { class: "flex min-h-8 items-center gap-1.5 rounded-full bg-cerulean-800 px-3 text-sm font-medium text-cerulean-100",
                Icon { icon: LdPin, class: "h-4 w-4" }
                {t!("group.active").to_string()}
            }
        };
    }
    rsx! {
        Button {
            variant: ButtonVariant::Secondary,
            onclick: move |_| mark_active(&db, Some(&group), revision, toaster),
            Icon { icon: LdPin, class: "h-5 w-5" }
            {t!("group.mark_active").to_string()}
        }
    }
}

/// Total spent in large type and the own balance (GRP-10, PER-02), or the
/// way to the first expense while there is none.
#[component]
fn Totals(overview: Overview) -> Element {
    let nav = use_navigator();
    let summary = &overview.summary;
    let base = overview.group.base_currency;
    let own = overview
        .me
        .as_ref()
        .and_then(|me| summary.people.get(me))
        .map(|totals| Money::new(totals.balance, base));
    let group_id = overview.group.id.as_str().to_string();

    rsx! {
        div { class: "flex flex-col items-center gap-1 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-4 py-5 text-center",
            span { class: "text-sm text-floral-white-400", {t!("summary.total").to_string()} }
            MoneyText { amount: summary.total, class: "text-4xl font-semibold text-floral-white-50" }
            if summary.expense_count > 0 {
                if let Some(own) = own {
                    OwnBalance { balance: own, class: "mt-1" }
                }
            } else {
                p { class: "mt-2 text-sm text-floral-white-400", {t!("summary.no_expenses").to_string()} }
                Button {
                    class: "mt-3 w-full",
                    onclick: move |_| {
                        nav.push(Route::ExpenseNew {
                            group: group_id.clone(),
                            receipt: String::new(),
                            copy: String::new(),
                        });
                    },
                    Icon { icon: LdPlus, class: "h-5 w-5" }
                    {t!("timeline.add").to_string()}
                }
            }
            if summary.skipped_count > 0 {
                p { class: "mt-3 flex items-start gap-2 text-left text-sm text-pale-oak-300",
                    Icon { icon: LdTriangleAlert, class: "mt-0.5 h-4 w-4 shrink-0" }
                    {t!("summary.skipped", count = summary.skipped_count).to_string()}
                }
            }
        }
    }
}

/// Who owes whom, the two rankings and everyone's figures.
#[component]
fn Balances(overview: Overview) -> Element {
    let mut sheet = use_signal(|| None::<SettleDraft>);
    let summary = &overview.summary;
    let base = overview.group.base_currency;
    let members: BTreeSet<PersonId> = overview
        .members
        .iter()
        .map(|member| member.person.id.clone())
        .collect();
    let line = |person: &PersonId, amount: i64| PersonLine {
        id: person.clone(),
        person: overview.people.get(person).cloned(),
        former: !members.contains(person),
        amount: Money::new(amount, base),
    };
    let paid: Vec<PersonLine> = summary
        .paid_ranking()
        .iter()
        .map(|(person, amount)| line(person, *amount))
        .collect();
    let owes: Vec<PersonLine> = summary
        .balance_ranking()
        .iter()
        .map(|(person, amount)| line(person, *amount))
        .collect();
    let transfers = summary.transfers.clone();
    // Paid and consumed as idee.md 8.3 counts them, settlements included,
    // so that paid − consumed is the balance shown next to them.
    let per_person: Vec<(PersonLine, Money, Money)> = summary
        .people
        .iter()
        .map(|(person, totals)| {
            (
                line(person, totals.balance),
                Money::new(totals.paid.saturating_add(totals.settled_out), base),
                Money::new(totals.consumed.saturating_add(totals.settled_in), base),
            )
        })
        .collect();

    rsx! {
        CardSection { title: t!("summary.who_owes_whom").to_string(),
            if transfers.is_empty() {
                p { class: "px-4 py-4 text-base text-floral-white-300", {t!("summary.all_settled").to_string()} }
            }
            for (index, transfer) in transfers.iter().enumerate() {
                DebtRow {
                    key: "{index}",
                    from: overview.people.get(&transfer.from).cloned(),
                    to: overview.people.get(&transfer.to).cloned(),
                    amount: Money::new(transfer.amount_minor, base),
                    onclick: {
                        let draft = SettleDraft::from_debt(transfer, &overview.group);
                        move |_| sheet.set(Some(draft.clone()))
                    },
                }
            }
        }
        CardSection { title: t!("summary.paid_most").to_string(),
            for (index, line) in paid.into_iter().enumerate() {
                RankRow { key: "{line.id.as_str()}", rank: index + 1, line, signed: false }
            }
        }
        CardSection { title: t!("summary.owes_most").to_string(),
            for (index, line) in owes.into_iter().enumerate() {
                RankRow { key: "{line.id.as_str()}", rank: index + 1, line, signed: true }
            }
        }
        CardSection { title: t!("summary.per_person").to_string(),
            for (line, paid, consumed) in per_person {
                PersonFiguresRow { key: "{line.id.as_str()}", line, paid, consumed }
            }
        }
        if let Some(draft) = sheet() {
            SettleSheet {
                group: overview.group.clone(),
                choices: overview.choices.clone(),
                draft,
                on_close: move |_| sheet.set(None),
            }
        }
    }
}

/// A person with an amount, as the rankings and figures show them.
#[derive(Debug, Clone, PartialEq)]
struct PersonLine {
    id: PersonId,
    /// `None` if the person cannot be found at all.
    person: Option<Person>,
    /// Still counts with past expenses, but has left the group.
    former: bool,
    amount: Money,
}

impl PersonLine {
    fn name_and_color(&self) -> (String, String) {
        person_label(self.person.as_ref())
    }
}

/// Place, person and amount in a ranking (GRP-11, GRP-12).
#[component]
fn RankRow(rank: usize, line: PersonLine, signed: bool) -> Element {
    let (name, color) = line.name_and_color();
    // A signed amount takes its color from the balance.
    let amount_class = if signed {
        "text-base font-semibold"
    } else {
        "text-base font-semibold text-floral-white-100"
    };
    rsx! {
        div { class: "flex min-h-14 items-center gap-3 border-b border-jet-black-800 px-4 py-2 last:border-b-0",
            span { class: "w-5 shrink-0 text-right text-sm tabular-nums text-floral-white-500", "{rank}" }
            Avatar { name: name.clone(), color, size: AvatarSize::Sm }
            PersonName { name, former: line.former }
            MoneyText { amount: line.amount, signed, class: amount_class }
        }
    }
}

/// Paid, share and balance of one person (GRP-14).
#[component]
fn PersonFiguresRow(line: PersonLine, paid: Money, consumed: Money) -> Element {
    let (name, color) = line.name_and_color();
    let format = NumberFormat::current();
    // One line each: side by side they get cut off on narrow phones.
    let paid = t!("summary.paid", amount = format_money(paid, format)).to_string();
    let consumed = t!("summary.consumed", amount = format_money(consumed, format)).to_string();
    rsx! {
        div { class: "flex min-h-16 items-center gap-3 border-b border-jet-black-800 px-4 py-2 last:border-b-0",
            Avatar { name: name.clone(), color, size: AvatarSize::Md }
            span { class: "flex min-w-0 flex-1 flex-col",
                PersonName { name, former: line.former }
                span { class: "truncate text-sm tabular-nums text-floral-white-400", "{paid}" }
                span { class: "truncate text-sm tabular-nums text-floral-white-400", "{consumed}" }
            }
            MoneyText { amount: line.amount, signed: true, class: "text-base font-semibold" }
        }
    }
}

/// Name, with a note if the person has left the group.
#[component]
fn PersonName(name: String, former: bool) -> Element {
    rsx! {
        span { class: "flex min-w-0 flex-1 flex-col",
            span { class: "truncate text-base text-floral-white-50", "{name}" }
            if former {
                span { class: "truncate text-sm text-floral-white-500", {t!("summary.former_member").to_string()} }
            }
        }
    }
}

fn avatars(members: &[GroupMember]) -> Vec<AvatarEntry> {
    members
        .iter()
        .map(|member| AvatarEntry {
            name: member.person.name.clone(),
            color: member.person.color.clone(),
        })
        .collect()
}
