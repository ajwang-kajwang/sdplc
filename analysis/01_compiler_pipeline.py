"""
01_compiler_pipeline.py
=======================
Summarises the SD-PLC compiler benchmark and CODESYS acceptance comparison.

Run from the root of the SD-PLC repository:
    python analysis/01_compiler_pipeline.py

Reads:
    results/compiler_benchmark/compiler_pipeline_benchmark.csv

Produces:
    analysis/figs/fig_01_compiler.png
"""

import pathlib

import matplotlib

matplotlib.use("Agg")
import matplotlib.patches as mpatches
import matplotlib.pyplot as plt
import pandas as pd

HERE = pathlib.Path(__file__).parent
REPO = HERE.parent
CSV = REPO / "results" / "compiler_benchmark" / "compiler_pipeline_benchmark.csv"
FIGDIR = HERE / "figs"
FIGDIR.mkdir(exist_ok=True)

VIOLET = "#6D5BB5"
SKY = "#2563EB"
TEAL = "#0D9488"
AMBER = "#F59E0B"
INK = "#1E293B"
SLATE = "#64748B"
PAPER = "#FAFBFC"
HAIR = "#E2E8F0"
GREEN = "#16A34A"

PHASE_ORDER = ["lex", "parse", "semantic", "codegen"]
PHASE_LABELS = {
    "lex": "Lexer",
    "parse": "Parser",
    "semantic": "Semantic analysis",
    "codegen": "LLVM IR generation",
}
PHASE_COLOURS = {
    "lex": VIOLET,
    "parse": SKY,
    "semantic": TEAL,
    "codegen": AMBER,
}


def load_data(csv_path: pathlib.Path) -> pd.DataFrame:
    if csv_path.exists():
        df = pd.read_csv(csv_path)
    else:
        print(f"[INFO] CSV not found at {csv_path}; using recorded thesis values.")
        rows = [
            ("compiler flotation", "programs/flotation_tank.st", "lex", "pass", 184.8, 211),
            ("compiler flotation", "programs/flotation_tank.st", "parse", "pass", 248.8, 1),
            ("compiler flotation", "programs/flotation_tank.st", "semantic", "pass", 89.7, 0),
            ("compiler flotation", "programs/flotation_tank.st", "codegen", "pass", 326.4, 6945),
            ("compiler control_flow", "programs/control_flow.st", "lex", "pass", 187.6, 279),
            ("compiler control_flow", "programs/control_flow.st", "parse", "pass", 290.1, 2),
            ("compiler control_flow", "programs/control_flow.st", "semantic", "pass", 53.6, 0),
            ("compiler control_flow", "programs/control_flow.st", "codegen", "pass", 190.7, 10028),
        ]
        df = pd.DataFrame(
            rows,
            columns=["benchmark", "source", "phase", "status", "elapsed_us", "items"],
        )
    df["phase"] = df["phase"].str.replace(" ", "", regex=False).str.lower()
    return df


df_all = load_data(CSV)
df = df_all[df_all["benchmark"] == "compiler flotation"].copy()
df["phase"] = pd.Categorical(df["phase"], categories=PHASE_ORDER, ordered=True)
df = df.sort_values("phase")

total_us = float(df["elapsed_us"].sum())
total_ms = total_us / 1000.0
all_pass = bool((df["status"].str.lower() == "pass").all())

print("=" * 68)
print("  SD-PLC compiler pipeline - flotation_tank.st")
print("=" * 68)
for _, row in df.iterrows():
    print(f"  {PHASE_LABELS[row['phase']]:<24} {row['status']:<5} {row['elapsed_us']:>8.1f} us")
print(f"  {'Total compile time':<24} {'':<5} {total_us:>8.1f} us = {total_ms:.3f} ms")
print()
print("CODESYS acceptance comparison")
print("  Same source accepted by CODESYS and SD-PLC.")
print("  Both report 11 declared variables with matching elementary types.")
print("  No syntax or semantic mismatch is recorded for the case study.")
print("=" * 68)

