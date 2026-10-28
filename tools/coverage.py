#!/usr/bin/env python3
"""Measure/check per-crate production-line floors and uncovered-line budgets."""
import argparse
import json
import math
from pathlib import Path

PACKAGES=("context-core","context-storage","context-workflows","context-auth","context-engine","context-process","context-mcp")

def measure(report,root):
    totals={name:{"lines":0,"covered":0} for name in PACKAGES}
    for data in report["data"]:
        for source in data["files"]:
            path=Path(source["filename"])
            try:relative=path.relative_to(root)
            except ValueError:continue
            parts=relative.parts
            if len(parts)>=4 and parts[0]=="crates" and parts[1] in totals and parts[2]=="src":name=parts[1]
            elif len(parts)>=2 and parts[0]=="src":name="context-mcp"
            else:continue
            summary=source["summary"]["lines"];totals[name]["lines"]+=summary["count"];totals[name]["covered"]+=summary["covered"]
    for value in totals.values():
        value["uncovered"]=value["lines"]-value["covered"]
        value["percent"]=100*value["covered"]/value["lines"] if value["lines"] else 0
    return totals

def check(totals,thresholds):
    failures=[]
    for name in PACKAGES:
        value=totals[name];floor=thresholds.get(name)
        if not floor or not value["lines"]:failures.append(name+": missing measured production code/budget");continue
        if value["percent"]<floor["minimum_percent"]:failures.append(name+": line floor regressed")
        if value["uncovered"]>floor["maximum_uncovered"]:failures.append(name+": uncovered-line budget exceeded")
    return failures

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("report",type=Path);parser.add_argument("--thresholds",type=Path,default=Path("tools/coverage-thresholds.json"));parser.add_argument("--write-baseline",action="store_true");args=parser.parse_args()
    root=Path(__file__).resolve().parents[1];totals=measure(json.loads(args.report.read_text()),root)
    for name,value in totals.items():print(f"{name}: {value['percent']:.2f}% ({value['covered']}/{value['lines']} lines), {value['uncovered']} uncovered")
    if args.write_baseline:
        if any(not v["lines"] for v in totals.values()):raise SystemExit("Cannot baseline unmeasured crates")
        values={name:{"minimum_percent":math.floor(v["percent"]),"maximum_uncovered":v["uncovered"]+max(5,math.ceil(v["lines"]*0.002))} for name,v in totals.items()}
        args.thresholds.write_text(json.dumps(values,indent=2)+"\n")
    failures=check(totals,json.loads(args.thresholds.read_text()))
    if failures:raise SystemExit("\n".join(failures))

if __name__=="__main__":main()
