"""UCI-style bot options — declare typed options as class attributes.

Descriptors resolve to their typed default on instance access, and can be
overridden by the host via ``SetOption`` messages before the game starts.

Usage::

    from pyrat_sdk import Bot, Direction, Spin, Check, Combo, Str

    class MyBot(Bot):
        name = "Greedy+"
        depth = Spin(default=3, min=1, max=10)
        avoid_mud = Check(default=True)
        strategy = Combo(default="greedy", choices=["greedy", "defensive"])
        model_path = Str(default="")

        def think(self, state, ctx):
            if self.depth > 5:  # resolves to int (default or host-set)
                ...
"""

from __future__ import annotations

import sys
from typing import Any


# ---------------------------------------------------------------------------
# Base descriptor
# ---------------------------------------------------------------------------


class _OptionDescriptor:
    """Base class for option descriptors.  Subclasses must set *wire_type*
    and implement *coerce*, *validate_default*, and *to_wire_default*.
    """

    wire_type: int  # OptionType enum value

    def __init__(self, *, default: Any) -> None:
        self.default = default
        self.name: str = ""  # set by __set_name__

    def __set_name__(self, owner: type, name: str) -> None:
        self.name = name

    def __get__(self, obj: object, objtype: type | None = None) -> Any:
        if obj is None:
            return self  # class-level → return descriptor itself
        return getattr(obj, f"_opt_{self.name}", self.default)

    def __set__(self, obj: object, value: Any) -> None:
        setattr(obj, f"_opt_{self.name}", value)

    # -- Subclass interface --------------------------------------------------

    def coerce(self, raw: str) -> Any:
        """Convert a wire string to the typed value.  Raise ValueError on failure."""
        raise NotImplementedError

    def validate_default(self) -> None:
        """Raise TypeError/ValueError if the declared default is invalid."""
        raise NotImplementedError

    def to_wire_default(self) -> str:
        """Serialize the default for OptionDef.default_value."""
        raise NotImplementedError


# ---------------------------------------------------------------------------
# Concrete descriptors
# ---------------------------------------------------------------------------


class Check(_OptionDescriptor):
    """Boolean option (OptionType.Check = 0)."""

    wire_type = 0

    def __init__(self, *, default: bool = False) -> None:
        super().__init__(default=default)

    def validate_default(self) -> None:
        if not isinstance(self.default, bool):
            raise TypeError(
                f"Check option '{self.name}': default must be bool, "
                f"got {type(self.default).__name__}"
            )

    def coerce(self, raw: str) -> bool:
        if raw == "true":
            return True
        if raw == "false":
            return False
        raise ValueError(
            f"Check option '{self.name}': expected 'true' or 'false', got {raw!r}"
        )

    def to_wire_default(self) -> str:
        return "true" if self.default else "false"


class Spin(_OptionDescriptor):
    """Integer option with min/max bounds (OptionType.Spin = 1)."""

    wire_type = 1

    def __init__(self, *, default: int, min: int, max: int) -> None:
        super().__init__(default=default)
        self.min = min
        self.max = max

    def validate_default(self) -> None:
        if not isinstance(self.default, int) or isinstance(self.default, bool):
            raise TypeError(
                f"Spin option '{self.name}': default must be int, "
                f"got {type(self.default).__name__}"
            )
        if not (self.min <= self.default <= self.max):
            raise ValueError(
                f"Spin option '{self.name}': default {self.default} "
                f"not in [{self.min}, {self.max}]"
            )

    def coerce(self, raw: str) -> int:
        try:
            value = int(raw)
        except (ValueError, TypeError):
            raise ValueError(
                f"Spin option '{self.name}': cannot convert {raw!r} to int"
            ) from None
        if not (self.min <= value <= self.max):
            raise ValueError(
                f"Spin option '{self.name}': {value} not in [{self.min}, {self.max}]"
            )
        return value

    def to_wire_default(self) -> str:
        return str(self.default)


class Combo(_OptionDescriptor):
    """String option constrained to a set of choices (OptionType.Combo = 2)."""

    wire_type = 2

    def __init__(self, *, default: str, choices: list[str]) -> None:
        super().__init__(default=default)
        self.choices = choices

    def validate_default(self) -> None:
        if not isinstance(self.default, str):
            raise TypeError(
                f"Combo option '{self.name}': default must be str, "
                f"got {type(self.default).__name__}"
            )
        if self.default not in self.choices:
            raise ValueError(
                f"Combo option '{self.name}': default {self.default!r} "
                f"not in {self.choices!r}"
            )

    def coerce(self, raw: str) -> str:
        if raw not in self.choices:
            raise ValueError(
                f"Combo option '{self.name}': {raw!r} not in {self.choices!r}"
            )
        return raw

    def to_wire_default(self) -> str:
        return str(self.default)


class Str(_OptionDescriptor):
    """Free-form string option (OptionType.String = 3)."""

    wire_type = 3

    def __init__(self, *, default: str = "") -> None:
        super().__init__(default=default)

    def validate_default(self) -> None:
        if not isinstance(self.default, str):
            raise TypeError(
                f"Str option '{self.name}': default must be str, "
                f"got {type(self.default).__name__}"
            )

    def coerce(self, raw: str) -> str:
        return raw

    def to_wire_default(self) -> str:
        return str(self.default)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def collect_options(cls: type) -> dict[str, _OptionDescriptor]:
    """Walk MRO and collect all option descriptors, validating defaults."""
    options: dict[str, _OptionDescriptor] = {}
    # Walk MRO in reverse so subclass overrides win.
    for base in reversed(cls.__mro__):
        for attr_name, attr_value in vars(base).items():
            if isinstance(attr_value, _OptionDescriptor):
                attr_value.validate_default()
                options[attr_name] = attr_value
    return options


def options_to_wire(
    options: dict[str, _OptionDescriptor],
) -> list[dict[str, Any]]:
    """Convert collected descriptors to dicts the codec can serialize."""
    result = []
    for name, desc in options.items():
        entry: dict[str, Any] = {
            "name": name,
            "wire_type": desc.wire_type,
            "default_str": desc.to_wire_default(),
        }
        if isinstance(desc, Spin):
            entry["min"] = desc.min
            entry["max"] = desc.max
        if isinstance(desc, Combo):
            entry["choices"] = desc.choices
        result.append(entry)
    return result


def apply_set_option(
    bot: object,
    option_defs: dict[str, _OptionDescriptor],
    name: str,
    value: str,
) -> None:
    """Coerce a wire string and set the option on the bot instance.

    Unknown names and invalid values print a warning and are ignored.
    """
    desc = option_defs.get(name)
    if desc is None:
        print(f"SetOption: unknown option {name!r}, ignoring", file=sys.stderr)
        return
    try:
        typed = desc.coerce(value)
    except (ValueError, TypeError) as e:
        print(f"SetOption: {e}, keeping default", file=sys.stderr)
        return
    desc.__set__(bot, typed)
