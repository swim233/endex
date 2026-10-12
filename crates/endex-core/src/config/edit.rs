//! Edits of `config.toml` that keep the user's comments, order and formatting.

use std::collections::HashSet;
use std::fmt;

use anyhow::{Context, Result};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

use super::{Source, tilde};

pub struct ConfigDoc(DocumentMut);

impl ConfigDoc {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(Self(text.parse()?))
    }

    /// Sets a dotted key such as `resources.threads.mid`, creating tables as needed. A replaced
    /// value keeps the comments around it.
    pub fn set(&mut self, key: &str, mut value: Value) -> Result<()> {
        let mut parts: Vec<&str> = key.split('.').collect();
        let last = parts.pop().context("empty key")?;
        let mut table = self.0.as_table_mut() as &mut dyn toml_edit::TableLike;
        for part in parts {
            table = table
                .entry(part)
                .or_insert_with(|| {
                    // No header of its own unless it gets keys: `[a.b]` instead of `[a]` + `[a.b]`.
                    let mut new = Table::new();
                    new.set_implicit(true);
                    Item::Table(new)
                })
                .as_table_like_mut()
                .with_context(|| format!("`{part}` in `{key}` is not a table"))?;
        }
        match table.get_mut(last) {
            Some(slot) => {
                if let Some(old) = slot.as_value() {
                    *value.decor_mut() = old.decor().clone();
                }
                *slot = Item::Value(value);
            }
            None => {
                table.insert(last, Item::Value(value));
            }
        }
        Ok(())
    }
}

impl ConfigDoc {
    /// Adds the top-level tables that `keys` need and the file lacks, copied with their
    /// comments and defaults from `template` (the default config), after the file's own tables
    /// and in the template's order.
    pub fn add_tables_from<'k>(
        &mut self,
        keys: impl IntoIterator<Item = &'k str>,
        template: &ConfigDoc,
    ) {
        let needed: HashSet<&str> = keys
            .into_iter()
            .filter_map(|key| key.split_once('.').map(|(table, _)| table))
            .collect();
        let root = self.0.as_table();
        let mut position = root
            .iter()
            .filter_map(|(_, item)| item.as_table().and_then(Table::position))
            .max()
            .unwrap_or(0);
        let missing: Vec<(String, Table)> = template
            .0
            .as_table()
            .iter()
            .filter(|(name, _)| needed.contains(name) && !root.contains_key(name))
            .filter_map(|(name, item)| Some((name.to_owned(), item.as_table()?.clone())))
            .collect();
        for (name, mut table) in missing {
            position += 1;
            table.set_position(Some(position));
            self.0.as_table_mut().insert(&name, Item::Table(table));
        }
    }
}

/// Comments of older default configs that are no longer true, and what replaces them.
const STALE_COMMENTS: &[(&str, &str)] = &[(
    "# Changing it requires `endex index --rebuild`.",
    "# Changing it, the model or OCR requires a rebuild of the index (settings window).",
)];

pub fn refresh_comments(text: &str) -> String {
    STALE_COMMENTS
        .iter()
        .fold(text.to_owned(), |text, (old, new)| text.replace(old, new))
}

impl fmt::Display for ConfigDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// One element per line, as people write longer lists by hand.
pub fn array(values: impl IntoIterator<Item = Value>) -> Value {
    let mut array = Array::new();
    for mut value in values {
        value.decor_mut().set_prefix("\n  ");
        array.push_formatted(value);
    }
    if !array.is_empty() {
        array.set_trailing_comma(true);
        array.set_trailing("\n");
    }
    Value::Array(array)
}

pub fn strings<S: AsRef<str>>(items: &[S]) -> Value {
    array(items.iter().map(|s| Value::from(s.as_ref())))
}

/// `{ path = "~/Notes", kind = "all" }` per source.
pub fn sources(sources: &[Source]) -> Value {
    array(sources.iter().map(|s| {
        let mut table = InlineTable::new();
        table.insert("path", Value::from(tilde(&s.path)));
        table.insert("kind", Value::from(s.kind.to_string()));
        table.fmt();
        Value::InlineTable(table)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, SourceKind};

    #[test]
    fn edits_keep_comments_and_layout() {
        let text = r#"# Endex configuration.
data_dir = "/tmp/endex"

[embedding]
# The C library.
library = "/lib.so"
model = "/m.litertlm"
tokenizer = "/t.json"

[index]
# Larger documents are skipped.
max_document_mb = 10  # MB

[resources]
threads = { light = 1, mid = 3, heavy = 6 }
"#;
        let mut doc = ConfigDoc::parse(text).unwrap();
        doc.set("index.max_document_mb", 20.into()).unwrap();
        doc.set("resources.threads.mid", 5.into()).unwrap();
        doc.set("log.usage", false.into()).unwrap();
        doc.set("exclude.rules", strings(&["*.log", "backup/"]))
            .unwrap();
        let source = Source {
            path: "/tmp/notes".into(),
            kind: SourceKind::Images,
        };
        doc.set("index.sources", sources(std::slice::from_ref(&source)))
            .unwrap();
        let out = doc.to_string();
        for kept in [
            "# Endex configuration.\ndata_dir",
            "# The C library.\nlibrary",
            "# Larger documents are skipped.\nmax_document_mb = 20  # MB\n",
            "threads = { light = 1, mid = 5, heavy = 6 }",
            "\n[log]\nusage = false\n",
        ] {
            assert!(out.contains(kept), "{kept:?} missing in:\n{out}");
        }
        let config = Config::parse(&out).unwrap();
        assert_eq!(config.index.max_document_mb, 20);
        assert_eq!(config.index.sources, [source]);
        assert_eq!(config.exclude.rules, ["*.log", "backup/"]);
        assert_eq!(config.resources.threads.mid, 5);
        assert_eq!(config.resources.threads.heavy, 6);
        assert!(config.exclude.gitignore);
    }
}
