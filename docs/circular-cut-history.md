# §26E — bounded circular Cut history

## Policy recorded before implementation

One reader validates the complete plate/Body/linear predecessor graph and builds
one ordered catalogue for add, edit and discovery on the caller's read-only
snapshot. Membership comes only from Body.tip and Feature.previous, checked
against the complete object and dependency sets. Names, rowids and ordinals
never identify a link. Every payload must round-trip losslessly. The supported
editor boundary is 0–16 forward Blind circular Cuts of one unconstrained,
axis-aligned XY rectangular NewBody plate; 16 can be edited but cannot be
extended. Longer documents remain readable under the general document rules
and receive an explicit editor refusal, never a truncated catalogue.

Every disk is checked against every other disk using the existing strict
WALL_CLEARANCE_MM policy, with the conflicting feature UUID in the refusal.
Depth is measured from base XY along +Z. No constraints, touching/overlapping/
nested disks, attachment, placement, ThroughAll or arbitrary history are added.

Historical references retain their UUID, owner, producer, role and selection.
The first Cut retains its legacy Carried base names. Every later producer uses
OriginCap/OriginSide for all base and ancestor faces, through real OCCT history;
one supported name per surface per producer. No final/historical producer alias
is introduced. Existing archive v3 and predecessor-qualified keys must prove
the full chain without geometric or ordinal matching.

Through→pocket at link i adds one own ExtrudeCap(End), plus one
OriginCap(i, End) at **each** subsequent producer. Repeat and same-kind edits
add nothing. Pocket→through refuses during preparation with all protected
floor UUIDs across producer scopes; strict rebuild resolution remains a second
guard. Writer re-derives the complete proposal from current payload numbers
inside its transaction and refuses new-ID collisions and duplicates.

SQL allowlists and the existing copy job stay in force: edit changes only the
selected Sketch/Cut payload/hash, modified_at and the permitted new refs; add
inserts the existing two objects and edges/refs and moves the existing Body tip.
All other cells, source bytes, metadata, capabilities/rowids and extra tables
remain intact. Snapshot/version guard, cold rebuilds, strict refs, cancellation,
handle release, SQLite close, no-clobber/alias checks, cleanup, atomic
publication and exit 7 retain their existing meanings.

Discovery adds `tools`, an explicit array ordered from first Cut to tip, to
both add target and saved edit DTOs. It includes the selected tool for edit.
Old singular `existing_cut` remains the sole Cut for N=1 and null for N=0 or
N>1. `neighboring_tool` remains the other Cut for N=2 and null otherwise.
`base_feature_id` always names NewBody; `previous_feature_id` always names the
selected Cut's immediate predecessor. Request v1, JSON v1 and exits stay put.
UI and CLI submit the same numeric request through the same worker/job and
must produce equivalent geometry, refs and exports. Automation reads JSON.

Long choices and tool details use bounded scrolling so edit controls remain
accessible. One shared form/worker preserves exact saved f64 on first Undo,
one Apply per step, draft on Cancel/failure/stale completion/failed async Open.

Verification and executed recipe will be recorded separately; prior CI is
evidence of the base only, not evidence for this uncommitted diff.

## Executable agent recipe

Set `FERRITECAD` to the fresh CLI and `FCAD_UFBX_READER` to the pinned
`read_production` reader. Extract the following Python block by its marker.
It creates 16 Cuts, edits first/middle/last, compares every SQL cell under the
edit allowlist, checks independent binary STL geometry and reads six small
FBX files. It prints representative 2/8/16 timings and macOS child peak RSS
without timing assertions. Native B-Rep, archive and mutation proofs are
separate gates in the verification report.

From the repository root, extraction is literal (the marker is a delimiter):

```sh
python3 - <<'PY'
from pathlib import Path
text = Path("docs/circular-cut-history.md").read_text()
code = text.split("# FCAD_26E_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("/private/tmp/ferrite-cut-history-recipe.py").write_text(code)
PY
python3 /private/tmp/ferrite-cut-history-recipe.py
```

