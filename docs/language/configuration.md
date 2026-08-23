# Slug Configuration

## Purpose

Slug configuration is an immutable key-value store created by the runtime before any Slug source is evaluated. Programs
read it through the builtin
`cfg(key, default)`. This keeps deployment choices outside source code while making every setting's fallback visible
where it is used.

This page is the operational companion to the [Language Specification](language-specification.md)
and [Runtime Requirements](runtime-requirements.md). It defines the current portable configuration contract for a
clean-room implementation.

## Reading configuration

```slug
val port = cfg("port", 8080)
val address = cfg("address", "0.0.0.0")
val enabled = cfg("feature.enabled", false)
```

`cfg` has exactly two arguments:

1. `key` MUST be a string.
2. `default` is required and is returned when no configured value exists.

A configured value is selected once at runtime startup. Changing a TOML file or environment variable after evaluation
begins does not change the value returned by later `cfg` calls.

## Namespaces

A key containing `.` is absolute:

```slug
cfg("slug.web.server.port", 8080)
```

A key without `.` is local to the module that executes the call. The runtime prefixes it with that module's
fully-qualified name:

```slug
// In module slug.web.server:
cfg("port", 8080) // reads slug.web.server.port
```

This rule applies independently to imported modules. A library can use a short local key without accidentally reading a
same-named setting for its importer. Use an absolute key only when a module deliberately shares configuration with
another module.

## Sources and precedence

The runtime merges these sources at startup. Later sources replace a value from earlier sources with the same
fully-qualified key.

| Precedence | Source            | Location or form                        |
|-----------:|-------------------|-----------------------------------------|
|     Lowest | Library TOML      | `$SLUG_HOME/lib/slug.toml`              |
|            | Project TOML      | `slug.toml` in the selected module root |
|            | Environment       | `SLUG__...` variables                   |
|    Highest | Program arguments | options after the entry program         |
|   Fallback | Source code       | second argument to `cfg`                |

Both TOML files are optional. An unavailable or malformed optional TOML file contributes no values and does not expose a
host parsing failure to the Slug program.

### TOML

TOML tables flatten to dot-separated keys. For a server module:

```toml
[slug.web.server]
address = "127.0.0.1"
port = 3000

[feature]
enabled = true
```

The file supplies `slug.web.server.address`, `slug.web.server.port`, and
`feature.enabled`. The project TOML overrides a colliding library TOML key.

### Environment variables

Only variables whose names start with `SLUG__` participate. Remove that prefix and replace every `__` with `.`:

```text
SLUG__slug__web__server__port=3001
SLUG__feature__enabled=true
```

Those variables override both TOML files. Environment values begin as strings;
`cfg` converts a string to a number or boolean when its fallback is respectively a number or boolean. If conversion
fails, the original string is returned.

### Program arguments

Options following the entry program are part of Slug's argument list and may be used as configuration. Supported forms
are:

```text
slug server.slug --slug.web.server.port=3002
slug server.slug --slug.web.server.port 3002
slug server.slug --feature.enabled
slug server.slug -v
```

A valueless option has the string value `"true"`. Repeating an option creates a list of its values. A dotted key is
absolute. A non-dotted key is entry-module sugar, so `slug server.slug --port 3002` sets `server.port`.

`argv()` returns every argument following the program name. `argm()` returns a map with `"options"` and `"positional"`
entries using the same option parser. Use `--` to stop option parsing and treat all remaining arguments as positional.

## Values and conversion

TOML scalar and array values retain their corresponding Slug number, boolean, string, or list shape. A string from the
environment or command line is converted according to the fallback passed to `cfg`:

| Fallback shape                   | String value behavior                   |
|----------------------------------|-----------------------------------------|
| `num`                            | Parse as a decimal number when possible |
| `bool`                           | Parse as a boolean when possible        |
| `list`                           | Wrap one string as a one-element list   |
| Any other value, including `nil` | Return the string unchanged             |

The fallback is a value contract, not a schema declaration. Configuration does not perform arbitrary coercion, validate
unknown keys, or provide mutable updates. Modules that need richer validation should validate the value returned by
`cfg` themselves.

## Complete example

Given this project `slug.toml`:

```toml
[server]
port = 3000

[feature]
enabled = false
```

and this entry module `server.slug`:

```slug
val port = cfg("port", 8080)
val enabled = cfg("feature.enabled", true)
println(port, enabled)
```

`slug server.slug` prints `3000 false`. Running
`slug server.slug --port 3002 --feature.enabled=true` prints `3002 true`. The short `port` option is scoped to `server`,
while the dotted feature key is absolute.

## Library configuration

A library should use local keys for settings owned by that module and provide a usable fallback. For example,
`slug.web.server` can read
`cfg("port", 8080)`, which resolves to `slug.web.server.port`. Library pages list their module-specific settings and
defaults. An application may override a library setting in TOML, through `SLUG__` environment variables, or with an
absolute command-line key.
