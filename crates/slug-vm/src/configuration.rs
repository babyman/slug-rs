use std::{collections::HashMap, fs, path::Path};

use crate::Value;

/// A value retained by the immutable runtime configuration store.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigurationValue {
    /// A typed value read from TOML.
    Value(Value),
    /// An unconverted environment or command-line value.
    Text(String),
    /// Repeated command-line option values.
    TextList(Vec<String>),
}

/// Immutable configuration collected before any Slug source executes.
#[derive(Clone, Debug, Default)]
pub struct Configuration {
    values: HashMap<String, ConfigurationValue>,
    arguments: Vec<String>,
    entry_module: String,
}

impl Configuration {
    /// Collects configuration using the portable source-precedence order.
    ///
    /// `slug_home` supplies the optional `$SLUG_HOME` directory; its library
    /// configuration is read from `lib/slug.toml`. `arguments` are the values
    /// after the entry program name, and `entry_module` scopes undotted options.
    #[must_use]
    pub fn load(
        module_root: &Path,
        slug_home: Option<&Path>,
        environment: impl IntoIterator<Item = (String, String)>,
        arguments: &[String],
        entry_module: &str,
    ) -> Self {
        let mut values = HashMap::new();
        if let Some(slug_home) = slug_home {
            merge_toml(&mut values, &slug_home.join("lib/slug.toml"));
        }
        merge_toml(&mut values, &module_root.join("slug.toml"));
        for (name, value) in environment {
            if let Some(key) = name.strip_prefix("SLUG__") {
                values.insert(key.replace("__", "."), ConfigurationValue::Text(value));
            }
        }
        for (key, value) in parse_arguments(arguments, entry_module).options {
            values.insert(key, value);
        }
        Self {
            values,
            arguments: arguments.to_vec(),
            entry_module: entry_module.into(),
        }
    }

    /// Returns the selected value for one fully-qualified configuration key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&ConfigurationValue> {
        self.values.get(key)
    }

    /// Resolves a configured value using the supplied Slug fallback's shape.
    #[must_use]
    pub fn resolve(&self, key: &str, fallback: &Value) -> Value {
        self.values
            .get(key)
            .map_or_else(|| fallback.clone(), |value| resolve_value(value, fallback))
    }

    /// Returns the unmodified program arguments after the entry program name.
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// Returns parsed program options and positional arguments as Slug values.
    #[must_use]
    pub fn argument_map(&self) -> Value {
        let parsed = parse_arguments(&self.arguments, &self.entry_module);
        let options = parsed
            .options
            .into_iter()
            .map(|(key, value)| (Value::string(key), stored_value(value)))
            .collect::<Vec<_>>();
        Value::Map(
            vec![
                (Value::string("options"), Value::Map(options.into())),
                (
                    Value::string("positional"),
                    Value::List(
                        parsed
                            .positional
                            .into_iter()
                            .map(Value::string)
                            .collect::<Vec<_>>()
                            .into(),
                    ),
                ),
            ]
            .into(),
        )
    }
}

fn resolve_value(value: &ConfigurationValue, fallback: &Value) -> Value {
    match value {
        ConfigurationValue::Value(value) => value.clone(),
        ConfigurationValue::Text(value) => convert_text(value, fallback),
        ConfigurationValue::TextList(values) => Value::List(
            values
                .iter()
                .map(|value| Value::string(value.as_str()))
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}

fn stored_value(value: ConfigurationValue) -> Value {
    match value {
        ConfigurationValue::Value(value) => value,
        ConfigurationValue::Text(value) => Value::string(value),
        ConfigurationValue::TextList(values) => Value::List(
            values
                .into_iter()
                .map(Value::string)
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}

fn convert_text(value: &str, fallback: &Value) -> Value {
    match fallback {
        Value::Int(_) => value
            .parse::<i64>()
            .map_or_else(|_| Value::string(value), Value::Int),
        Value::Float(_) => value
            .parse::<f64>()
            .map_or_else(|_| Value::string(value), Value::Float),
        Value::Bool(_) => value
            .parse::<bool>()
            .map_or_else(|_| Value::string(value), Value::Bool),
        Value::List(_) => Value::List(vec![Value::string(value)].into()),
        _ => Value::string(value),
    }
}

fn merge_toml(values: &mut HashMap<String, ConfigurationValue>, path: &Path) {
    let Ok(source) = fs::read_to_string(path) else {
        return;
    };
    let Ok(document) = source.parse::<toml::Value>() else {
        return;
    };
    flatten_toml(values, "", &document);
}

fn flatten_toml(
    values: &mut HashMap<String, ConfigurationValue>,
    prefix: &str,
    value: &toml::Value,
) {
    match value {
        toml::Value::Table(entries) => {
            for (name, value) in entries {
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                flatten_toml(values, &key, value);
            }
        }
        _ => {
            if let Some(value) = toml_value(value) {
                values.insert(prefix.into(), ConfigurationValue::Value(value));
            }
        }
    }
}

fn toml_value(value: &toml::Value) -> Option<Value> {
    match value {
        toml::Value::String(value) => Some(Value::string(value.as_str())),
        toml::Value::Integer(value) => Some(Value::Int(*value)),
        toml::Value::Float(value) => Some(Value::Float(*value)),
        toml::Value::Boolean(value) => Some(Value::Bool(*value)),
        toml::Value::Array(values) => values
            .iter()
            .map(toml_value)
            .collect::<Option<Vec<_>>>()
            .map(|values| Value::List(values.into())),
        toml::Value::Datetime(_) | toml::Value::Table(_) => None,
    }
}

struct ParsedArguments {
    options: HashMap<String, ConfigurationValue>,
    positional: Vec<String>,
}

fn parse_arguments(arguments: &[String], entry_module: &str) -> ParsedArguments {
    let mut options = HashMap::<String, Vec<String>>::new();
    let mut positional = Vec::new();
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        if argument == "--" {
            positional.extend(arguments[index + 1..].iter().cloned());
            break;
        }
        let Some((name, inline)) = option_name(argument) else {
            positional.push(argument.clone());
            index += 1;
            continue;
        };
        let value = if let Some(value) = inline {
            value.into()
        } else if let Some(value) = arguments
            .get(index + 1)
            .filter(|value| !value.starts_with('-'))
        {
            index += 1;
            value.clone()
        } else {
            "true".into()
        };
        let key = if name.contains('.') || entry_module.is_empty() {
            name.into()
        } else {
            format!("{entry_module}.{name}")
        };
        options.entry(key).or_default().push(value);
        index += 1;
    }
    let options = options
        .into_iter()
        .map(|(key, values)| {
            let value = match values.as_slice() {
                [value] => ConfigurationValue::Text(value.clone()),
                _ => ConfigurationValue::TextList(values),
            };
            (key, value)
        })
        .collect();
    ParsedArguments {
        options,
        positional,
    }
}

fn option_name(argument: &str) -> Option<(&str, Option<&str>)> {
    let name = argument
        .strip_prefix("--")
        .or_else(|| argument.strip_prefix('-'))?;
    if name.is_empty() {
        return None;
    }
    let (name, value) = name
        .split_once('=')
        .map_or((name, None), |(name, value)| (name, Some(value)));
    (!name.is_empty()).then_some((name, value))
}
