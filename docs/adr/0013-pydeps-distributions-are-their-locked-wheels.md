---
status: accepted
---

# A pydeps distribution is its locked wheel

A Python library target must hold what an installer would put in site-packages, and nothing else. An sdist is a build input, not that layout. It ships `setup.py`, `tests/` and `docs/` at its root, and src-layout projects (`requests`, `charset_normalizer`) keep their code under `src/`. So globbing `**/*.py` over the unpacked sdist with `base_module = ""` turned `setup.py` and `tests/__init__.py` into modules that collide between distributions, made `requests` importable only as `src.requests`, and dropped data files such as `certifi`'s `cacert.pem`. A wheel *is* the installed layout, and `uv` has already chosen and hashed one for every distribution in the lock. So in the `pydeps` cell of [ADR 0010](0010-pydeps-stores-one-distribution-per-store-link.md):

- **A distribution's store link holds its locked pure wheel (`py3-none-any`), unpacked.** `pydeps-gen` takes the wheel over the sdist. `<name>.data/purelib` and `platlib` are merged into the root, as an installer would do. The rest of `<name>.data/` (scripts, headers, data) is dropped. Fixups, then user patches, apply to that tree.
- **The distribution's `python_library` holds everything the wheel installs**, with `base_module = ""`: its `.py` files as `srcs`, and every other file, `*.dist-info` included, as `resources`. `importlib.metadata` and entry points read `dist-info`.
- **A distribution with no pure wheel fails `pydeps-gen`**, which names it. That covers both a distribution with only platform wheels and one with only an sdist.
- **`python-deps.toml` moves to `schema_version = 3`.** Its `url` and `hash` describe the wheel, so a v2 file fails and asks for `tk sync` rather than being read as a wheel.

Decided in triage of [Python binaries fail when two vendored sdists ship a top-level setup.py](https://github.com/firefly-engineering/turnkey/issues/246).

## Considered options

- **Build each sdist into a wheel in its Nix derivation** (`pip wheel --no-build-isolation`). Rejected. It needs each build backend (setuptools, hatchling, flit, maturin…) and its build dependencies from nixpkgs, which `uv.lock` doesn't lock, and compiled backends make it heavy.
- **Work out the installed layout from the sdist's metadata** (`top_level.txt`, `setup.cfg`, `pyproject.toml`). Rejected. It means parsing every build backend's configuration, and the result would still be a guess.
- **Exclude build-only files (`setup.py` and similar) from the sdist glob.** Rejected. It leaves `tests/`, src layouts and data files broken.
- **`prebuilt_python_library` over the `.whl`**, letting buck2 install it. Rejected. Fixups and user patches need an unpacked tree to apply to.
- **Platform wheels, chosen per platform with `select()`.** Deferred. It takes one store link per platform and Python ABI, which goes against ADR 0010's one store link per distribution, and nothing needs it yet.

## Consequences

- **Distributions with compiled code can't be vendored yet.** Locks that need them fail `pydeps-gen` until platform wheels are supported.
- **User patches written against the sdist layout stop applying.** Paths change (`vendor/requests/src/requests/…` becomes `vendor/requests/requests/…`). The distribution's build fails and asks for the patch to be regenerated with `tk compose patch`.
- **Fixups now stand in for nothing an sdist build would run.** The wheel is already built. Fixups only correct the installed tree.