fig, axes = plt.subplots(
    1,
    2,
    figsize=(12, 5.2),
    facecolor=PAPER,
    gridspec_kw={"width_ratios": [1.2, 1]},
)
fig.suptitle(
    "SD-PLC compiler evidence - flotation_tank.st",
    fontsize=13,
    fontweight="bold",
    color=INK,
    y=1.01,
)

ax = axes[0]
ax.set_facecolor("white")
phases = [PHASE_LABELS[p] for p in PHASE_ORDER]
times = [float(df.loc[df["phase"] == p, "elapsed_us"].iloc[0]) for p in PHASE_ORDER]
colours = [PHASE_COLOURS[p] for p in PHASE_ORDER]
bars = ax.barh(phases[::-1], times[::-1], color=colours[::-1], height=0.55, zorder=3)

for bar, t in zip(bars, times[::-1]):
    ax.text(
        t + 5,
        bar.get_y() + bar.get_height() / 2,
        f"{t:.1f} us",
        va="center",
        ha="left",
        fontsize=10,
        color=INK,
    )

ax.axvline(total_us, linestyle="--", color=AMBER, linewidth=1.5, zorder=4)
ax.text(
    total_us + 5,
    -0.55,
    f"Total: {total_ms:.3f} ms",
    color=AMBER,
    fontsize=10,
    fontweight="bold",
    va="bottom",
)
ax.set_xlabel("Elapsed time (us)", fontsize=11, color=SLATE)
ax.set_xlim(0, total_us * 1.35)
ax.set_title("A  Compiler stages", fontsize=11, color=INK, pad=8)
ax.tick_params(colors=SLATE)
ax.grid(axis="x", color=HAIR, linestyle="--", linewidth=0.6, zorder=0)
for spine in ax.spines.values():
    spine.set_color(HAIR)

ax2 = axes[1]
ax2.set_facecolor("white")
ax2.axis("off")
ax2.set_title("B  Evidence summary", fontsize=11, color=INK, pad=8)

summary_rows = [
    ["Compiler phases", "All pass" if all_pass else "Check required"],
    ["CODESYS source acceptance", "Match"],
    ["Declared variables", "11 vs 11"],
    ["Elementary types", "REAL / BOOL / LINT"],
    ["Semantic errors", "0 recorded"],
    ["LLVM IR output", "Generated"],
    ["Total compile time", f"{total_ms:.3f} ms"],
]

col_widths = [0.55, 0.45]
row_height = 0.09
x0, y0 = 0.02, 0.92

for r, row in enumerate(summary_rows):
    x = x0
    bg = "#F0EDFA" if r % 2 == 0 else "white"
    for c, (cell, width) in enumerate(zip(row, col_widths)):
        rect = mpatches.FancyBboxPatch(
            (x, y0 - r * row_height - row_height),
            width - 0.006,
            row_height - 0.006,
            boxstyle="round,pad=0.004",
            linewidth=0,
            facecolor=bg,
            transform=ax2.transAxes,
            clip_on=False,
        )
        ax2.add_patch(rect)
        ax2.text(
            x + width / 2,
            y0 - r * row_height - row_height / 2,
            cell,
            transform=ax2.transAxes,
            ha="center",
            va="center",
            fontsize=9.5,
            color=GREEN if (c == 1 and cell in {"All pass", "Match", "Generated"}) else INK,
            fontweight="bold" if c == 1 else "normal",
        )
        x += width

note = "SD-PLC vs CODESYS"


ax2.text(
    0.04,
    y0 - len(summary_rows) * row_height - 0.06,
    note,
    transform=ax2.transAxes,
    fontsize=8.8,
    color=SLATE,
    va="top",
    style="italic",
)

fig.tight_layout(pad=1.7)
out = FIGDIR / "fig_01_compiler.png"
fig.savefig(out, dpi=160, bbox_inches="tight", facecolor=PAPER)
print(f"\nSaved -> {out}")
plt.close(fig)
