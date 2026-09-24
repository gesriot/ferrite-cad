# §26G — edit the base height of a circular Cut history

## Policy recorded before implementation

The existing `edit-extrude --distance-mm` and Edit extrusion form edit the
constant Blind height of the original NewBody in one validated 1–16 circular
Cut history. The complete shared CutHistory reader decides support from objects,
edges, ownership, lossless payloads and names. A malformed or unsupported history
cannot fall back to the legacy standalone path. No new command, feature, schema,
boolean, tool scaling or copier is introduced. Standalone edits keep their
existing contract and restrictions.

The requested height is finite and positive, as before. In a Cut history it
also satisfies the existing PolygonExtrusion height bound and the shared Cut
policy for **every** unchanged absolute tool. Depth must be `<= height` exactly;
floor classification remains `depth < height` exactly. There is no new epsilon
or snapping. The existing wall/disk clearance remains strictly greater than
`WALL_CLEARANCE_MM = Tolerance::DEFAULT_LINEAR = 1e-7 mm`.

Pocket→pocket and through→through add no names. Through→pocket adds exactly one
own `ExtrudeCap(End)` and one `OriginCap(origin Cut, End)` at each descendant
producer, using the same transition/name rules as editing a Cut. Every old and
new reference must resolve through real OCCT history after cold rebuild. A
height that removes a saved pocket floor refuses during preparation with its
Cut and protected reference UUIDs; refs are never removed or reassigned. A tool
deeper than the new thickness refuses with its Cut UUID. All tools are checked
before any new reference UUID is minted.

The writer re-reads the original row and the complete current history inside
the writing transaction, re-derives payload and added refs from the proposed
height, and rejects forgeries, stale history and duplicate/cross-domain UUIDs.
SQL changes are limited to the base Extrude payload/hash, necessary new refs
and the existing `meta.modified_at` policy; capabilities/rowids and all other
cells remain unchanged. Source bytes, Sketches, tool numbers/UUIDs, graph and
Body tip stay intact. The common copy job keeps strict baseline/old/new refs,
source/version/alias guards, cancellation, cleanup, close and Keep publication.

Discovery supplies typed height context on the same pinned snapshot. UI uses
the domain validator, explains fixed absolute Cut depths, retains input after
Save Cancel/refusal/stale reply, and restores it after failed async Open through
the existing completion/scene-acceptance path. A successful reopened copy stays
editable by Edit Cut/Edit Sketch and Add Cut below the existing limit.

[Executed verification and limitations](edit-cut-base-height-verification.md).

## Discovery and publication

`features[].base_height_edit` is null for standalone or unsupported features.
For the supported original base it contains `body_id`, `profile_sketch_id`,
`extents_mm`, history-ordered `tools` (the existing Cut tool DTO), and
`protected_floors`: `{feature_id, reference_ids}` for each Cut, including its
own and descendant floor references. Empty IDs mean the Cut currently has no
saved floor. The existing feature UUID, `editable`, distance and refusal keep
meaning height editing; the document refusal still takes priority. This is
constraint context, not a new request.

Request/response/operation v1 and exits 0/2/7 are unchanged. Exit 2 publishes
nothing. Exit 7 can mean that the copy was published but its report was lost:
inspect the destination and its version before deciding anything; do not retry
blindly. Native execution still constructs the kernel before the edit job, so
an unavailable-kernel refusal may precede request-domain checks in a stub CLI.
Kernel-free discovery and document preparation do not require a kernel.

## Executable agent recipe

The marked block creates fresh 1/2/4/16-Cut sources with the real CLI. It selects
UUID/version from JSON, changes height, checks all SQL cells, reopens and performs
validate/cold/topology checks, parses STL independently, and reads three small
FBXs with pinned ufbx. It also exercises a protected-floor refusal and a real
lost stdout after publication. No human-readable output is parsed for identity
or control flow. B-Rep/cache/writer mutations have separate native gates.

