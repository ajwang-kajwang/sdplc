import csv, re, collections

with open("codesys_flotation_10ms_raw.csv", "r") as f:
    lines = [l.strip() for l in f.readlines()]

data = collections.defaultdict(dict)
current_var = None

for line in lines:
    m = re.match(r'^\d+\.Variable;\s+PLC_PRG\.(\w+)', line)
    if m:
        current_var = m.group(1)
        continue
    m = re.match(r'^;\s+([\d.]+);\s+([\d.eE+\-]+)', line)
    if m and current_var:
        ts = round(float(m.group(1)))
        val = m.group(2)
        data[ts][current_var] = val

timestamps = sorted(data.keys())
variables = ["cycle","level","target_level","air_flow","feed_flow",
             "tailings_flow","concentrate_grade","motor_running",
             "emergency_stop","high_level","low_air"]

with open("codesys_flotation_10ms.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["cycle_index"] + variables)
    for i, ts in enumerate(timestamps, 1):
        row = [i] + [data[ts].get(v, "") for v in variables]
        w.writerow(row)

print(f"Written {len(timestamps)} rows")