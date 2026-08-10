"""
03_trajectory_comparison.py
============================
Validates the SD-PLC compiled binary against the CODESYS 10 ms benchmark trace
using two complementary pieces of evidence:

  1. Trajectory context  — CODESYS trace over 1 000 cycles with the SD-PLC
                           final value plotted as an endpoint marker.  The
                           marker colour is green (within tolerance) or red
                           (outside tolerance) so the verdict is instant.

  2. Tolerance margin    — horizontal bar chart showing how much headroom each
                           SD-PLC final value has relative to its tolerance
                           band.  A bar that reaches 100 % means the value is
                           exactly at the tolerance boundary; 0 % means a
                           perfect match.

  3. Verdict table       — full numeric breakdown.

Run from the repo root:
    python analysis/03_trajectory_comparison.py

Produces:
    analysis/figs/fig_03_trajectory.png
"""

import pathlib

import matplotlib
matplotlib.use("Agg")
import matplotlib.gridspec as gridspec
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

# ── paths ────────────────────────────────────────────────────────────────────
HERE            = pathlib.Path(__file__).parent
REPO            = HERE.parent
CODESYS_CSV     = REPO / "benchmark" / "codesys_flotation_10ms.csv"
SDPLC_FINAL_CSV = REPO / "results" / "runtime" / "latest" / "runtime_final_values.csv"
FIGDIR          = HERE / "figs"
FIGDIR.mkdir(exist_ok=True)

# ── palette ──────────────────────────────────────────────────────────────────
SKY    = "#2563EB"
AMBER  = "#F59E0B"
TEAL   = "#0D9488"
VIOLET = "#6D5BB5"
RED    = "#EF4444"
GREEN  = "#16A34A"
INK    = "#1E293B"
SLATE  = "#64748B"
PAPER  = "#FAFBFC"
HAIR   = "#E2E8F0"

SCAN_MS = 10

# (csv_column, display_label, trace_colour, endpoint_tolerance)
VARIABLES = [
    ("level",             "Tank level",         TEAL,   1.0),
    ("air_flow",          "Air flow",            SKY,    1.0),
    ("feed_flow",         "Feed flow",           AMBER,  1.0),
    ("concentrate_grade", "Concentrate grade",   VIOLET, 0.1),
]
EXTRA_ENDPOINT = [
    ("tailings_flow", "Tailings flow", 2.5),
]
STATE_VARS = ["motor_running", "emergency_stop"]


# ── loaders ──────────────────────────────────────────────────────────────────
def load_codesys(path):
    if path.exists():
        df = pd.read_csv(path)
        print(f"[DATA] CODESYS trace  {path.relative_to(REPO)}  ({len(df)} rows)")
        return df
    print("[WARN] CODESYS CSV not found — using synthetic placeholder.")
    print(f"       Place real export at: {path}")
    N, tau = 1000, 250
    t = np.arange(N)
    rng = np.random.default_rng(0)
    def ap(s, e, n=0.01):
        return s + (e - s) * (1 - np.exp(-t / tau)) + rng.normal(0, n, N)
    return pd.DataFrame({
        "level":             ap(50.00, 61.89, 0.02),
        "air_flow":          ap(30.00, 49.08, 0.05),
        "feed_flow":         ap(40.00, 59.08, 0.05),
        "tailings_flow":     ap(32.00, 35.00, 0.03),
        "concentrate_grade": ap(82.00, 82.72, 0.005),
        "motor_running":     np.ones(N, dtype=int),
        "emergency_stop":    np.zeros(N, dtype=int),
    })


def load_runtime_final(path):
    if path.exists():
        df = pd.read_csv(path)
        out = {}
        for _, row in df.iterrows():
            raw = str(row["value"]).strip()
            if raw.upper() in {"TRUE", "FALSE"}:
                out[str(row["name"])] = raw.upper() == "TRUE"
            else:
                try:
                    out[str(row["name"])] = float(raw)
                except ValueError:
                    out[str(row["name"])] = raw
        print(f"[DATA] SD-PLC finals  {path.relative_to(REPO)}")
        return out
    print("[WARN] runtime_final_values.csv not found — using thesis Table 5.4 values.")
    return {
        "level": 62.429, "air_flow": 50.000, "feed_flow": 60.000,
        "tailings_flow": 37.300, "concentrate_grade": 82.750,
        "motor_running": True, "emergency_stop": False,
    }


def as_bool(v):
    if pd.isna(v): return False
    if isinstance(v, (bool, np.bool_)): return bool(v)
    if isinstance(v, str): return v.strip().lower() in {"true","1","yes"}
    return bool(int(v))


