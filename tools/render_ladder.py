#!/usr/bin/env python3
"""Render botpack ladder standings from pyrat-eval results JSON.

Usage: render_ladder.py <results.json> <out_dir>

Writes <out_dir>/index.html and copies the results JSON to
<out_dir>/results.json. Prints a markdown table to stdout (the CI workflow
pipes it into the job summary).
"""

import html
import json
import os
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

PAGE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>PyRat Botpack Ladder</title>
<style>
  body {{ font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; color: #222; }}
  table {{ border-collapse: collapse; width: 100%; }}
  th, td {{ text-align: left; padding: 0.4rem 0.8rem; border-bottom: 1px solid #ddd; }}
  th {{ border-bottom: 2px solid #222; }}
  td.num, th.num {{ text-align: right; font-variant-numeric: tabular-nums; }}
  footer {{ margin-top: 2rem; font-size: 0.85rem; color: #666; }}
  code {{ background: #f4f4f4; padding: 0.1rem 0.3rem; border-radius: 3px; }}
</style>
</head>
<body>
<h1>PyRat Botpack Ladder</h1>
<p>Elo standings of the botpack bots, recomputed from scratch on every merge
to <code>main</code> under the conditions pinned in
<code>botpack/ladder.toml</code>.</p>
<table>
<thead><tr><th class="num">#</th><th>Bot</th><th class="num">Elo</th></tr></thead>
<tbody>
{rows}
</tbody>
</table>
<footer>
<p>Updated {date} &middot; commit <code>{sha}</code> &middot;
{success} games played ({failure} failed attempts) &middot;
<a href="results.json">results.json</a></p>
</footer>
</body>
</html>
"""


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    results_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])

    results = json.loads(results_path.read_text())
    if results.get("status") != "finished" or not results.get("standings"):
        print(f"no standings to render (status: {results.get('status')})", file=sys.stderr)
        return 1

    standings = results["standings"]
    attempts = results.get("attempts", {})
    sha = os.environ.get("GITHUB_SHA", "local")[:9]
    date = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    rows = "\n".join(
        f'<tr><td class="num">{rank}</td><td>{html.escape(row["player_id"])}</td>'
        f'<td class="num">{row["elo"]:.0f}</td></tr>'
        for rank, row in enumerate(standings, start=1)
    )

    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "index.html").write_text(
        PAGE.format(
            rows=rows,
            date=date,
            sha=html.escape(sha),
            success=attempts.get("success", "?"),
            failure=attempts.get("failure", "?"),
        )
    )
    shutil.copyfile(results_path, out_dir / "results.json")

    print("| # | Bot | Elo |")
    print("|--:|-----|----:|")
    for rank, row in enumerate(standings, start=1):
        print(f"| {rank} | {row['player_id']} | {row['elo']:.0f} |")

    return 0


if __name__ == "__main__":
    sys.exit(main())
