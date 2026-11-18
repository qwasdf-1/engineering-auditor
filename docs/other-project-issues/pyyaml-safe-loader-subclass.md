# PyYAML safe-loader subclasses are reported as unsafe deserialization

**Status:** Fixed after 1.5.0 (092ad43, unreleased). **Observed:**
2026-10-29 and 2026-10-30 with Engineering Auditor 1.5.0.

## Evidence

A downstream Python tool subclasses `yaml.SafeLoader` only to reject
duplicate mapping keys, and loads its files with that loader. This
reconstruction has the same shape:

```python
import yaml


class UniqueKeyLoader(yaml.SafeLoader):
    """A safe loader that rejects duplicate mapping keys."""


def _unique_mapping(loader, node, deep=False):
    loader.flatten_mapping(node)
    result = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in result:
            raise yaml.constructor.ConstructorError(
                None, None, f"duplicate key {key!r}", key_node.start_mark
            )
        result[key] = loader.construct_object(value_node, deep=deep)
    return result


UniqueKeyLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _unique_mapping
)


def load(path):
    with open(path, encoding="utf-8") as source:
        return yaml.load(source, Loader=UniqueKeyLoader)
```

1.5.0 reported each `yaml.load` call through this loader as a high-severity
`unsafe_deserialization` finding (CWE-502). That was four calls in the
downstream tool. The base loader constructs no arbitrary objects, and the
added constructor only validates keys.

## Root Cause

The Python check accepted a `Loader=` argument only when it named
`SafeLoader`, `CSafeLoader`, or `BaseLoader` directly. It did not follow a
repository class back to a safe base.

## Resolution

092ad43 reads a repository subclass of a safe loader as safe, unless the
repository registers a `python/` tag constructor on it. It also reads a
ruamel.yaml `YAML()` load as safe unless its `typ` builds objects. Unknown,
imported, and unresolvable loaders stay reportable.
