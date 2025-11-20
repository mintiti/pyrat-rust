"""Move provider abstractions for decoupling game execution from move acquisition."""

from typing import Optional, Protocol

from pyrat_engine.core import Direction

from .ai_process import AIInfo, AIProcess
from .logger import GameLogger


class MoveProvider(Protocol):
    """Protocol for providing moves for a player.

    This abstraction decouples game execution from move acquisition,
    enabling headless execution, direct function calls, and testability.
    """

    @property
    def info(self) -> AIInfo:
        """Get player information (name, author, etc.)."""
        ...

    def start(self) -> bool:
        """Start the provider and perform initialization.

        Returns:
            True if successful, False otherwise
        """
        ...

    def send_game_start(self, game, preprocessing_time: float) -> None:
        """Send game initialization to the provider.

        Args:
            game: PyRat game instance
            preprocessing_time: Time allowed for preprocessing in seconds
        """
        ...

    def get_move(
        self, rat_move: Direction, python_move: Direction
    ) -> Optional[Direction]:
        """Get next move from the provider.

        Args:
            rat_move: Last move made by rat
            python_move: Last move made by python

        Returns:
            Direction or None if timeout/error occurred
        """
        ...

    def send_game_over(
        self, winner: str, rat_score: float, python_score: float
    ) -> None:
        """Notify provider that game has ended.

        Args:
            winner: "rat", "python", or "draw"
            rat_score: Final rat score
            python_score: Final python score
        """
        ...

    def stop(self) -> None:
        """Stop the provider and clean up resources."""
        ...

    def is_alive(self) -> bool:
        """Check if provider is still functional.

        Returns:
            True if provider can still provide moves, False otherwise
        """
        ...


class SubprocessMoveProvider:
    """Move provider that uses subprocess-based AI via protocol communication.

    This wraps the existing AIProcess to conform to the MoveProvider protocol,
    enabling the GameRunner to work with the abstraction.
    """

    def __init__(
        self,
        script_path: str,
        player_name: str,
        timeout: float = 1.0,
        logger: Optional[GameLogger] = None,
    ):
        """Initialize subprocess move provider.

        Args:
            script_path: Path to the AI script
            player_name: "rat" or "python"
            timeout: Timeout in seconds for AI responses
        """
        self._ai_process = AIProcess(script_path, player_name, timeout, logger=logger)

    @property
    def info(self) -> AIInfo:
        """Get player information."""
        return self._ai_process.info

    def start(self) -> bool:
        """Start the AI subprocess."""
        return self._ai_process.start()

    def send_game_start(self, game, preprocessing_time: float) -> None:
        """Send game initialization to AI."""
        self._ai_process.send_game_start(game, preprocessing_time)

    def get_move(
        self, rat_move: Direction, python_move: Direction
    ) -> Optional[Direction]:
        """Get move from AI subprocess."""
        return self._ai_process.get_move(rat_move, python_move)

    def send_game_over(
        self, winner: str, rat_score: float, python_score: float
    ) -> None:
        """Notify AI of game end."""
        self._ai_process.send_game_over(winner, rat_score, python_score)

    def stop(self) -> None:
        """Stop the AI subprocess."""
        self._ai_process.stop()

    def is_alive(self) -> bool:
        """Check if AI subprocess is still alive."""
        return self._ai_process.is_alive()

    # Optional extension used by game loop to keep AI protocol in sync on timeouts
    def notify_timeout(self, default_move: Direction) -> None:
        """Inform the AI subprocess that a move timed out (protocol message)."""
        self._ai_process.notify_timeout(default_move)
