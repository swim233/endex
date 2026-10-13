//! `GetSettings` and `ApplySettings`: the config as flat dotted keys (`resources.profile`),
//! checked per field and written back into config.toml with the user's comments kept.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use endex_core::config::edit::{self, ConfigDoc, refresh_comments};
use endex_core::config::{
    AppearanceConfig, Config, Source, SourceKind, default_toml, enum_name, expand_home, tilde,
};
use endex_core::exclude::{BUILTIN, SENSITIVE, check_rule};
use toml_edit::Value as Toml;
use zbus::zvariant::{OwnedValue, Value};

pub type Dict = HashMap<String, OwnedValue>;
/// Field-level problems, by key; `""` for the file as a whole. Messages are for the user.
pub type Errors = BTreeMap<String, String>;

enum Type {
    Bool,
    Number(u32, u32),
    /// A number in halves, such as a font size.
    Halves(f64, f64),
    Choice(&'static [&'static str]),
    /// `default` or `#rrggbb`.
    Color,
    Strings,
    Path,
    Sources,
}

const DEVICES: &[&str] = &["cpu", "gpu"];
const PALETTES: &[&str] = &["plasma", "proto"];

const KEYS: &[(&str, Type)] = &[
    ("index.sources", Type::Sources),
    ("index.declined", Type::Strings),
    ("index.max_document_mb", Type::Number(1, 1024)),
    ("exclude.gitignore", Type::Bool),
    ("exclude.builtin", Type::Bool),
    ("exclude.sensitive", Type::Bool),
    ("exclude.hidden", Type::Bool),
    ("exclude.rules", Type::Strings),
    ("update.scan", Type::Bool),
    (
        "update.scan_interval",
        Type::Choice(&["startup", "hourly", "daily"]),
    ),
    ("update.watch", Type::Bool),
    ("embedding.model", Type::Path),
    (
        "embedding.prefix_style",
        Type::Choice(&["litert", "model_card"]),
    ),
    ("ocr.enabled", Type::Bool),
    ("devices.query", Type::Choice(DEVICES)),
    ("devices.bulk", Type::Choice(DEVICES)),
    ("devices.incremental", Type::Choice(DEVICES)),
    ("resources.pause_on_battery", Type::Bool),
    ("resources.low_priority", Type::Bool),
    (
        "resources.profile",
        Type::Choice(&["light", "mid", "heavy"]),
    ),
    ("resources.threads.light", Type::Number(1, 64)),
    ("resources.threads.mid", Type::Number(1, 64)),
    ("resources.threads.heavy", Type::Number(1, 64)),
    ("resources.gpu_idle_release_min", Type::Number(0, 1440)),
    ("log.usage", Type::Bool),
    (
        "appearance.preset",
        Type::Choice(&["plasma", "proto", "glass", "custom"]),
    ),
    ("appearance.base", Type::Choice(PALETTES)),
    (
        "appearance.scheme",
        Type::Choice(&["system", "light", "dark"]),
    ),
    ("appearance.accent", Type::Color),
    (
        "appearance.font",
        Type::Choice(&[
            "system",
            "noto_sans",
            "inter",
            "noto_serif",
            "jetbrains_mono",
        ]),
    ),
    ("appearance.font_size", Type::Halves(8.0, 14.0)),
    (
        "appearance.density",
        Type::Choice(&["comfortable", "compact"]),
    ),
    ("appearance.radius", Type::Number(0, 16)),
    (
        "appearance.popup_size",
        Type::Choice(&["default", "small", "screen", "custom"]),
    ),
    ("appearance.screen_percent", Type::Number(40, 90)),
    ("appearance.popup_width", Type::Number(480, 1600)),
    ("appearance.popup_height", Type::Number(320, 1000)),
    ("appearance.translucency", Type::Number(0, 60)),
    ("appearance.blur", Type::Bool),
];

