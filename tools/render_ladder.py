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

REPO_URL = "https://github.com/mintiti/pyrat-rust"

PAGE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>PyRat Botpack Ladder</title>
<style>
  :root {{
    --bg: #0e1014;
    --row: #171a21;
    --row-top: #1d2027;
    --text: #e8e6e1;
    --muted: #8a8f98;
    --cheese: #f4b942;
    --cheese-dim: #b8862c;
    --rust: #de9a66;
    --python: #6da9d8;
    --whisker: rgba(232, 230, 225, 0.55);
    --anchor-line: rgba(232, 230, 225, 0.18);
  }}
  * {{ box-sizing: border-box; }}
  body {{
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    background: var(--bg);
    color: var(--text);
    max-width: 46rem;
    margin: 0 auto;
    padding: 3.5rem 1.25rem 2.5rem;
  }}
  header h1 {{
    font-size: 1.7rem;
    font-weight: 700;
    letter-spacing: -0.02em;
    margin: 0 0 0.4rem;
  }}
  header p {{
    color: var(--muted);
    margin: 0 0 2.2rem;
    font-size: 0.95rem;
    line-height: 1.5;
  }}
  ol.board {{ list-style: none; margin: 0; padding: 0; }}
  li.row {{
    display: grid;
    grid-template-columns: 2rem minmax(11rem, auto) 1fr auto;
    gap: 0.9rem;
    align-items: center;
    background: var(--row);
    border-radius: 10px;
    padding: 0.8rem 1.1rem;
    margin-bottom: 0.5rem;
  }}
  li.row.top {{ background: var(--row-top); box-shadow: inset 0 0 0 1px rgba(244, 185, 66, 0.25); }}
  .rank {{
    color: var(--muted);
    font-size: 0.95rem;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }}
  .row.top .rank {{ color: var(--cheese); font-weight: 700; }}
  .who {{ display: flex; align-items: baseline; gap: 0.5rem; min-width: 0; }}
  .name {{ font-weight: 600; font-size: 1.02rem; }}
  .chip {{
    font-size: 0.68rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    padding: 0.1rem 0.4rem;
    border-radius: 4px;
    color: var(--bg);
  }}
  .chip.rs {{ background: var(--rust); }}
  .chip.py {{ background: var(--python); }}
  .games {{ color: var(--muted); font-size: 0.78rem; white-space: nowrap; }}
  .track {{
    position: relative;
    height: 1.5rem;
    border-radius: 5px;
    background: rgba(255, 255, 255, 0.04);
    overflow: hidden;
  }}
  .anchorline {{
    position: absolute;
    top: 0; bottom: 0;
    width: 0;
    border-left: 1px dashed var(--anchor-line);
  }}
  .bar {{
    position: absolute;
    top: 0.25rem; bottom: 0.25rem;
    left: 0;
    border-radius: 0 3px 3px 0;
    background: linear-gradient(90deg, var(--cheese-dim), var(--cheese));
    opacity: 0.9;
  }}
  .ci {{
    position: absolute;
    top: 50%;
    height: 0;
    border-top: 2px solid var(--whisker);
    transform: translateY(-1px);
  }}
  .ci::before, .ci::after {{
    content: "";
    position: absolute;
    top: -4px;
    height: 8px;
    border-left: 2px solid var(--whisker);
  }}
  .ci::before {{ left: 0; }}
  .ci::after {{ right: 0; }}
  .elo {{
    font-variant-numeric: tabular-nums;
    font-weight: 650;
    font-size: 1.02rem;
    text-align: right;
    white-space: nowrap;
  }}
  .elo .pm {{ color: var(--muted); font-weight: 400; font-size: 0.82rem; }}
  footer {{
    margin-top: 2.2rem;
    color: var(--muted);
    font-size: 0.82rem;
    line-height: 1.7;
  }}
  footer a {{ color: var(--cheese); text-decoration: none; }}
  footer a:hover {{ text-decoration: underline; }}
  @media (max-width: 540px) {{
    li.row {{ grid-template-columns: 1.4rem minmax(0, 1fr) auto; }}
    .track {{ display: none; }}
  }}
