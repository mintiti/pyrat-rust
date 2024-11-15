from typing import Any

# TODO(MT) : Make this cleaner on the typing
class PyRatEnv:
    def __init__(
        self,
        width: int | None = None,
        height: int | None = None,
        cheese_count: int | None = None,
        symmetric: bool = True,
        seed: int | None = None,
    ) -> None: ...
    def reset(self, seed: int | None = None) -> dict[str, Any]: ...
    def step(
        self,
        actions: list[int],
    ) -> tuple[dict[str, Any], list[float], bool, bool, dict[str, Any]]: ...
    @property
    def num_actions(self) -> int: ...
    def get_state(self) -> dict[str, Any]: ...