/// Every settable key, plus read-only ones: `index.suggested` (folders to offer that are
/// neither sources nor declined), `exclude.builtin_list`, `exclude.sensitive_list` and
/// `config_path`.
pub fn get(config: &Config, config_path: &Path) -> Dict {
    let mut d = Dict::new();
    let sources: Vec<Dict> = config
        .index
        .sources
        .iter()
        .map(|s| {
            let mut source = Dict::new();
            put(&mut source, "path", tilde(&s.path));
            put(&mut source, "kind", s.kind.to_string());
            source
        })
        .collect();
    put(&mut d, "index.sources", sources);
    put(&mut d, "index.declined", paths(&config.index.declined));
    put(
        &mut d,
        "index.max_document_mb",
        config.index.max_document_mb as u32,
    );
    let e = &config.exclude;
    put(&mut d, "exclude.gitignore", e.gitignore);
    put(&mut d, "exclude.builtin", e.builtin);
    put(&mut d, "exclude.sensitive", e.sensitive);
    put(&mut d, "exclude.hidden", e.hidden);
    put(&mut d, "exclude.rules", e.rules.clone());
    let u = &config.update;
    put(&mut d, "update.scan", u.scan);
    put(&mut d, "update.scan_interval", enum_name(&u.scan_interval));
    put(&mut d, "update.watch", u.watch);
    put(&mut d, "embedding.model", tilde(&config.embedding.model));
    put(
        &mut d,
        "embedding.prefix_style",
        config.embedding.prefix_style.as_str(),
    );
    put(&mut d, "ocr.enabled", config.ocr.enabled);
    put(&mut d, "devices.query", config.devices.query.as_str());
    put(&mut d, "devices.bulk", config.devices.bulk.as_str());
    put(
        &mut d,
        "devices.incremental",
        config.devices.incremental.as_str(),
    );
    let r = &config.resources;
    put(&mut d, "resources.pause_on_battery", r.pause_on_battery);
    put(&mut d, "resources.low_priority", r.low_priority);
    put(&mut d, "resources.profile", enum_name(&r.profile));
    put(&mut d, "resources.threads.light", r.threads.light);
    put(&mut d, "resources.threads.mid", r.threads.mid);
    put(&mut d, "resources.threads.heavy", r.threads.heavy);
    put(
        &mut d,
        "resources.gpu_idle_release_min",
        r.gpu_idle_release_min,
    );
    put(&mut d, "log.usage", config.log.usage);
    let a = &config.appearance;
    put(&mut d, "appearance.preset", a.preset.as_str());
    put(&mut d, "appearance.base", a.base.as_str());
    put(&mut d, "appearance.scheme", a.scheme.as_str());
    put(&mut d, "appearance.accent", a.accent.as_str());
    put(&mut d, "appearance.font", a.font.as_str());
    put(&mut d, "appearance.font_size", a.font_size);
    put(&mut d, "appearance.density", a.density.as_str());
    put(&mut d, "appearance.radius", a.radius);
    put(&mut d, "appearance.popup_size", a.popup_size.as_str());
    put(&mut d, "appearance.screen_percent", a.screen_percent);
    put(&mut d, "appearance.popup_width", a.popup_width);
    put(&mut d, "appearance.popup_height", a.popup_height);
    put(&mut d, "appearance.translucency", a.translucency);
    put(&mut d, "appearance.blur", a.blur);

    let taken = |dir: &Path| {
        config
            .index
            .sources
            .iter()
            .any(|s| s.owner() == dir || s.root() == dir)
            || config.index.declined.iter().any(|p| expand_home(p) == dir)
    };
    let suggested: Vec<PathBuf> = [dirs::document_dir(), dirs::picture_dir()]
        .into_iter()
        .flatten()
        .filter(|dir| !taken(dir))
        .collect();
    put(&mut d, "index.suggested", paths(&suggested));
    put(&mut d, "exclude.builtin_list", BUILTIN.to_vec());
    put(&mut d, "exclude.sensitive_list", SENSITIVE.to_vec());
    put(&mut d, "config_path", config_path.display().to_string());
    d
}

