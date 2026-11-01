//! Explicit UI catalogs. Document contents, file paths and diagnostic details are never translated.
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::{
    collections::BTreeMap,
    sync::{
        OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    Russian,
    French,
    German,
    Spanish,
    Italian,
}

impl Language {
    pub const ALL: [Self; 6] = [
        Self::English,
        Self::Russian,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Italian,
    ];
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
            Self::Italian => "Italiano",
        }
    }
    pub fn text(self, key: &str) -> &str {
        catalogs()[self as usize]
            .get(key)
            .or_else(|| catalogs()[0].get(key))
            .map(String::as_str)
            .unwrap_or(key)
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);
thread_local! {
    // A UI context keeps its own choice while worker-created native dialogs use the process choice.
    static UI_LANGUAGE: Cell<Option<Language>> = const { Cell::new(None) };
}
pub fn set_language(language: Language) {
    LANGUAGE.store(language as u8, Ordering::Relaxed);
    UI_LANGUAGE.set(Some(language));
}
pub fn language() -> Language {
    UI_LANGUAGE
        .get()
        .unwrap_or_else(|| Language::ALL[usize::from(LANGUAGE.load(Ordering::Relaxed)).min(5)])
}
pub fn tr(key: &str) -> &str {
    language().text(key)
}

fn catalogs() -> &'static [BTreeMap<String, String>; 6] {
    static CATALOGS: OnceLock<[BTreeMap<String, String>; 6]> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        [
            include_str!("../locales/en.json"),
            include_str!("../locales/ru.json"),
            include_str!("../locales/fr.json"),
            include_str!("../locales/de.json"),
            include_str!("../locales/es.json"),
            include_str!("../locales/it.json"),
        ]
        .map(|source| serde_json::from_str(source).expect("Embedded UI catalog is valid JSON"))
    })
}

/// Keep the message key and its values until display, so existing status text switches language too.
#[derive(Clone, Debug)]
pub enum Message {
    Text {
        key: &'static str,
        values: Vec<(&'static str, String)>,
    },
    Context {
        key: &'static str,
        name: &'static str,
        message: Box<Message>,
    },
    Detail(String),
}
impl Message {
    pub fn new(key: &'static str, values: &[(&'static str, String)]) -> Self {
        Self::Text {
            key,
            values: values.to_vec(),
        }
    }
    pub fn translated(&self) -> String {
        self.in_language(language())
    }
    pub fn wrap(key: &'static str, name: &'static str, message: Message) -> Self {
        Self::Context {
            key,
            name,
            message: Box::new(message),
        }
    }
    pub fn in_language(&self, language: Language) -> String {
        match self {
            Self::Detail(detail) => detail.clone(),
            Self::Context { key, name, message } => {
                Self::new(key, &[(name, message.in_language(language))]).in_language(language)
            }
            Self::Text { key, values } => {
                // Substitute only tokens from the template, once. A path containing braces stays intact.
                let template = language.text(key);
                let mut result = String::with_capacity(template.len());
                let mut rest = template;
                while let Some(start) = rest.find('{') {
                    result.push_str(&rest[..start]);
                    let Some(end) = rest[start..].find('}').map(|n| n + start) else {
                        result.push_str(&rest[start..]);
                        return result;
                    };
                    let name = &rest[start + 1..end];
                    if let Some((_, value)) = values.iter().find(|(key, _)| *key == name) {
                        result.push_str(value);
                    } else {
                        result.push_str(&rest[start..=end]);
                    }
                    rest = &rest[end + 1..];
                }
                result.push_str(rest);
                result
            }
        }
    }
    pub fn is(&self, key: &str) -> bool {
        matches!(self, Self::Text { key: own, .. } if *own == key)
    }
}
impl From<&'static str> for Message {
    fn from(key: &'static str) -> Self {
        Self::new(key, &[])
    }
}
impl From<String> for Message {
    fn from(detail: String) -> Self {
        Self::Detail(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn placeholders(text: &str) -> Vec<&str> {
        let mut values: Vec<_> = text
            .split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name))
            .collect();
        values.sort_unstable();
        values
    }
    #[test]
    fn catalogs_are_complete_and_preserve_named_values() {
        let catalogs = catalogs();
        assert!(catalogs[0].len() > 100);
        for catalog in catalogs {
            assert_eq!(catalog.len(), catalogs[0].len());
            for (key, english) in &catalogs[0] {
                let translated = catalog
                    .get(key)
                    .unwrap_or_else(|| panic!("Missing message: {key}"));
                assert!(!translated.trim().is_empty(), "Empty message: {key}");
                assert_eq!(
                    placeholders(english),
                    placeholders(translated),
                    "Changed values: {key}"
                );
            }
        }
    }
    #[test]
    fn substitution_keeps_paths_and_retranslates_existing_messages() {
        let path = "C:/資料/{number}/rapport.pdf";
        let message = Message::new("Saved {path}", &[("path", path.into())]);
        assert_eq!(
            message.in_language(Language::English),
            format!("Saved {path}")
        );
        assert_eq!(
            message.in_language(Language::French),
            format!("Enregistré : {path}")
        );
        assert_eq!(
            Language::Russian.text("unknown diagnostic"),
            "unknown diagnostic"
        );
    }
}