```python
# FCAD_26E_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile, sqlite3, time, sys, re
metrics=[]
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-cut-history-"))
WIDTH, DEPTH, HEIGHT = 80.0, 50.0, 12.0
DEFLECTION, ANGULAR = 0.05, 0.1

def run(args, code=0):
    started=time.perf_counter()
    command=[cli,*map(str,args)]
    if sys.platform=="darwin": command=["/usr/bin/time","-l",*command]
    p = subprocess.run(command, capture_output=True, text=True)
    rss=re.search(r"(\d+)\s+maximum resident set size",p.stderr)
    metrics.append((args[0],time.perf_counter()-started,int(rss.group(1))/1048576 if rss else None))
    assert p.returncode == code, (args,p.returncode,p.stdout,p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def measure(path, tools):
    """Everything the mesh must be for the part to really have that cut."""
    stl = path.with_suffix(".stl")
    report = run(["export-stl", str(path), "-o", str(stl),
                  "--linear-deflection", str(DEFLECTION),
                  "--angular-deflection", str(ANGULAR), "--json"])
    raw = stl.read_bytes()
    n = struct.unpack_from("<I", raw, 80)[0]
    assert len(raw) == 84 + 50 * n and report["result"]["triangles"] == n
    tris, six = [], 0.0
    for i in range(n):
        off = 84 + 50 * i + 12
        t = [list(struct.unpack_from("<fff", raw, off + 12 * k)) for k in range(3)]
        tris.append(t)
        a, b, c = t
        six += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
    volume = six / 6.0

    # Closed and wound one way.
    q = lambda v: tuple(round(x, 4) for x in v)
    directed = {}
    for t in tris:
        p = [q(v) for v in t]
        for k in range(3):
            directed[(p[k], p[(k + 1) % 3])] = directed.get((p[k], p[(k + 1) % 3]), 0) + 1
    assert all(v == 1 for v in directed.values()), "not one oriented surface"
    assert all((b, a) in directed for (a, b) in directed), "the mesh is open"

    for CENTER, RADIUS, depth in tools:
        # The bore wall: on the tool's circle and spanning the depth. A facet of a
        # pocket floor also has its corners on that circle, which is why "wall"
        # means the part of it that is not flat.
        def on_bore(t):
            return all(abs(math.hypot(v[0] - CENTER[0], v[1] - CENTER[1]) - RADIUS)
                       < DEFLECTION + 1e-4 for v in t)
        def spans(t):
            zs = [v[2] for v in t]
            return max(zs) - min(zs) > 1e-4
        wall = [t for t in tris if on_bore(t) and spans(t)]
        assert len(wall) >= 12, "no bore wall"
        def radial(t):
            a, b, c = t
            u = [b[i] - a[i] for i in range(3)]
            v = [c[i] - a[i] for i in range(3)]
            nx = u[1] * v[2] - u[2] * v[1]
            ny = u[2] * v[0] - u[0] * v[2]
            mx = (a[0] + b[0] + c[0]) / 3 - CENTER[0]
            my = (a[1] + b[1] + c[1]) / 3 - CENTER[1]
            return nx * mx + ny * my
        assert all(radial(t) < 0 for t in wall), "a boss, not a hole"

        zs = sorted(v[2] for t in wall for v in t)
        assert abs(zs[0]) < 1e-4 and abs(zs[-1] - depth) < 1e-4, (zs[0], zs[-1])

        # Through or not, said by the mesh rather than by the request.
        def covers(t, z):
            if any(abs(v[2] - z) > 1e-4 for v in t):
                return False
            (x1, y1), (x2, y2), (x3, y3) = [(v[0], v[1]) for v in t]
            d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
            if abs(d) < 1e-12:
                return False
            a = ((y2 - y3) * (CENTER[0] - x3) + (x3 - x2) * (CENTER[1] - y3)) / d
            b = ((y3 - y1) * (CENTER[0] - x3) + (x1 - x3) * (CENTER[1] - y3)) / d
            return a >= -1e-9 and b >= -1e-9 and 1 - a - b >= -1e-9
        if depth < HEIGHT:
            assert any(covers(t, depth) for t in tris), "a pocket has a floor"
            assert any(covers(t, HEIGHT) for t in tris), "and a closed far side"
        else:
            assert not any(covers(t, HEIGHT) for t in tris), "a hole is open"

    lo = [min(v[j] for t in tris for v in t) for j in range(3)]
    hi = [max(v[j] for t in tris for v in t) for j in range(3)]
    assert [round(v, 4) for v in lo] == [0.0, 0.0, 0.0]
    assert [round(v, 4) for v in hi] == [WIDTH, DEPTH, HEIGHT]

    # A mesh bore is inscribed, so it removes a little less than the exact
    # cylinder. Bounded on both sides rather than compared to an analytic pi.
    exact = WIDTH * DEPTH * HEIGHT - sum(math.pi*r*r*d for _,r,d in tools)
    inscribed = WIDTH * DEPTH * HEIGHT - sum(math.pi*(r-DEFLECTION)**2*d for _,r,d in tools)
    assert exact - 1e-3 <= volume <= inscribed + 1e-3, volume
    return volume


def cut(source, destination, tool, code=0):
    catalog=run(["inspect",source,"--json"])["result"]
    body=catalog["bodies"][0]
    request=root/"cut.json"
    center,radius,depth=tool
    request.write_text(json.dumps({"request_version":1,"center_mm":center,"radius_mm":radius,"depth_mm":depth}))
    return run(["cut-circular-copy",source,"--body",body["body_id"],"--expect-version",catalog["content_version"],"--request",request,"-o",destination,"--json"],code)

def edit(source, destination, selected, tool, code=0):
    cat=run(["inspect",source,"--json"])["result"]
    saved=next(f["circular_cut_edit"]["saved"] for f in cat["features"] if f["feature_id"]==selected)
    request=root/"edit.json"
    c,r,d=tool
    request.write_text(json.dumps({"request_version":1,"tool_curve_id":saved["tool_curve_id"],"center_mm":c,"radius_mm":r,"depth_mm":d}))
    result=run(["edit-circular-cut",source,"--feature",selected,"--expect-version",cat["content_version"],"--request",request,"-o",destination,"--json"],code)
    if code==0:
        assert result["result"]["previous_feature_id"]==saved["previous_feature_id"]
        assert result["result"]["feature_id"]==selected
    return result

def sql(path):
    with sqlite3.connect(path) as c:
        data={}
        for name, in c.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name"):
            quoted='"'+name.replace('"','""')+'"'
            try: cur=c.execute(f"SELECT rowid,* FROM {quoted} ORDER BY rowid")
            except sqlite3.OperationalError: cur=c.execute(f"SELECT * FROM {quoted} ORDER BY 1,2")
            data[name]=([x[0] for x in cur.description],cur.fetchall())
        return data

def preserved(source, copy, saved, added):
    a,b=sql(source),sql(copy)
    assert a.keys()==b.keys()
    import uuid
    ids=[uuid.UUID(saved[k]).bytes for k in ["feature_id","tool_sketch_id"]]
    for name,(cols,rows) in a.items():
        assert cols==b[name][0]
        now=b[name][1]
        if name=="topology_refs":
            assert len(now)==len(rows)+added and all(row in now for row in rows)
            continue
        assert len(rows)==len(now)
        for old,new in zip(rows,now):
            for i,(x,y) in enumerate(zip(old,new)):
                assert x==y or (name=="meta" and cols[i]=="modified_at") or (name=="objects" and old[cols.index("id")] in ids and cols[i] in ["payload","payload_hash"]),(name,cols[i])

create=root/"plate.json"
create.write_text(json.dumps({"request_version":1,"points_mm":[[0,0],[WIDTH,0],[WIDTH,DEPTH],[0,DEPTH]],"height_mm":HEIGHT}))
source=root/"plate.fcad"
run(["create-sketch-extrude",create,"-o",source,"--json"])
tools=[]
for i in range(16):
    slot=(i*7)%16
    tools.append(([10.+slot%4*19.,7.+slot//4*12.],1.5+i%5*.25,12. if i%2==0 else 3.+i%7))
    dest=root/f"cuts-{i+1}.fcad"
    before=source.read_bytes()
    cut(source,dest,tools[-1])
    assert source.read_bytes()==before
    if i+1 in [2,8,16]: print("PERF",i+1,"add",metrics[-1])
    source=dest
    if i+1 not in [2,3,4,8,16]: continue
    cat=run(["inspect",source,"--json"])["result"]
    if i+1 in [2,8,16]: print("PERF",i+1,"inspect",metrics[-1])
    choices=[f["circular_cut_edit"]["saved"] for f in cat["features"] if f["circular_cut_edit"]["available"]]
    assert len(choices)==i+1
    ordered=next(c for c in choices if c["feature_id"]==c["tip_feature_id"])["tools"]
    assert [t["center_mm"] for t in ordered]==[t[0] for t in tools]
    assert cat["bodies"][0]["cut_edit"]["available"]==(i+1<16)
    original=source.read_bytes()
    for index in sorted({0,(i+1)//2,i}):
        saved=next(c for c in choices if c["feature_id"]==ordered[index]["feature_id"])
        c,r,d=tools[index]
        changed=list(tools); changed[index]=([c[0]+.375,c[1]-.25],r+.125,2.75)
        copy=root/f"edit-{i+1}-{index}.fcad"
        edit(source,copy,saved["feature_id"],changed[index])
        if i+1 in [2,8,16] and index==0: print("PERF",i+1,"edit-first",metrics[-1])
        preserved(source,copy,saved,(i+1-index) if d==HEIGHT else 0)
        measure(copy,changed)
        assert source.read_bytes()==original
        if i+1 in [3,4]:
            fbx=copy.with_suffix(".fbx")
            run(["export-fbx",copy,"-o",fbx,"--json"])
            p=subprocess.run([reader,"--identity",str(fbx)],capture_output=True,text=True)
            assert p.returncode==0 and "checks=6 failures=0" in p.stdout,(p.stdout,p.stderr)
    before=source.read_bytes()
    never=root/"never.fcad"
    error=edit(source,never,ordered[0]["feature_id"],(tools[-1][0],tools[0][1],2.),2)
    assert ordered[-1]["feature_id"] in error["error"]["message"] and not never.exists()
    assert source.read_bytes()==before
error=cut(source,root/"never.fcad",([40.,25.],1.,2.),2)
assert "16" in error["error"]["message"] and not (root/"never.fcad").exists()
print("FCAD_26E_RECIPE_OK",root)
```
