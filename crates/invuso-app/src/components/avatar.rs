use dioxus::prelude::*;

/// Size of an [`Avatar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AvatarSize {
    /// In stacks and dense rows.
    Sm,
    /// In list rows.
    #[default]
    Md,
    /// On detail screens.
    Lg,
}

/// Round badge with a person's initials on their color (UI-07).
/// `color` is a design-token scale name (`preferences::PERSON_COLORS`).
#[component]
pub fn Avatar(name: String, color: String, #[props(default)] size: AvatarSize) -> Element {
    let size = match size {
        AvatarSize::Sm => "h-8 w-8 text-xs",
        AvatarSize::Md => "h-11 w-11 text-base",
        AvatarSize::Lg => "h-20 w-20 text-2xl",
    };
    let colors = color_classes(&color);

    rsx! {
        span {
            class: "flex shrink-0 items-center justify-center rounded-full font-semibold select-none {size} {colors}",
            aria_hidden: "true",
            "{initials(&name)}"
        }
    }
}

/// Person shown in an [`AvatarStack`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarEntry {
    pub name: String,
    pub color: String,
}

/// Overlapping small avatars, e.g. the members of a group (UI-07). Shows at
/// most `max` avatars and a "+n" badge for the rest.
#[component]
pub fn AvatarStack(people: Vec<AvatarEntry>, #[props(default = 4)] max: usize) -> Element {
    let hidden = people.len().saturating_sub(max);
    let names = people
        .iter()
        .map(|person| person.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    rsx! {
        div { class: "flex items-center -space-x-2", role: "img", aria_label: "{names}",
            for (index, person) in people.iter().take(max).enumerate() {
                span { key: "{index}", class: "rounded-full ring-2 ring-jet-black-950",
                    Avatar { name: person.name.clone(), color: person.color.clone(), size: AvatarSize::Sm }
                }
            }
            if hidden > 0 {
                span { class: "flex h-8 w-8 items-center justify-center rounded-full bg-jet-black-800 text-xs font-semibold text-floral-white-200 ring-2 ring-jet-black-950",
                    "+{hidden}"
                }
            }
        }
    }
}

/// Background and text classes per person color. Written out in full so
/// Tailwind finds them when scanning the sources.
pub fn color_classes(color: &str) -> &'static str {
    match color {
        "muted-teal" => "bg-muted-teal-700 text-muted-teal-50",
        "pale-oak" => "bg-pale-oak-700 text-pale-oak-50",
        "thistle" => "bg-thistle-700 text-thistle-50",
        "dusty-grape" => "bg-dusty-grape-600 text-dusty-grape-50",
        "ash-grey" => "bg-ash-grey-700 text-ash-grey-50",
        "slate-grey" => "bg-slate-grey-700 text-slate-grey-50",
        // "cerulean" and colors this version does not know.
        _ => "bg-cerulean-700 text-cerulean-50",
    }
}

/// Up to two initials: first letters of the first and last word.
fn initials(name: &str) -> String {
    let mut words = name.split_whitespace();
    let first = words.next().and_then(|word| word.chars().next());
    let last = words.next_back().and_then(|word| word.chars().next());
    first
        .into_iter()
        .chain(last)
        .flat_map(char::to_uppercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_from_first_and_last_word() {
        assert_eq!(initials("Konstantin"), "K");
        assert_eq!(initials("anna maria schmidt"), "AS");
        assert_eq!(initials("  Ben   Ali "), "BA");
        assert_eq!(initials("山田 太郎"), "山太");
        assert_eq!(initials(""), "");
    }
}
