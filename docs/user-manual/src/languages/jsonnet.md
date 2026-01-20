# Jsonnet Support

Turnkey provides Jsonnet support for generating JSON configuration files with Buck2 integration. Jsonnet is a data templating language that extends JSON with variables, functions, and imports.

## Setup

Add to `toolchain.toml`:

```toml
[toolchains]
jsonnet = {}
```

Turnkey uses [jrsonnet](https://github.com/CertainLach/jrsonnet), a fast Rust implementation of Jsonnet.

## Project Structure

```
my-project/
├── config/
│   ├── base.libsonnet       # Shared configuration
│   ├── dev.jsonnet          # Development config
│   ├── prod.jsonnet         # Production config
│   └── rules.star
```

## Build Rules

### jsonnet_library

Compile Jsonnet files to JSON:

```python
load("@prelude//jsonnet:jsonnet.bzl", "jsonnet_library")

jsonnet_library(
    name = "config-dev",
    srcs = ["dev.jsonnet"],
    deps = [":base"],  # Dependencies on other jsonnet_library targets
    ext_strs = {
        "env": "development",
        "region": "us-west-2",
    },
)
```

### Attributes

| Attribute | Description |
|-----------|-------------|
| `srcs` | Jsonnet source files (first file is entry point) |
| `deps` | Dependencies on other `jsonnet_library` targets |
| `out` | Output filename (defaults to `<src>.json`) |
| `ext_strs` | External string variables (`--ext-str key=value`) |
| `ext_codes` | External code variables (`--ext-code key=value`) |
| `tla_strs` | Top-level argument strings (`--tla-str key=value`) |
| `tla_codes` | Top-level argument code (`--tla-code key=value`) |

## Example

### base.libsonnet

```jsonnet
{
  // Shared configuration
  appName: 'my-app',
  version: '1.0.0',

  // Environment-specific overrides
  envConfig(env):: {
    development: {
      logLevel: 'debug',
      replicas: 1,
    },
    production: {
      logLevel: 'warn',
      replicas: 3,
    },
  }[env],
}
```

### dev.jsonnet

```jsonnet
local base = import 'base.libsonnet';
local env = std.extVar('env');

base {
  environment: env,
  config: base.envConfig(env),
}
```

### rules.star

```python
load("@prelude//jsonnet:jsonnet.bzl", "jsonnet_library")

# Shared library
jsonnet_library(
    name = "base",
    srcs = ["base.libsonnet"],
)

# Development config
jsonnet_library(
    name = "config-dev",
    srcs = ["dev.jsonnet"],
    deps = [":base"],
    ext_strs = {"env": "development"},
)

# Production config
jsonnet_library(
    name = "config-prod",
    srcs = ["dev.jsonnet"],  # Same template, different vars
    deps = [":base"],
    ext_strs = {"env": "production"},
    out = "config-prod.json",
)
```

## Building

```bash
# Build a specific config
tk build //config:config-dev

# View the output
tk build //config:config-dev --show-output
cat $(tk build //config:config-dev --show-output 2>&1 | grep -o 'buck-out/[^ ]*')

# Build all configs
tk build //config:...
```

## External Variables

### ext_strs (External Strings)

Pass string values from the build system:

```python
jsonnet_library(
    name = "config",
    srcs = ["config.jsonnet"],
    ext_strs = {
        "env": "production",
        "version": "1.2.3",
    },
)
```

Access in Jsonnet:

```jsonnet
{
  environment: std.extVar('env'),
  version: std.extVar('version'),
}
```

### ext_codes (External Code)

Pass Jsonnet expressions:

```python
jsonnet_library(
    name = "config",
    srcs = ["config.jsonnet"],
    ext_codes = {
        "replicas": "3",
        "features": "['auth', 'api']",
    },
)
```

### Top-Level Arguments

For parameterized configs using functions:

```jsonnet
// config.jsonnet
function(env, replicas=1) {
  environment: env,
  replicas: replicas,
}
```

```python
jsonnet_library(
    name = "config",
    srcs = ["config.jsonnet"],
    tla_strs = {"env": "production"},
    tla_codes = {"replicas": "5"},
)
```

## Use Cases

- **Kubernetes manifests** - Generate YAML/JSON configs with environment-specific values
- **Application configuration** - Type-safe config generation with inheritance
- **Infrastructure as Code** - Generate Terraform JSON, CloudFormation, etc.
- **CI/CD pipelines** - Generate pipeline configs from templates
