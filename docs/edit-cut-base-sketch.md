# §26F — coordinates of the original Sketch in a circular Cut history

`Edit Sketch <name> — <UUID>…` and `edit-sketch-copy` accept the original
unconstrained, four-Line axis-aligned XY rectangle of the single Body whose
complete 1–16 Cut history satisfies [§26E](circular-cut-history.md). Zero Cuts
uses the unchanged standalone 3–256 Line polygon contract. No new command,
feature, boolean, schema, capability, reference or history limit is introduced.

The request is still v1: every saved `curve_id` exactly once in stored order,
with new `start_mm`. Each end becomes the next start. Exact saved closure and
winding follow [the polygon contract](edit-sketch-copy.md); no sorting, contour
canonicalisation, implicit vertex movement or regenerated UUID occurs. A changed
base must satisfy polygon numeric bounds and the existing rectangle policy.
Every saved disk must clear every new wall by **more than** `WALL_CLEARANCE_MM`.
A refusal names the offending Cut UUID. All tools retain their absolute base-XY
centres, radii and depths. Moving a boundary does not move a hole. Height stays put.

The one `CutHistory` reader checks the complete object/edge set, linear ownership,
lossless payloads, tool policy and saved names. Its pinned catalogue supplies
Sketch, Add Cut and Edit Cut discovery without a traversal for each row. The
shared four-object `sketch_edit::frame` remains unchanged, so circle, annulus and
constraint editors still refuse a history. NewBody's operation-specific Cut-edit
refusal still precedes history errors.

Discovery adds `sketches[].cut_history`: null for standalone or unsupported
Sketches; otherwise `{body_id, base_feature_id, tools, wall_clearance_mm}`.
`tools` uses the existing ordered Cut tool DTO (feature/tool Sketch/curve UUIDs,
absolute `center_mm`, `radius_mm`, `depth_mm`), from first Cut to tip. This is
explicit context for the coordinate policy, not a request or a second API.
Existing `vertices`, `editable`, refusal priorities, operation, response v1 and
exit 0/2/7 retain their meanings.

Draft validation and preparation use the same document-domain coordinate check.
The writer extracts request numbers from the prepared payload and re-derives the
whole permitted payload against the current document **inside its transaction**.
Forgery and intervening tool edits refuse. Only the base Sketch's payload/hash
are updated; even `meta.modified_at` retains the legacy coordinate editor's
value. No capabilities/rowids, object/curve/ref UUIDs, edges, Cut/Body payloads,
heights, topology rules, additional tables or source bytes change.

The existing copy job owns backup, pinned source/version/alias guards, cold
baseline and edited rebuilds, strict resolution of every saved ref, cleanup,
close and atomic Keep publication. Native base, Carried and Origin refs use real
OCCT history. Editing the base invalidates the base and all descendants in an
old sidecar; the next rebuild hits all. There is no geometry/proximity fallback.

The form shows that tools stay fixed, retains invalid intermediate drafts, and
keeps existing selection/drag/Undo/Redo/Save behavior. The action list scrolls
within a bounded area. Line-edit completion now uses the existing published-draft
retention so failed async Open restores the draft, as other profile editors do.
No constraints in history, tool editing through this form, overlap/tangency,
reordering/suppression/deletion, arbitrary planes/solids, attachment, live geometry
preview or in-place Save is added. `edit-extrude` is unchanged.

[Verification and limitations](edit-cut-base-sketch-verification.md).

## Executable agent recipe

Use the fresh native CLI and pinned `read_production` ufbx reader. This block
creates 16 through/pocket Cuts and takes every target UUID/version from discovery.
It expands, shrinks and shifts boundaries at 2/4/16 Cuts, compares every SQL cell,
reads STL independently and checks three small FBXs. B-Rep/cache and mutations
are separate native gates.

```sh
python3 - <<'PY'
from pathlib import Path
text = Path("docs/edit-cut-base-sketch.md").read_text()
code = text.split("# FCAD_26F_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("/private/tmp/ferrite-cut-base-recipe.py").write_text(code)
PY
FERRITECAD=/path/to/ferritecad FCAD_UFBX_READER=/path/to/read_production \
  python3 /private/tmp/ferrite-cut-base-recipe.py
```

