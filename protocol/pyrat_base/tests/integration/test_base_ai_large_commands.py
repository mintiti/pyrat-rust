"""Tests for PyRatAI handling of large protocol commands.

These tests verify that the protocol can parse and handle very large commands
without hanging or crashing. We test the protocol parsing directly rather than
running the full AI loop to avoid timeout issues in tests.
"""
# ruff: noqa: PLR2004

import pytest

from pyrat_base.protocol import Protocol


class TestLargeCommandHandling:
    """Test that protocol can handle large commands."""

    def test_large_walls_parsing(self):
        """Test parsing very large walls command."""
        protocol = Protocol()

        # Generate a large walls list (100+ walls)
        walls = []
        for i in range(100):
            walls.append(f"({i},{i})-({i+1},{i})")
        walls_cmd = "walls " + " ".join(walls)

        # Parse the command
        cmd = protocol.parse_command(walls_cmd)

        # Verify it parsed correctly
        assert cmd is not None
        assert cmd.type.name == "WALLS"
        assert "walls" in cmd.data
        assert len(cmd.data["walls"]) == 100

        # Check a few wall positions
        assert ((0, 0), (1, 0)) in cmd.data["walls"]
        assert ((99, 99), (100, 99)) in cmd.data["walls"]

    def test_large_mud_parsing(self):
        """Test parsing very large mud command."""
        protocol = Protocol()

        # Generate a large mud list (80+ mud entries)
        mud_entries = []
        for i in range(80):
            mud_entries.append(f"({i},{i})-({i+1},{i}):{(i % 5) + 1}")
        mud_cmd = "mud " + " ".join(mud_entries)

        # Parse the command
        cmd = protocol.parse_command(mud_cmd)

        # Verify it parsed correctly
        assert cmd is not None
        assert cmd.type.name == "MUD"
        assert "mud" in cmd.data
        assert len(cmd.data["mud"]) == 80

        # Check a few mud entries
        first_entry = cmd.data["mud"][0]
        assert first_entry == ((0, 0), (1, 0), 1)

        last_entry = cmd.data["mud"][-1]
        assert last_entry == ((79, 79), (80, 79), 5)

    def test_large_cheese_parsing(self):
        """Test parsing command with many cheese positions."""
        protocol = Protocol()

        # Generate 200 cheese positions
        cheese_positions = []
        for i in range(200):
            cheese_positions.append(f"({i % 21},{i % 15})")
        cheese_cmd = "cheese " + " ".join(cheese_positions)

        # Parse the command
        cmd = protocol.parse_command(cheese_cmd)

        # Verify it parsed correctly
        assert cmd is not None
        assert cmd.type.name == "CHEESE"
        assert "cheese" in cmd.data
        assert len(cmd.data["cheese"]) == 200

    def test_extremely_long_single_wall(self):
        """Test handling a single extremely long command (5000+ chars)."""
        protocol = Protocol()

        # Generate a walls command with 300+ walls to exceed 5000 chars
        walls = []
        for i in range(300):
            walls.append(f"({i*2},{i*2})-({i*2+1},{i*2})")
        walls_cmd = "walls " + " ".join(walls)
        min_command_length = 5000
        assert len(walls_cmd) > min_command_length

        # Parse the very long command
        cmd = protocol.parse_command(walls_cmd)

        # Should still parse correctly
        assert cmd is not None
        assert len(cmd.data["walls"]) == 300

    def test_maze_parsing(self):
        """Test parsing maze dimensions."""
        protocol = Protocol()

        # Large maze
        cmd = protocol.parse_command("maze height:500 width:500")

        assert cmd is not None
        assert cmd.type.name == "MAZE"
        assert cmd.data["width"] == 500
        assert cmd.data["height"] == 500

    def test_mixed_large_game_state(self):
        """Test parsing a sequence of large commands."""
        protocol = Protocol()

        # Create various large commands
        commands = []

        # Large maze
        commands.append(protocol.parse_command("maze height:500 width:500"))

        # Many walls
        walls = [f"({i},{i})-({i+1},{i})" for i in range(50)]
        commands.append(protocol.parse_command(f"walls {' '.join(walls)}"))

        # Many mud entries
        mud = [f"({i},{i})-({i},{i+1}):2" for i in range(50)]
        commands.append(protocol.parse_command(f"mud {' '.join(mud)}"))

        # Many cheese
        cheese = [f"({i*3},{i*2})" for i in range(100)]
        commands.append(protocol.parse_command(f"cheese {' '.join(cheese)}"))

        # All commands should parse successfully
        for cmd in commands:
            assert cmd is not None

        # Verify maze dimensions
        assert commands[0].data["width"] == 500
        assert commands[0].data["height"] == 500

        # Verify counts
        assert len(commands[1].data["walls"]) == 50  # walls
        assert len(commands[2].data["mud"]) == 50  # mud
        assert len(commands[3].data["cheese"]) == 100  # cheese


@pytest.mark.integration
class TestLargeCommandIntegration:
    """Integration tests with actual game state creation."""

    def test_create_game_with_many_walls(self):
        """Test creating a game state with many walls."""
        from pyrat_engine.core.game import GameState as PyGameState

        # Create walls (but not too many to avoid performance issues)
        walls = []
        for i in range(30):
            walls.append(((i, 0), (i, 1)))

        # Create game state
        game = PyGameState.create_custom(
            width=50,
            height=50,
            walls=walls,
            mud=[],
            cheese=[(25, 25)],
            player1_pos=(0, 0),
            player2_pos=(49, 49),
            symmetric=False,
        )

        # Verify it was created successfully
        assert game is not None
        assert game.player1_score == 0
        assert game.player2_score == 0


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