/// The new file text and the config it loads as, or what is wrong with `changes`. Tables the
/// file lacks are added with the default config's comments.
pub fn apply(text: &str, changes: &Dict, current: &Config) -> Result<(String, Config), Errors> {
    let file_error = |message: String| Errors::from([(String::new(), message)]);
    let mut doc = ConfigDoc::parse(text)
        .map_err(|e| file_error(format!("config.toml 不是有效的 TOML，请先修好它：{e}")))?;
    let template = ConfigDoc::parse(&default_toml()).expect("the default config parses");
    doc.add_tables_from(changes.keys().map(String::as_str), &template);
    let mut errors = Errors::new();
    for (key, value) in changes {
        let result = to_toml(key, value).and_then(|value| {
            doc.set(key, value)
                .map_err(|e| format!("无法写入 config.toml：{e:#}"))
        });
        if let Err(message) = result {
            errors.insert(key.clone(), message);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let text = refresh_comments(&doc.to_string());
    let config = Config::parse(&text).map_err(|e| file_error(format!("{e:#}")))?;
    let mut errors = validate(&config, current);
    if changes.contains_key("embedding.model") {
        let model = &config.embedding.model;
        if !model.is_file() {
            errors.insert("embedding.model".into(), "找不到这个文件".into());
        } else if model.extension().is_none_or(|e| e != "litertlm") {
            errors.insert("embedding.model".into(), "不是 .litertlm 模型文件".into());
        }
    }
    if errors.is_empty() {
        Ok((text, config))
    } else {
        Err(errors)
    }
}

/// What the file format cannot express: sources (absolute, not inside the data dir, no
/// overlaps, and new ones must exist) and rules. Used for the settings window and for edits by
/// hand. A source that already was configured may be missing (an unplugged drive).
pub fn validate(config: &Config, previous: &Config) -> Errors {
    let mut errors = Errors::new();
    let roots: Vec<PathBuf> = config.index.sources.iter().map(Source::root).collect();
    let data_dir = fs::canonicalize(&config.data_dir).unwrap_or_else(|_| config.data_dir.clone());
    for (i, (source, root)) in config.index.sources.iter().zip(&roots).enumerate() {
        let new = !previous
            .index
            .sources
            .iter()
            .any(|p| p.owner() == source.owner());
        let message = if !source.owner().is_absolute() {
            Some("请填写绝对路径（可以用 ~/ 开头）".to_owned())
        } else if new && !root.exists() {
            Some("文件夹不存在".to_owned())
        } else if new && !root.is_dir() {
            Some("不是文件夹".to_owned())
        } else if root.starts_with(&data_dir) || root.starts_with(&config.data_dir) {
            Some("不能索引 Endex 自己的数据目录".to_owned())
        } else {
            roots
                .iter()
                .enumerate()
                .find(|&(j, other)| j != i && (root.starts_with(other) || other.starts_with(root)))
                .map(|(_, other)| format!("和 {} 重叠，同一个文件只能属于一个目录", tilde(other)))
        };
        if let Some(message) = message {
            errors.insert(format!("index.sources.{i}"), message);
        }
    }
    for (i, rule) in config.exclude.rules.iter().enumerate() {
        if let Err(e) = check_rule(rule) {
            errors.insert(format!("exclude.rules.{i}"), format!("规则写法有误：{e:#}"));
        }
    }
    errors.extend(check_appearance(&config.appearance));
    errors
}

/// `[appearance]` as written by hand gets the checks of ApplySettings.
fn check_appearance(a: &AppearanceConfig) -> Errors {
    let values: [(&str, Value); 14] = [
        ("preset", a.preset.as_str().into()),
        ("base", a.base.as_str().into()),
        ("scheme", a.scheme.as_str().into()),
        ("accent", a.accent.as_str().into()),
        ("font", a.font.as_str().into()),
        ("font_size", a.font_size.into()),
        ("density", a.density.as_str().into()),
        ("radius", a.radius.into()),
        ("popup_size", a.popup_size.as_str().into()),
        ("screen_percent", a.screen_percent.into()),
        ("popup_width", a.popup_width.into()),
        ("popup_height", a.popup_height.into()),
        ("translucency", a.translucency.into()),
        ("blur", a.blur.into()),
    ];
    values
        .iter()
        .filter_map(|(name, value)| {
            let key = format!("appearance.{name}");
            to_toml(&key, value).err().map(|message| (key, message))
        })
        .collect()
}

fn to_toml(key: &str, value: &Value) -> Result<Toml, String> {
    let value = unwrap(value);
    let Some((_, ty)) = KEYS.iter().find(|(k, _)| *k == key) else {
        return Err(if get_only(key) {
            "只读，不能修改".to_owned()
        } else {
            "未知的设置项".to_owned()
        });
    };
    match ty {
        Type::Bool => match value {
            Value::Bool(b) => Ok(Toml::from(*b)),
            _ => Err("应为开或关（布尔值）".into()),
        },
        Type::Number(min, max) => match integer(value) {
            Some(n) if (i64::from(*min)..=i64::from(*max)).contains(&n) => Ok(Toml::from(n)),
            Some(_) => Err(format!("应在 {min} 到 {max} 之间")),
            None => Err("应为整数".into()),
        },
        Type::Halves(min, max) => {
            let n = match value {
                Value::F64(n) => Some(*n),
                _ => integer(value).map(|n| n as f64),
            };
            match n {
                Some(n) if (*min..=*max).contains(&n) && (n * 2.0).fract() == 0.0 => {
                    Ok(Toml::from(n))
                }
                Some(_) => Err(format!("应在 {min} 到 {max} 之间，以 0.5 为单位")),
                None => Err("应为数字".into()),
            }
        }
        Type::Choice(choices) => match value {
            Value::Str(s) if choices.contains(&s.as_str()) => Ok(Toml::from(s.as_str())),
            _ => Err(format!("应为 {} 之一", choices.join("、"))),
        },
        Type::Color => match value {
            Value::Str(s) if s.as_str() == "default" => Ok(Toml::from("default")),
            Value::Str(s)
                if s.len() == 7
                    && s.starts_with('#')
                    && s[1..].chars().all(|c| c.is_ascii_hexdigit()) =>
            {
                Ok(Toml::from(s.to_ascii_lowercase()))
            }
            _ => Err("应为 default 或 #rrggbb 形式的颜色".into()),
        },
        Type::Path => match value {
            Value::Str(s) if !s.trim().is_empty() => Ok(Toml::from(s.trim())),
            _ => Err("应为文件路径".into()),
        },
        Type::Strings => {
            let items = strings(value).ok_or("应为字符串列表")?;
            let items: Vec<String> = items
                .into_iter()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            Ok(edit::strings(&items))
        }
        Type::Sources => {
            let Value::Array(items) = value else {
                return Err("应为目录列表".into());
            };
            let mut sources = Vec::new();
            for item in items.inner() {
                let Value::Dict(dict) = unwrap(item) else {
                    return Err("每个目录应为 {path, kind}".into());
                };
                let field = |name: &str| {
                    dict.iter().find_map(|(k, v)| match (unwrap(k), unwrap(v)) {
                        (Value::Str(k), Value::Str(v)) if k.as_str() == name => {
                            Some(v.as_str().trim().to_owned())
                        }
                        _ => None,
                    })
                };
                let path = field("path")
                    .filter(|p| !p.is_empty())
                    .ok_or("目录缺少 path")?;
                let kind: SourceKind = match field("kind") {
                    Some(kind) => kind
                        .parse()
                        .map_err(|_| "kind 应为 all、docs 或 images".to_owned())?,
                    None => SourceKind::All,
                };
                sources.push(Source {
                    path: path.into(),
                    kind,
                });
            }
            Ok(edit::sources(&sources))
        }
    }
}

fn get_only(key: &str) -> bool {
    matches!(
        key,
        "index.suggested" | "exclude.builtin_list" | "exclude.sensitive_list" | "config_path"
    )
}

fn unwrap<'a>(mut value: &'a Value<'a>) -> &'a Value<'a> {
    while let Value::Value(inner) = value {
        value = inner;
    }
    value
}

fn integer(value: &Value) -> Option<i64> {
    Some(match value {
        Value::U8(n) => (*n).into(),
        Value::U16(n) => (*n).into(),
        Value::I16(n) => (*n).into(),
        Value::U32(n) => (*n).into(),
        Value::I32(n) => (*n).into(),
        Value::I64(n) => *n,
        Value::U64(n) => i64::try_from(*n).ok()?,
        _ => return None,
    })
}

fn strings(value: &Value) -> Option<Vec<String>> {
    let Value::Array(items) = value else {
        return None;
    };
    items
        .inner()
        .iter()
        .map(|item| match unwrap(item) {
            Value::Str(s) => Some(s.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

fn paths(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| tilde(p)).collect()
}

fn put<'a>(d: &mut Dict, key: &str, value: impl Into<Value<'a>>) {
    let value = OwnedValue::try_from(value.into()).expect("no file descriptors");
    d.insert(key.to_owned(), value);
}