```python
# FCAD_26F_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile, sqlite3, uuid
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-cut-base-"))
HEIGHT, DEFLECTION, ANGULAR = 12.0, 0.05, 0.1

def run(args, code=0):
    p = subprocess.run([cli,*map(str,args)],capture_output=True,text=True)
    assert p.returncode == code,(args,p.returncode,p.stdout,p.stderr)
    return json.loads(p.stdout)

def measure(path, tools, bounds):
    """Everything the mesh must be for the part to really have that cut."""
    plate_width, plate_depth = [bounds[1][i] - bounds[0][i] for i in range(2)]
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
    assert [round(v, 4) for v in lo] == [*bounds[0], 0.0]
    assert [round(v, 4) for v in hi] == [*bounds[1], HEIGHT]

    # A mesh bore is inscribed, so it removes a little less than the exact
    # cylinder. Bounded on both sides rather than compared to an analytic pi.
    exact = plate_width * plate_depth * HEIGHT - sum(math.pi*r*r*d for _,r,d in tools)
    inscribed = plate_width * plate_depth * HEIGHT - sum(math.pi*(r-DEFLECTION)**2*d for _,r,d in tools)
    assert exact - 1e-3 <= volume <= inscribed + 1e-3, volume
    return volume

def cut(source, destination, tool, code=0):
    catalog=run(["inspect",source,"--json"])["result"]
    body=catalog["bodies"][0]
    request=root/"cut.json"
    center,radius,depth=tool
    request.write_text(json.dumps({"request_version":1,"center_mm":center,"radius_mm":radius,"depth_mm":depth}))
    return run(["cut-circular-copy",source,"--body",body["body_id"],"--expect-version",catalog["content_version"],"--request",request,"-o",destination,"--json"],code)
def sql(path):
    with sqlite3.connect(path) as c:
        data={}
        for name, in c.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name"):
            quoted='"'+name.replace('"','""')+'"'
            try: cur=c.execute(f"SELECT rowid,* FROM {quoted} ORDER BY rowid")
            except sqlite3.OperationalError: cur=c.execute(f"SELECT * FROM {quoted} ORDER BY 1,2")
            data[name]=([x[0] for x in cur.description],cur.fetchall())
        return data
def preserved(source, copy, sketch):
    a,b=sql(source),sql(copy)
    assert a.keys()==b.keys()
    selected=uuid.UUID(sketch).bytes
    changed=False
    for name,(cols,rows) in a.items():
        assert cols==b[name][0] and len(rows)==len(b[name][1])
        for old,new in zip(rows,b[name][1]):
            for i,(x,y) in enumerate(zip(old,new)):
                if x==y: continue
                assert name=="objects" and old[cols.index("id")]==selected and cols[i] in ["payload","payload_hash"],(name,cols[i])
                changed=True
    assert changed

create=root/"plate.json"
create.write_text(json.dumps({"request_version":1,"points_mm":[[0,0],[80,0],[80,50],[0,50]],"height_mm":HEIGHT}))
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
    source=dest
    if i+1 not in [2,4,16]: continue
    catalog=run(["inspect",source,"--json"])["result"]
    base=next(s for s in catalog["sketches"] if s["cut_history"] is not None)
    assert base["editable"] and len(base["cut_history"]["tools"])==i+1
    assert [t["center_mm"] for t in base["cut_history"]["tools"]]==[t[0] for t in tools]
    before=source.read_bytes()
    for case,bounds in enumerate([[[-4.,-3.],[86.,56.]],[[2.,2.],[73.,48.]],[[3.,1.],[83.,51.]]]):
        vertices=[{"curve_id":v["curve_id"],"start_mm":[bounds[int(v["start_mm"][a]!=0)][a] for a in range(2)]} for v in base["vertices"]]
        request=root/f"base-{i+1}-{case}.json"
        request.write_text(json.dumps({"request_version":1,"vertices":vertices}))
        copy=root/f"base-{i+1}-{case}.fcad"
        run(["edit-sketch-copy",source,"--sketch",base["sketch_id"],"--expect-version",catalog["content_version"],"--request",request,"-o",copy,"--json"])
        preserved(source,copy,base["sketch_id"])
        measure(copy,tools,bounds)
        assert source.read_bytes()==before
        if i+1==4:
            fbx=copy.with_suffix(".fbx")
            run(["export-fbx",copy,"-o",fbx,"--json"])
            p=subprocess.run([reader,"--identity",str(fbx)],capture_output=True,text=True)
            assert p.returncode==0 and "checks=6 failures=0" in p.stdout,(p.stdout,p.stderr)
    offending=max(base["cut_history"]["tools"],key=lambda t:t["center_mm"][0]+t["radius_mm"])
    right=offending["center_mm"][0]+offending["radius_mm"]
    bad=[{"curve_id":v["curve_id"],"start_mm":[right if v["start_mm"][0]==80 else v["start_mm"][0],v["start_mm"][1]]} for v in base["vertices"]]
    request=root/"refused.json"
    request.write_text(json.dumps({"request_version":1,"vertices":bad}))
    never=root/"never.fcad"
    error=run(["edit-sketch-copy",source,"--sketch",base["sketch_id"],"--expect-version",catalog["content_version"],"--request",request,"-o",never,"--json"],2)
    assert offending["feature_id"] in error["error"]["message"] and not never.exists()
    assert source.read_bytes()==before
print("FCAD_26F_RECIPE_OK",root)
```
