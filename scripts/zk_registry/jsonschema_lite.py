"""A small JSON Schema validator for the subset of draft 2020-12 used by the registry schemas.

Supported keywords: ``type``, ``enum``, ``const``, ``pattern``, ``minLength``, ``minimum``,
``maximum``, ``minItems``, ``properties``, ``required``, ``additionalProperties``, ``items``,
``allOf``, ``anyOf``, ``if``/``then``/``else``, ``$ref`` (local ``#/$defs/...`` only) and the
annotations ``$schema``, ``$id``, ``title``, ``description``, ``$defs``.  Any other keyword is an
error, so the schema cannot silently rely on an unsupported feature.
"""
from __future__ import annotations

import re

_ANNOTATIONS = {"$schema", "$id", "title", "description", "$defs", "$comment", "examples"}
_SUPPORTED = {"type", "enum", "const", "pattern", "minLength", "minimum", "maximum", "minItems", "properties",
              "required", "additionalProperties", "items", "allOf", "anyOf", "if", "then", "else", "$ref"}

_TYPES = {
    "object": lambda v: isinstance(v, dict),
    "array": lambda v: isinstance(v, list),
    "string": lambda v: isinstance(v, str),
    "integer": lambda v: isinstance(v, int) and not isinstance(v, bool),
    "number": lambda v: isinstance(v, (int, float)) and not isinstance(v, bool),
    "boolean": lambda v: isinstance(v, bool),
    "null": lambda v: v is None,
}


class SchemaError(ValueError):
    """The schema itself uses an unsupported keyword or a bad reference."""


def validate(instance, schema: dict, root: dict | None = None, path: str = "$") -> list[str]:
    """All validation errors of ``instance`` against ``schema`` (empty list when valid)."""
    root = schema if root is None else root
    errs: list[str] = []
    unknown = set(schema) - _SUPPORTED - _ANNOTATIONS
    if unknown:
        raise SchemaError(f"unsupported schema keywords at {path}: {sorted(unknown)}")
    if "$ref" in schema:
        ref = schema["$ref"]
        if not ref.startswith("#/$defs/"):
            raise SchemaError(f"unsupported $ref {ref}")
        target = root.get("$defs", {}).get(ref[len("#/$defs/"):])
        if target is None:
            raise SchemaError(f"unresolved $ref {ref}")
        errs += validate(instance, target, root, path)
    if "type" in schema:
        types = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        if not any(_TYPES[t](instance) for t in types):
            return errs + [f"{path}: expected {'/'.join(types)}"]
    if "enum" in schema and instance not in schema["enum"]:
        errs.append(f"{path}: {instance!r} not in {schema['enum']}")
    if "const" in schema and instance != schema["const"]:
        errs.append(f"{path}: expected {schema['const']!r}")
    if isinstance(instance, str):
        if "pattern" in schema and not re.search(schema["pattern"], instance):
            errs.append(f"{path}: does not match {schema['pattern']}")
        if "minLength" in schema and len(instance) < schema["minLength"]:
            errs.append(f"{path}: shorter than {schema['minLength']}")
    if _TYPES["number"](instance):
        if "minimum" in schema and instance < schema["minimum"]:
            errs.append(f"{path}: below {schema['minimum']}")
        if "maximum" in schema and instance > schema["maximum"]:
            errs.append(f"{path}: above {schema['maximum']}")
    if isinstance(instance, list):
        if "minItems" in schema and len(instance) < schema["minItems"]:
            errs.append(f"{path}: fewer than {schema['minItems']} items")
        if "items" in schema:
            for i, item in enumerate(instance):
                errs += validate(item, schema["items"], root, f"{path}[{i}]")
    if isinstance(instance, dict):
        for key in schema.get("required", []):
            if key not in instance:
                errs.append(f"{path}: missing required property {key!r}")
        props = schema.get("properties", {})
        for key, value in instance.items():
            if key in props:
                errs += validate(value, props[key], root, f"{path}.{key}")
            else:
                extra = schema.get("additionalProperties", True)
                if extra is False:
                    errs.append(f"{path}: unexpected property {key!r}")
                elif isinstance(extra, dict):
                    errs += validate(value, extra, root, f"{path}.{key}")
    for sub in schema.get("allOf", []):
        errs += validate(instance, sub, root, path)
    if "anyOf" in schema and not any(not validate(instance, s, root, path) for s in schema["anyOf"]):
        errs.append(f"{path}: matches none of anyOf")
    if "if" in schema:
        branch = "then" if not validate(instance, schema["if"], root, path) else "else"
        if branch in schema:
            errs += validate(instance, schema[branch], root, path)
    return errs
