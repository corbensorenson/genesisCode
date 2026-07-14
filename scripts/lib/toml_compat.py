"""TOML reader compatible with the declared Python 3.9 minimum."""

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.9 and 3.10
    from vendor import tomli as tomllib

__all__ = ["tomllib"]