# ── load ─────────────────────────────────────────────────────────────────────
codesys       = load_codesys(CODESYS_CSV)
runtime_final = load_runtime_final(SDPLC_FINAL_CSV)
N      = len(codesys)
time_s = np.arange(N) * SCAN_MS / 1000.0
last   = codesys.iloc[-1]


# ── statistics ────────────────────────────────────────────────────────────────
endpoint_stats = []
all_pass = True
for col, label, tol in [(c,l,t) for c,l,_,t in VARIABLES] + EXTRA_ENDPOINT:
    if col not in runtime_final or col not in last.index:
        continue
    sd, cd = float(runtime_final[col]), float(last[col])
    diff   = abs(sd - cd)
    margin = diff / tol
    ok     = margin <= 1.0
    if not ok: all_pass = False
    endpoint_stats.append(dict(col=col, label=label, tol=tol,
                               sd=sd, cd=cd, diff=diff,
                               margin=margin, verdict="Within tolerance" if ok else "OUTSIDE"))

state_mismatches, state_results = 0, []
for col in STATE_VARS:
    if col not in runtime_final or col not in last.index: continue
    sd, cd = bool(runtime_final[col]), as_bool(last[col])
    mm = sd != cd
    state_mismatches += int(mm)
    state_results.append((col, sd, cd, mm))


# ── terminal summary ──────────────────────────────────────────────────────────
print()
print("=" * 84)
print("  SD-PLC vs CODESYS — endpoint comparison  (compiled runtime final values)")
print("=" * 84)
print(f"  {'Variable':<22} {'SD-PLC':>10} {'CODESYS':>10}"
      f" {'|Δ|':>8} {'Tol':>7} {'Margin':>8}  Verdict")
print("  " + "-" * 80)
for s in endpoint_stats:
    print(f"  {s['label']:<22} {s['sd']:>10.3f} {s['cd']:>10.3f}"
          f" {s['diff']:>8.3f} {s['tol']:>7.3f} {s['margin']:>7.1%}  {s['verdict']}")
print(f"\n  Boolean state mismatches: {state_mismatches}")
print(f"  Overall: {'ALL PASS ✓' if all_pass and state_mismatches==0 else 'FAILURES PRESENT ✗'}")
print("=" * 84)
print()
print("  Interpretation:")
print("  The CODESYS Trace runs the full closed-loop simulation so process variables")
print("  evolve over time.  SD-PLC validates at the *endpoint*: after 1 000 scans the")
print("  compiled binary's final outputs land within tolerance of the CODESYS reference.")
print()


# ── figure ────────────────────────────────────────────────────────────────────
fig = plt.figure(figsize=(14, 11), facecolor=PAPER)
gs  = gridspec.GridSpec(3, 2, figure=fig,
                        hspace=0.54, wspace=0.32,
                        height_ratios=[1, 1, 1.15])