</style>
</head>
<body>
<header>
  <h1>&#129472; PyRat Botpack Ladder</h1>
  <p>The <a href="{repo}/tree/main/botpack" style="color:var(--cheese);text-decoration:none">botpack bots</a>,
  rated against each other under fixed conditions. Recomputed from scratch on
  every merge to main.</p>
</header>
<main>
<ol class="board">
{rows}
</ol>
</main>
<footer>
<p>Updated {date} &middot; commit <a href="{repo}/commit/{sha}">{sha_short}</a> &middot;
{success} games ({failure} failed attempts) &middot; <a href="results.json">results.json</a></p>
<p>&plusmn; is a ~95% interval (2&times;stderr), conditional on the anchor:
greedy is pinned at 1000, matching alpharat's scale. The dashed line marks it.
Conditions live in <a href="{repo}/blob/main/botpack/ladder.toml">ladder.toml</a>.</p>
</footer>
</body>
</html>
"""

ROW = """<li class="row{top}">
  <span class="rank">{rank}</span>
  <div class="who"><span class="name">{name}</span><span class="chip {lang}">{lang}</span></div>
  <div class="track">{anchor}<div class="bar" style="width:{bar_pct:.1f}%"></div>{ci}</div>
  <div class="elo">{elo}{pm} <span class="games">&middot; {games} games</span></div>
</li>"""


def split_lang(player_id: str) -> tuple[str, str]:
    """Botpack convention: `-py` suffix marks Python bots, the rest are Rust."""
    if player_id.endswith("-py"):
        return player_id[: -len("-py")], "py"
    return player_id, "rs"


def build_rows(standings: list[dict]) -> str:
    cis = [1.96 * float(r.get("elo_stderr", 0.0)) for r in standings]
    los = [r["elo"] - ci for r, ci in zip(standings, cis)]
    his = [r["elo"] + ci for r, ci in zip(standings, cis)]
    span_lo, span_hi = min(los), max(his)
    pad = 0.06 * (span_hi - span_lo) or 1.0
    axis_lo, axis_hi = span_lo - pad, span_hi + pad
    width = axis_hi - axis_lo

    def pct(v: float) -> float:
        return max(0.0, min(100.0, 100.0 * (v - axis_lo) / width))

    anchor_html = ""
    anchor_elo = 1000.0
    if axis_lo <= anchor_elo <= axis_hi:
        anchor_html = f'<div class="anchorline" style="left:{pct(anchor_elo):.1f}%"></div>'

    rows = []
    for rank, (r, ci) in enumerate(zip(standings, cis), start=1):
        name, lang = split_lang(r["player_id"])
        ci_html = ""
        if ci > 0:
            left, right = pct(r["elo"] - ci), pct(r["elo"] + ci)
            ci_html = f'<div class="ci" style="left:{left:.1f}%;width:{right - left:.1f}%"></div>'
        pm = f' <span class="pm">&plusmn;{ci:.0f}</span>' if ci > 0 else ""
        rows.append(
            ROW.format(
                top=" top" if rank == 1 else "",
                rank=rank,
                name=html.escape(name),
                lang=lang,
                anchor=anchor_html,
                bar_pct=pct(r["elo"]),
                ci=ci_html,
                elo=f"{r['elo']:.0f}",
                pm=pm,
                games=r.get("games", "?"),
            )
        )
    return "\n".join(rows)


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
    sha = os.environ.get("GITHUB_SHA", "local")
    date = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "index.html").write_text(
        PAGE.format(
            repo=REPO_URL,
            rows=build_rows(standings),
            date=date,
            sha=html.escape(sha),
            sha_short=html.escape(sha[:9]),
            success=attempts.get("success", "?"),
            failure=attempts.get("failure", "?"),
        )
    )
    shutil.copyfile(results_path, out_dir / "results.json")

    print("| # | Bot | Elo | ± | Games |")
    print("|--:|-----|----:|--:|------:|")
    for rank, row in enumerate(standings, start=1):
        ci = 1.96 * float(row.get("elo_stderr", 0.0))
        print(
            f"| {rank} | {row['player_id']} | {row['elo']:.0f} "
            f"| ±{ci:.0f} | {row.get('games', '?')} |"
        )

    return 0


if __name__ == "__main__":
    sys.exit(main())