```sh
python3 - <<'EXTRACT'
from pathlib import Path
text = Path("docs/edit-cut-base-height.md").read_text()
code = text.split("# FCAD_26G_AGENT_RECIPE\n", 1)[1].split("\n```", 1)[0]
Path("/private/tmp/ferrite-cut-height-recipe.py").write_text(code)
EXTRACT
FERRITECAD=/path/to/staged/ferritecad FCAD_UFBX_READER=/path/to/read_production \
  python3 /private/tmp/ferrite-cut-height-recipe.py
```

```python
# FCAD_26G_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile, sqlite3, uuid
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-cut-height-"))
DEFLECTION, ANGULAR = 0.05, 0.1

def run(args, code=0):
    p = subprocess.run([cli,*map(str,args)],capture_output=True,text=True)
    if p.returncode == 7:
        raise RuntimeError("Report lost: inspect destination before any further action; do not retry blindly")
    assert p.returncode == code,(args,p.returncode,p.stdout,p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def base(path):
    catalog = run(["inspect",path,"--json"])["result"]
    selected = [f for f in catalog["features"] if f["base_height_edit"] is not None]
    assert len(selected)==1 and selected[0]["editable"]
    return catalog, selected[0]

def edit(source, destination, height, code=0):
    catalog, selected = base(source)
    return run(["edit-extrude",source,"--feature",selected["feature_id"],
                "--expect-version",catalog["content_version"],"--distance-mm",height,
                "-o",destination,"--json"],code)

def measure(path, tools, height):
    bounds = [[0.,0.],[80.,50.]]
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
        if depth < height:
            assert any(covers(t, depth) for t in tris), "a pocket has a floor"
            assert any(covers(t, height) for t in tris), "and a closed far side"
        else:
            assert not any(covers(t, height) for t in tris), "a hole is open"

    lo = [min(v[j] for t in tris for v in t) for j in range(3)]
    hi = [max(v[j] for t in tris for v in t) for j in range(3)]
    assert [round(v, 4) for v in lo] == [*bounds[0], 0.0]
    assert [round(v, 4) for v in hi] == [*bounds[1], height]

    # A mesh bore is inscribed, so it removes a little less than the exact
    # cylinder. Bounded on both sides rather than compared to an analytic pi.
    exact = plate_width * plate_depth * height - sum(math.pi*r*r*d for _,r,d in tools)
    inscribed = plate_width * plate_depth * height - sum(math.pi*(r-DEFLECTION)**2*d for _,r,d in tools)
    assert exact - 1e-3 <= volume <= inscribed + 1e-3, volume
    return volume

def sql(path):
    with sqlite3.connect(path) as c:
        data={}
        for name, in c.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name"):
            quoted='"'+name.replace('"','""')+'"'
            try: cur=c.execute(f"SELECT rowid,* FROM {quoted} ORDER BY rowid")
            except sqlite3.OperationalError: cur=c.execute(f"SELECT * FROM {quoted} ORDER BY 1,2")
            data[name]=([x[0] for x in cur.description],cur.fetchall())
        return data

def preserved(source, copy, feature, expected_added):
    a,b=sql(source),sql(copy)
    assert a.keys()==b.keys()
    selected=uuid.UUID(feature).bytes
    for name,(cols,rows) in a.items():
        assert cols==b[name][0]
        now=b[name][1]
        if name=="topology_refs":
            assert len(now)==len(rows)+expected_added and all(r in now for r in rows)
            continue
        assert len(rows)==len(now)
        for old,new in zip(rows,now):
            for i,(x,y) in enumerate(zip(old,new)):
                assert x==y or (name=="meta" and cols[i]=="modified_at") or (name=="objects" and old[cols.index("id")]==selected and cols[i] in ["payload","payload_hash"]),(name,cols[i])

def verified(path, height):
    catalog,selected=base(path)
    assert selected["distance_mm"]==height
    tools=selected["base_height_edit"]["tools"]
    assert sum(f["circular_cut_edit"]["available"] for f in catalog["features"])==len(tools)
    assert any(s["editable"] for s in catalog["sketches"])
    assert catalog["bodies"][0]["cut_edit"]["available"]==(len(tools)<16)
    valid=run(["validate",path,"--json"])["result"]
    assert valid["valid"] and valid["errors"]==valid["warnings"]==0
    run(["rebuild",path,"--cold"])
    run(["print-topology",path])
    measure(path,[(t["center_mm"],t["radius_mm"],t["depth_mm"]) for t in tools],height)
    return selected

def plate(name):
    request=root/(name+".json")
    request.write_text(json.dumps({"request_version":1,"points_mm":[[0,0],[80,0],[80,50],[0,50]],"height_mm":12}))
    out=root/(name+".fcad")
    run(["create-sketch-extrude",request,"-o",out,"--json"])
    return out

def cut(source, i, pocket=False):
    catalog=run(["inspect",source,"--json"])["result"]
    slot=(i*7)%16
    request=root/"cut.json"
    request.write_text(json.dumps({"request_version":1,"center_mm":[10.+slot%4*19.,7.+slot//4*12.],"radius_mm":1.5+i%5*.25,"depth_mm":3.+i%7 if pocket or i%2 else 12.}))
    out=root/(source.stem+"-cut.fcad")
    run(["cut-circular-copy",source,"--body",catalog["bodies"][0]["body_id"],"--expect-version",catalog["content_version"],"--request",request,"-o",out,"--json"])
    return out

small=[]
source=plate("mixed")
for i in range(16):
    before=source.read_bytes(); destination=cut(source,i)
    assert source.read_bytes()==before
    source=destination
    if i+1 not in [1,2,4,16]: continue
    catalog,selected=base(source); context=selected["base_height_edit"]
    before=source.read_bytes(); grown=root/f"grown-{i+1}.fcad"
    edit(source,grown,14)
    floors={p["feature_id"]:p["reference_ids"] for p in context["protected_floors"]}
    expected=sum(len(context["tools"])-j for j,t in enumerate(context["tools"]) if not floors[t["feature_id"]])
    preserved(source,grown,selected["feature_id"],expected)
    now=verified(grown,14)
    assert [(t["feature_id"],t["center_mm"],t["radius_mm"],t["depth_mm"]) for t in context["tools"]]==[(t["feature_id"],t["center_mm"],t["radius_mm"],t["depth_mm"]) for t in now["base_height_edit"]["tools"]]
    assert source.read_bytes()==before
    never=root/"never.fcad"
    error=edit(source,never,10,2)["error"]
    assert context["tools"][0]["feature_id"] in error["message"] and not never.exists()
    protected=now["base_height_edit"]["protected_floors"][0]
    error=edit(grown,never,12,2)["error"]
    assert protected["feature_id"] in error["message"]
    assert all(ref in error["message"] for ref in protected["reference_ids"])
    assert not never.exists()
    if i+1 in [2,4]: small.append(grown)

pockets=plate("pockets")
for i in range(4): pockets=cut(pockets,i,True)
_,selected=base(pockets); before=pockets.read_bytes()
shorter=root/"shorter.fcad";edit(pockets,shorter,10)
preserved(pockets,shorter,selected["feature_id"],0);verified(shorter,10)
assert pockets.read_bytes()==before
small.append(shorter)
for path in small:
    fbx=path.with_suffix(".fbx");run(["export-fbx",path,"-o",fbx,"--json"])
    p=subprocess.run([reader,"--identity",str(fbx)],capture_output=True,text=True)
    assert p.returncode==0 and "checks=6 failures=0" in p.stdout,(p.stdout,p.stderr)

# Exercise exit 7 deliberately. Inspection is safe; a second edit is not issued.
catalog,selected=base(source); delivered=root/"lost-report.fcad"
p=subprocess.Popen([cli,"edit-extrude",str(source),"--feature",selected["feature_id"],"--expect-version",catalog["content_version"],"--distance-mm","14","-o",str(delivered),"--json"],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
p.stdout.close();stderr=p.stderr.read();assert p.wait()==7,stderr
assert delivered.exists();verified(delivered,14)
print("FCAD_26G_RECIPE_OK",root)
```