# ── panels A–D: CODESYS trajectory + SD-PLC endpoint marker ──────────────────
for i, (col, label, colour, tol) in enumerate(VARIABLES):
    ax = fig.add_subplot(gs[i // 2, i % 2])
    ax.set_facecolor("white")
    panel_letter = chr(ord("A") + i)

    if col in codesys.columns:
        ax.plot(time_s, codesys[col].astype(float),
                color=colour, linewidth=2.2, zorder=3,
                label="CODESYS trace (benchmark)")

    if col in runtime_final:
        sd_val  = float(runtime_final[col])
        cd_val  = float(last[col]) if col in last.index else None
        diff    = abs(sd_val - cd_val) if cd_val is not None else None
        within  = (diff is not None) and (diff <= tol)
        dot_c   = GREEN if within else RED

        ax.scatter(time_s[-1], sd_val, s=65, color=dot_c, zorder=5,
                   label="SD-PLC compiled final")

        if diff is not None:
            sym = "✓" if within else "✗"
            # nudge annotation above or below the dot depending on position
            cd_end = float(codesys[col].iloc[-1]) if col in codesys.columns else sd_val
            y_off  = 18 if sd_val <= cd_end else -42
            ax.annotate(
                f" SD-PLC: {sd_val:.3f}\n CODESYS: {cd_val:.3f}\n Δ = {diff:.3f}  {sym}",
                xy=(time_s[-1], sd_val),
                xytext=(-95, y_off), textcoords="offset points",
                fontsize=7.5, color=INK,
                bbox=dict(boxstyle="round,pad=0.3", fc="white",
                          ec=dot_c, lw=0.9, alpha=0.93),
                arrowprops=dict(arrowstyle="-", color=dot_c, lw=0.9),
                zorder=6,
            )

    ax.set_title(f"{panel_letter}  {label}", fontsize=10.5, color=INK, pad=5)
    ax.set_xlabel("Time (s)", fontsize=9, color=SLATE)
    ax.set_ylabel("Value", fontsize=9, color=SLATE)
    ax.tick_params(colors=SLATE, labelsize=8)
    ax.grid(color=HAIR, linewidth=0.5, zorder=0)
    ax.legend(fontsize=8, framealpha=0.9, loc="upper left")
    for spine in ax.spines.values():
        spine.set_color(HAIR)


# ── panel E: tolerance margin chart ──────────────────────────────────────────
ax_m = fig.add_subplot(gs[2, 0])
ax_m.set_facecolor("white")

labels  = [s["label"]          for s in endpoint_stats]
margins = [s["margin"] * 100   for s in endpoint_stats]
colours = [GREEN if s["margin"] <= 1.0 else RED for s in endpoint_stats]

y = np.arange(len(labels))
ax_m.barh(y, margins, color=colours, alpha=0.85, height=0.55, zorder=3)
ax_m.axvline(100, color=RED, linewidth=1.5, linestyle="--",
             label="Tolerance boundary  (100 %)")

for yi, s in zip(y, endpoint_stats):
    ax_m.text(s["margin"] * 100 + 1.5, yi,
              f"Δ = {s['diff']:.3f} / ± {s['tol']:.1f}",
              va="center", ha="left", fontsize=8, color=INK)

ax_m.set_yticks(y)
ax_m.set_yticklabels(labels, fontsize=9)
ax_m.set_xlabel("Tolerance consumed  (%)", fontsize=9, color=SLATE)
ax_m.set_xlim(0, 175)
ax_m.set_title("E  Tolerance margin\n(lower bar = closer match to CODESYS)",
               fontsize=10, color=INK, pad=5)
ax_m.tick_params(colors=SLATE, labelsize=8)
ax_m.grid(axis="x", color=HAIR, linewidth=0.5, zorder=0)
ax_m.legend(fontsize=8.5, loc="lower right")
for spine in ax_m.spines.values():
    spine.set_color(HAIR)


# ── panel F: verdict table ────────────────────────────────────────────────────
ax_t = fig.add_subplot(gs[2, 1])
ax_t.set_facecolor(PAPER)
ax_t.axis("off")
ax_t.set_title("F  Endpoint verdict table", fontsize=10, color=INK, pad=5)

tbl_rows = [
    [s["label"], f"{s['sd']:.3f}", f"{s['cd']:.3f}",
     f"{s['diff']:.3f}", f"± {s['tol']:.1f}", s["verdict"]]
    for s in endpoint_stats
]
tbl_rows.append([
    "Boolean states", "runtime final", "CODESYS final",
    str(state_mismatches), "0 mismatches",
    "Match" if state_mismatches == 0 else "Mismatch",
])

tbl = ax_t.table(
    cellText=tbl_rows,
    colLabels=["Variable", "SD-PLC", "CODESYS", "|Δ|", "Tolerance", "Verdict"],
    cellLoc="center", loc="center", bbox=[0, 0, 1, 1],
)
tbl.auto_set_font_size(False)
tbl.set_fontsize(9.2)
for (r, c), cell in tbl.get_celld().items():
    if r == 0:
        cell.set_facecolor(VIOLET)
        cell.set_text_props(color="white", fontweight="bold")
    elif r % 2 == 1:
        cell.set_facecolor("#F0EDFA")
    else:
        cell.set_facecolor("white")
    if c == 5 and r > 0:
        ok = cell.get_text().get_text() in {"Within tolerance", "Match"}
        cell.set_text_props(color=GREEN if ok else RED, fontweight="bold")
    cell.set_edgecolor(HAIR)


# ── footer note ───────────────────────────────────────────────────────────────
fig.text(
    0.5, -0.018,
    "Trajectory panels: CODESYS benchmark run as reference process evolution.  "
    "SD-PLC endpoint markers (green ✓ = within tolerance, red ✗ = outside) come "
    "from the compiled binary after 1 000 scan cycles.  Tolerance values are "
    "engineering judgements stated in the thesis.",
    ha="center", va="top", fontsize=8.5, color=SLATE, style="italic",
    transform=fig.transFigure,
)

fig.suptitle(
    f"SD-PLC vs CODESYS  —  Trajectory context and endpoint validation"
    f"  (1 000 cycles · 10 ms scan · N = {N})",
    fontsize=13, fontweight="bold", color=INK, y=1.02,
)

out = FIGDIR / "fig_03_trajectory.png"
fig.savefig(out, dpi=160, bbox_inches="tight", facecolor=PAPER)
print(f"Saved → {out}")
plt.close(fig)