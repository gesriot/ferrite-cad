// Standalone OCCT smoke test: prove shared Open CASCADE works before any Rust
// binding depends on it. Every step catches Standard_Failure and std::exception;
// main never lets an exception escape. Exit code = number of failed steps.

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <iomanip>
#include <iostream>
#include <random>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

// OCCT — version
#include <Standard_Version.hxx>

// Exceptions
#include <Standard_Failure.hxx>

// Geometry / topology
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepTools.hxx>
#include <GProp_GProps.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Wire.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <gp_Ax2.hxx>
#include <gp_Dir.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>

// Mesh
#include <Poly_Triangulation.hxx>

// XDE / STEP
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Static.hxx>
#include <Quantity_Color.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <STEPControl_StepModelType.hxx>
#include <TCollection_AsciiString.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TDataStd_Name.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <TDocStd_Document.hxx>
#include <UnitsMethods.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_ColorType.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>

namespace {

using Clock = std::chrono::steady_clock;

class ScopedTempPath {
public:
  explicit ScopedTempPath(std::filesystem::path path) : path_(std::move(path)) {}

  ScopedTempPath(const ScopedTempPath&) = delete;
  ScopedTempPath& operator=(const ScopedTempPath&) = delete;

  ~ScopedTempPath()
  {
    std::error_code ignored;
    std::filesystem::remove(path_, ignored);
  }

  const std::filesystem::path& path() const { return path_; }

private:
  std::filesystem::path path_;
};

std::filesystem::path UniqueRoundTripPath()
{
  std::random_device entropy;
  std::mt19937_64 random(entropy());
  const auto dir = std::filesystem::temp_directory_path();
  for (int attempt = 0; attempt < 16; ++attempt) {
    std::ostringstream name;
    name << "occt_smoke_roundtrip_" << std::hex << random() << ".step";
    const auto candidate = dir / name.str();
    if (!std::filesystem::exists(candidate)) {
      return candidate;
    }
  }
  throw std::runtime_error("could not allocate a unique temporary STEP path");
}

struct StepResult {
  int         id = 0;
  const char* name = "";
  bool        pass = false;
  double      ms = 0.0;
  std::string detail;
};

// Shape held across steps that need the previous solid (boolean → fillet → mesh).
struct SharedGeom {
  TopoDS_Shape box;
  TopoDS_Shape cut_result;
  TopoDS_Shape filleted;
};

// STEP metadata captured on read and checked after write/read round-trip.
struct StepMeta {
  std::string units;
  std::string name;
  bool        has_color = false;
  double      r = 0, g = 0, b = 0;
};

int CountSubshapes(const TopoDS_Shape& shape, TopAbs_ShapeEnum type)
{
  TopTools_IndexedMapOfShape map;
  TopExp::MapShapes(shape, type, map);
  return map.Extent();
}

bool IsValidSolid(const TopoDS_Shape& shape, std::string& why)
{
  if (shape.IsNull()) {
    why = "shape is null";
    return false;
  }
  const TopAbs_ShapeEnum t = shape.ShapeType();
  if (t != TopAbs_SOLID && t != TopAbs_COMPSOLID && t != TopAbs_COMPOUND) {
    why = "shape type is not solid/compsolid/compound";
    return false;
  }
  // Explicit geometric/topological check; no silent defaults for “valid enough”.
  BRepCheck_Analyzer analyzer(shape, Standard_True);
  if (!analyzer.IsValid()) {
    why = "BRepCheck_Analyzer reports invalid";
    return false;
  }
  if (t == TopAbs_COMPOUND) {
    bool any_solid = false;
    for (TopExp_Explorer ex(shape, TopAbs_SOLID); ex.More(); ex.Next()) {
      any_solid = true;
      break;
    }
    if (!any_solid) {
      why = "compound contains no solid";
      return false;
    }
  }
  return true;
}

double EdgeLength(const TopoDS_Edge& edge)
{
  GProp_GProps props;
  BRepGProp::LinearProperties(edge, props);
  return props.Mass();
}

// Read XDE document: units, first useful name, first colour.
bool ExtractStepMeta(const Handle(TDocStd_Document)& doc, StepMeta& out, std::string& err)
{
  out = StepMeta{};

  // Length unit from the XCAF document. GetLengthUnit returns the document's
  // internal unit expressed in metres (overload without base-unit enum).
  Standard_Real unit_metres = 0.0;
  if (XCAFDoc_DocumentTool::GetLengthUnit(doc, unit_metres)) {
    std::ostringstream us;
    us << std::setprecision(12)
       << "length_unit_m=" << unit_metres
       << " length_unit_mm=" << (unit_metres * 1000.0);
    out.units = us.str();
  } else {
    // Fallback: cascade unit factor and Interface_Static (no silent default
    // printed as if it were a measured file unit).
    const char* static_unit = Interface_Static::CVal("xstep.cascade.unit");
    std::ostringstream us;
    us << "xstep.cascade.unit="
       << (static_unit != nullptr && static_unit[0] != '\0' ? static_unit
                                                           : "(unset)");
    // GetCasCadeLengthUnit: factor relative to millimetres in typical builds.
    us << " cascade_length_unit=" << UnitsMethods::GetCasCadeLengthUnit();
    out.units = us.str();
  }

  Handle(XCAFDoc_ShapeTool) shape_tool =
    XCAFDoc_DocumentTool::ShapeTool(doc->Main());
  Handle(XCAFDoc_ColorTool) color_tool =
    XCAFDoc_DocumentTool::ColorTool(doc->Main());
  if (shape_tool.IsNull() || color_tool.IsNull()) {
    err = "XCAF ShapeTool or ColorTool is null";
    return false;
  }

  TDF_LabelSequence labels;
  shape_tool->GetFreeShapes(labels);
  if (labels.Length() == 0) {
    shape_tool->GetShapes(labels);
  }
  if (labels.Length() == 0) {
    err = "XCAF document has no shapes";
    return false;
  }

  // Walk free shapes and their components for a name and a colour.
  for (Standard_Integer i = 1; i <= labels.Length(); ++i) {
    const TDF_Label lab = labels.Value(i);

    if (out.name.empty()) {
      Handle(TDataStd_Name) name_attr;
      if (lab.FindAttribute(TDataStd_Name::GetID(), name_attr) && !name_attr.IsNull()) {
        const TCollection_ExtendedString& es = name_attr->Get();
        if (es.Length() > 0) {
          TCollection_AsciiString ascii(es, '?');
          out.name = ascii.ToCString();
        }
      }
      // Also try GetShapeLabel name via ShapeTool helper.
      if (out.name.empty()) {
        Handle(TDataStd_Name) n2;
        TDF_Label referred = lab;
        if (shape_tool->GetReferredShape(lab, referred)) {
          if (referred.FindAttribute(TDataStd_Name::GetID(), n2) && !n2.IsNull()) {
            TCollection_AsciiString ascii(n2->Get(), '?');
            if (ascii.Length() > 0) {
              out.name = ascii.ToCString();
            }
          }
        }
      }
    }

    if (!out.has_color) {
      Quantity_Color col;
      const XCAFDoc_ColorType types[] = {
        XCAFDoc_ColorGen, XCAFDoc_ColorSurf, XCAFDoc_ColorCurv};
      for (XCAFDoc_ColorType ct : types) {
        if (color_tool->GetColor(lab, ct, col)) {
          out.has_color = true;
          out.r = col.Red();
          out.g = col.Green();
          out.b = col.Blue();
          break;
        }
      }
      // Instance / referred shape colour.
      if (!out.has_color) {
        TDF_Label referred;
        if (shape_tool->GetReferredShape(lab, referred)) {
          for (XCAFDoc_ColorType ct : types) {
            if (color_tool->GetColor(referred, ct, col)) {
              out.has_color = true;
              out.r = col.Red();
              out.g = col.Green();
              out.b = col.Blue();
              break;
            }
          }
        }
      }
    }

    // Components under assemblies.
    if (out.name.empty() || !out.has_color) {
      TDF_LabelSequence comps;
      if (shape_tool->GetComponents(lab, comps)) {
        for (Standard_Integer j = 1; j <= comps.Length(); ++j) {
          const TDF_Label c = comps.Value(j);
          if (out.name.empty()) {
            Handle(TDataStd_Name) na;
            if (c.FindAttribute(TDataStd_Name::GetID(), na) && !na.IsNull()) {
              TCollection_AsciiString ascii(na->Get(), '?');
              if (ascii.Length() > 0) {
                out.name = ascii.ToCString();
              }
            }
          }
          if (!out.has_color) {
            Quantity_Color col;
            if (color_tool->GetColor(c, XCAFDoc_ColorGen, col)
                || color_tool->GetColor(c, XCAFDoc_ColorSurf, col)) {
              out.has_color = true;
              out.r = col.Red();
              out.g = col.Green();
              out.b = col.Blue();
            }
          }
        }
      }
    }
  }

  // Last resort: scan all labels with colours / names via tools.
  if (out.name.empty() || !out.has_color) {
    TDF_LabelSequence all;
    shape_tool->GetShapes(all);
    for (Standard_Integer i = 1; i <= all.Length(); ++i) {
      const TDF_Label lab = all.Value(i);
      if (out.name.empty()) {
        Handle(TDataStd_Name) na;
        if (lab.FindAttribute(TDataStd_Name::GetID(), na) && !na.IsNull()) {
          TCollection_AsciiString ascii(na->Get(), '?');
          if (ascii.Length() > 0) {
            out.name = ascii.ToCString();
          }
        }
      }
      if (!out.has_color) {
        Quantity_Color col;
        if (color_tool->GetColor(lab, XCAFDoc_ColorGen, col)
            || color_tool->GetColor(lab, XCAFDoc_ColorSurf, col)
            || color_tool->GetColor(lab, XCAFDoc_ColorCurv, col)) {
          out.has_color = true;
          out.r = col.Red();
          out.g = col.Green();
          out.b = col.Blue();
        }
      }
    }
  }

  if (out.name.empty()) {
    err = "no shape name found in XDE document";
    return false;
  }
  if (!out.has_color) {
    err = "no shape colour found in XDE document";
    return false;
  }
  return true;
}

Handle(TDocStd_Document) NewXdeDocument()
{
  Handle(XCAFApp_Application) app = XCAFApp_Application::GetApplication();
  Handle(TDocStd_Document) doc;
  app->NewDocument("MDTV-XCAF", doc);
  if (doc.IsNull()) {
    throw Standard_Failure("XCAFApp_Application::NewDocument returned null");
  }
  return doc;
}

bool ColourNearlyEqual(const StepMeta& a, const StepMeta& b, double tol = 1e-3)
{
  return a.has_color && b.has_color
         && std::abs(a.r - b.r) <= tol
         && std::abs(a.g - b.g) <= tol
         && std::abs(a.b - b.b) <= tol;
}

void PrintStepLine(const StepResult& r)
{
  std::cout << "  [" << (r.pass ? "PASS" : "FAIL") << "] "
            << "step " << r.id << " " << r.name
            << "  " << std::fixed << std::setprecision(2) << r.ms << " ms";
  if (!r.detail.empty()) {
    std::cout << "  — " << r.detail;
  }
  std::cout << '\n';
}

template <typename Fn>
StepResult RunStep(int id, const char* name, Fn&& fn)
{
  StepResult r;
  r.id = id;
  r.name = name;
  r.pass = false;
  const auto t0 = Clock::now();
  try {
    fn(r);
  } catch (const Standard_Failure& e) {
    r.pass = false;
    const char* msg = e.GetMessageString();
    r.detail = std::string("Standard_Failure: ")
               + (msg != nullptr ? msg : "(no message)");
  } catch (const std::exception& e) {
    r.pass = false;
    r.detail = std::string("std::exception: ") + e.what();
  } catch (...) {
    r.pass = false;
    r.detail = "unknown non-standard exception";
  }
  r.ms = std::chrono::duration<double, std::milli>(Clock::now() - t0).count();
  PrintStepLine(r);
  return r;
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

void Step1_BoxVolume(StepResult& r, SharedGeom& geom)
{
  constexpr double dx = 10.0;
  constexpr double dy = 20.0;
  constexpr double dz = 30.0;
  constexpr double vol_tol = 1e-7;
  constexpr double expected = dx * dy * dz; // 6000

  BRepPrimAPI_MakeBox maker(dx, dy, dz);
  maker.Build();
  if (!maker.IsDone()) {
    r.detail = "BRepPrimAPI_MakeBox failed";
    return;
  }
  geom.box = maker.Shape();

  GProp_GProps props;
  BRepGProp::VolumeProperties(geom.box, props);
  const double vol = props.Mass();
  const double err = std::abs(vol - expected);

  std::ostringstream d;
  d << std::setprecision(10)
    << "box " << dx << "x" << dy << "x" << dz
    << " vol=" << vol << " expected=" << expected
    << " |err|=" << err << " tol=" << vol_tol;
  r.detail = d.str();
  r.pass = err <= vol_tol;
}

void Step2_ExtrudeProfile(StepResult& r)
{
  constexpr double height = 8.0; // mm
  // Closed planar rectangle 6 x 4 in XY, then prism along +Z.
  const gp_Pnt p0(0.0, 0.0, 0.0);
  const gp_Pnt p1(6.0, 0.0, 0.0);
  const gp_Pnt p2(6.0, 4.0, 0.0);
  const gp_Pnt p3(0.0, 4.0, 0.0);

  BRepBuilderAPI_MakeEdge e0(p0, p1);
  BRepBuilderAPI_MakeEdge e1(p1, p2);
  BRepBuilderAPI_MakeEdge e2(p2, p3);
  BRepBuilderAPI_MakeEdge e3(p3, p0);
  if (!e0.IsDone() || !e1.IsDone() || !e2.IsDone() || !e3.IsDone()) {
    r.detail = "failed to build one of the four edges";
    return;
  }

  BRepBuilderAPI_MakeWire wire_maker;
  wire_maker.Add(e0.Edge());
  wire_maker.Add(e1.Edge());
  wire_maker.Add(e2.Edge());
  wire_maker.Add(e3.Edge());
  if (!wire_maker.IsDone()) {
    r.detail = "BRepBuilderAPI_MakeWire failed";
    return;
  }
  const TopoDS_Wire wire = wire_maker.Wire();
  if (!wire.Closed()) {
    r.detail = "wire is not closed";
    return;
  }

  // Only plane faces from a planar wire; tolerance left to builder but face
  // construction is checked explicitly below.
  BRepBuilderAPI_MakeFace face_maker(wire, Standard_True);
  if (!face_maker.IsDone()) {
    r.detail = "BRepBuilderAPI_MakeFace failed";
    return;
  }

  BRepPrimAPI_MakePrism prism(face_maker.Face(), gp_Vec(0.0, 0.0, height));
  prism.Build();
  if (!prism.IsDone()) {
    r.detail = "BRepPrimAPI_MakePrism failed";
    return;
  }

  std::string why;
  const bool ok = IsValidSolid(prism.Shape(), why);
  std::ostringstream d;
  d << "extrude height=" << height << " mm";
  if (ok) {
    d << " solid ok faces=" << CountSubshapes(prism.Shape(), TopAbs_FACE);
  } else {
    d << " " << why;
  }
  r.detail = d.str();
  r.pass = ok;
}

void Step3_BooleanCut(StepResult& r, SharedGeom& geom)
{
  if (geom.box.IsNull()) {
    r.detail = "step 1 box is missing";
    return;
  }

  // Cylinder through the box; explicit radius/height, axis at box centre.
  constexpr double cyl_r = 3.0;
  constexpr double cyl_h = 40.0;
  const gp_Ax2 axis(gp_Pnt(5.0, 10.0, -5.0), gp_Dir(0.0, 0.0, 1.0));

  BRepPrimAPI_MakeCylinder cyl_maker(axis, cyl_r, cyl_h);
  cyl_maker.Build();
  if (!cyl_maker.IsDone()) {
    r.detail = "BRepPrimAPI_MakeCylinder failed";
    return;
  }

  // Fuzzy value is set explicitly before Build (constructor overloads that
  // run immediately would ignore a late SetFuzzyValue).
  constexpr double fuzzy = 1.0e-7;
  TopTools_ListOfShape args;
  TopTools_ListOfShape tools;
  args.Append(geom.box);
  tools.Append(cyl_maker.Shape());

  BRepAlgoAPI_Cut cut;
  cut.SetFuzzyValue(fuzzy);
  cut.SetArguments(args);
  cut.SetTools(tools);
  cut.Build();
  if (!cut.IsDone()) {
    r.detail = "BRepAlgoAPI_Cut failed";
    return;
  }
  geom.cut_result = cut.Shape();

  std::string why;
  const bool ok = IsValidSolid(geom.cut_result, why);
  std::ostringstream d;
  d << "cut cylinder r=" << cyl_r << " h=" << cyl_h
    << " fuzzy=" << fuzzy;
  if (ok) {
    d << " solid ok";
  } else {
    d << " " << why;
  }
  r.detail = d.str();
  r.pass = ok;
}

void Step4_Fillet(StepResult& r, SharedGeom& geom)
{
  if (geom.cut_result.IsNull()) {
    r.detail = "step 3 cut result is missing";
    return;
  }

  constexpr double radius = 2.0; // mm
  BRepFilletAPI_MakeFillet fillet(geom.cut_result);

  TopTools_IndexedMapOfShape edges;
  TopExp::MapShapes(geom.cut_result, TopAbs_EDGE, edges);

  // Pick a long enough straight-ish outer edge so r=2 is safe on the 10x20x30 box.
  TopoDS_Edge chosen;
  double best_len = 0.0;
  for (Standard_Integer i = 1; i <= edges.Extent(); ++i) {
    const TopoDS_Edge e = TopoDS::Edge(edges(i));
    if (BRep_Tool::Degenerated(e)) {
      continue;
    }
    const double len = EdgeLength(e);
    // Prefer edges longer than 2*radius and maximise length.
    if (len > 2.0 * radius + 1.0 && len > best_len) {
      best_len = len;
      chosen = e;
    }
  }
  if (chosen.IsNull()) {
    // Fallback: any non-degenerate edge.
    for (Standard_Integer i = 1; i <= edges.Extent(); ++i) {
      const TopoDS_Edge e = TopoDS::Edge(edges(i));
      if (!BRep_Tool::Degenerated(e)) {
        chosen = e;
        best_len = EdgeLength(e);
        break;
      }
    }
  }
  if (chosen.IsNull()) {
    r.detail = "no suitable edge for fillet";
    return;
  }

  fillet.Add(radius, chosen);
  fillet.Build();
  if (!fillet.IsDone()) {
    std::ostringstream d;
    d << "BRepFilletAPI_MakeFillet failed radius=" << radius
      << " edge_len=" << best_len;
    r.detail = d.str();
    return;
  }
  geom.filleted = fillet.Shape();

  std::string why;
  const bool ok = IsValidSolid(geom.filleted, why);
  std::ostringstream d;
  d << "fillet radius=" << radius << " mm edge_len=" << best_len;
  if (ok) {
    d << " solid ok";
  } else {
    d << " " << why;
  }
  r.detail = d.str();
  r.pass = ok;
}

void Step5_BRepRoundTrip(StepResult& r, const SharedGeom& geom)
{
  const TopoDS_Shape& src =
    !geom.filleted.IsNull() ? geom.filleted
    : !geom.cut_result.IsNull() ? geom.cut_result
                                : geom.box;
  if (src.IsNull()) {
    r.detail = "no shape available for B-Rep round-trip";
    return;
  }

  const int f0 = CountSubshapes(src, TopAbs_FACE);
  const int e0 = CountSubshapes(src, TopAbs_EDGE);
  const int v0 = CountSubshapes(src, TopAbs_VERTEX);

  std::stringstream buffer;
  BRepTools::Write(src, buffer);
  if (buffer.str().empty()) {
    r.detail = "BRepTools::Write produced empty string";
    return;
  }
  buffer.seekg(0);

  BRep_Builder builder;
  TopoDS_Shape dst;
  BRepTools::Read(dst, buffer, builder);
  if (dst.IsNull()) {
    r.detail = "BRepTools::Read returned null shape";
    return;
  }

  const int f1 = CountSubshapes(dst, TopAbs_FACE);
  const int e1 = CountSubshapes(dst, TopAbs_EDGE);
  const int v1 = CountSubshapes(dst, TopAbs_VERTEX);

  std::ostringstream d;
  d << "faces " << f0 << "->" << f1
    << " edges " << e0 << "->" << e1
    << " vertices " << v0 << "->" << v1
    << " bytes=" << buffer.str().size();
  r.detail = d.str();
  r.pass = (f0 == f1) && (e0 == e1) && (v0 == v1) && (f0 > 0);
}

void Step6_ReadStepXde(StepResult& r, const std::string& path, StepMeta& meta)
{
  Handle(TDocStd_Document) doc = NewXdeDocument();

  STEPCAFControl_Reader reader;
  reader.SetColorMode(Standard_True);
  reader.SetNameMode(Standard_True);
  reader.SetLayerMode(Standard_True);
  reader.SetPropsMode(Standard_True);

  const IFSelect_ReturnStatus st = reader.ReadFile(path.c_str());
  if (st != IFSelect_RetDone) {
    std::ostringstream d;
    d << "STEPCAFControl_Reader::ReadFile failed status=" << static_cast<int>(st)
      << " path=" << path;
    r.detail = d.str();
    return;
  }

  if (!reader.Transfer(doc)) {
    r.detail = "STEPCAFControl_Reader::Transfer failed";
    return;
  }

  std::string err;
  if (!ExtractStepMeta(doc, meta, err)) {
    r.detail = err;
    return;
  }

  std::ostringstream d;
  d << std::setprecision(6)
    << "units=[" << meta.units << "] name=\"" << meta.name << "\" "
    << "color=RGB(" << meta.r << "," << meta.g << "," << meta.b << ")";
  r.detail = d.str();
  r.pass = true;
}

void Step7_StepWriteReadRoundTrip(StepResult& r,
                                  const std::string& input_path,
                                  const StepMeta& original)
{
  // Re-read input into a fresh document, write to a temp STEP, read back.
  Handle(TDocStd_Document) doc = NewXdeDocument();

  STEPCAFControl_Reader reader;
  reader.SetColorMode(Standard_True);
  reader.SetNameMode(Standard_True);
  if (reader.ReadFile(input_path.c_str()) != IFSelect_RetDone) {
    r.detail = "re-read of input STEP failed";
    return;
  }
  if (!reader.Transfer(doc)) {
    r.detail = "Transfer of input STEP failed";
    return;
  }

  // Write under the system temp directory so the input tree stays clean. A
  // unique, scoped path lets multiple smoke tests run concurrently and removes
  // the generated file even if one of the later checks fails.
  const ScopedTempPath tmp(UniqueRoundTripPath());
  const std::string out_path = tmp.path().string();

  STEPCAFControl_Writer writer;
  writer.SetColorMode(Standard_True);
  writer.SetNameMode(Standard_True);
  if (!writer.Transfer(doc, STEPControl_AsIs)) {
    r.detail = "STEPCAFControl_Writer::Transfer failed";
    return;
  }
  if (writer.Write(out_path.c_str()) != IFSelect_RetDone) {
    r.detail = "STEPCAFControl_Writer::Write failed path=" + out_path;
    return;
  }

  Handle(TDocStd_Document) doc2 = NewXdeDocument();
  STEPCAFControl_Reader reader2;
  reader2.SetColorMode(Standard_True);
  reader2.SetNameMode(Standard_True);
  if (reader2.ReadFile(out_path.c_str()) != IFSelect_RetDone) {
    r.detail = "read-back of written STEP failed path=" + out_path;
    return;
  }
  if (!reader2.Transfer(doc2)) {
    r.detail = "Transfer of written STEP failed";
    return;
  }

  StepMeta back;
  std::string err;
  if (!ExtractStepMeta(doc2, back, err)) {
    r.detail = "extract after round-trip: " + err;
    return;
  }

  const bool name_ok = (back.name == original.name)
                       || (back.name.find(original.name) != std::string::npos)
                       || (original.name.find(back.name) != std::string::npos);
  const bool color_ok = ColourNearlyEqual(original, back);

  std::ostringstream d;
  d << std::setprecision(6)
    << "wrote=" << out_path
    << " name \"" << original.name << "\"->\"" << back.name << "\""
    << " color RGB(" << original.r << "," << original.g << "," << original.b
    << ")->RGB(" << back.r << "," << back.g << "," << back.b << ")"
    << " name_ok=" << (name_ok ? "yes" : "no")
    << " color_ok=" << (color_ok ? "yes" : "no");
  r.detail = d.str();
  r.pass = name_ok && color_ok;
}

void Step8_Tessellate(StepResult& r, const SharedGeom& geom)
{
  if (geom.filleted.IsNull()) {
    r.detail = "step 4 filleted shape is missing";
    return;
  }

  // Explicit linear and angular deflections — never rely on OCCT defaults.
  constexpr double linear_deflection = 0.01; // mm
  constexpr double angular_deflection = 0.5; // rad
  constexpr Standard_Boolean is_relative = Standard_False;
  constexpr Standard_Boolean parallel = Standard_True;

  BRepMesh_IncrementalMesh mesher(
    geom.filleted,
    linear_deflection,
    is_relative,
    angular_deflection,
    parallel);
  mesher.Perform();
  if (!mesher.IsDone()) {
    r.detail = "BRepMesh_IncrementalMesh failed";
    return;
  }

  int triangles = 0;
  for (TopExp_Explorer ex(geom.filleted, TopAbs_FACE); ex.More(); ex.Next()) {
    TopLoc_Location loc;
    const Handle(Poly_Triangulation) tri =
      BRep_Tool::Triangulation(TopoDS::Face(ex.Current()), loc);
    if (!tri.IsNull()) {
      triangles += tri->NbTriangles();
    }
  }

  std::ostringstream d;
  d << "linear_defl=" << linear_deflection << " mm"
    << " angular_defl=" << angular_deflection << " rad"
    << " relative=" << (is_relative ? "true" : "false")
    << " triangles=" << triangles;
  r.detail = d.str();
  r.pass = triangles > 0;
}

} // namespace

int main(int argc, char** argv)
{
  // First line: OCCT version.
  std::cout << "OCC_VERSION_COMPLETE=" << OCC_VERSION_COMPLETE << '\n';

  if (argc != 2) {
    std::cerr
      << "Usage: " << (argc > 0 ? argv[0] : "occt_smoke")
      << " <path-to-step-file>\n"
      << "The STEP file must carry at least one shape name and one colour "
         "(XDE).\n";
    return 1;
  }

  const std::string step_path = argv[1];
  SharedGeom geom;
  StepMeta step_meta;
  std::vector<StepResult> results;
  results.reserve(8);

  results.push_back(RunStep(1, "box_volume", [&](StepResult& r) {
    Step1_BoxVolume(r, geom);
  }));
  results.push_back(RunStep(2, "extrude_profile", [&](StepResult& r) {
    Step2_ExtrudeProfile(r);
  }));
  results.push_back(RunStep(3, "boolean_cut", [&](StepResult& r) {
    Step3_BooleanCut(r, geom);
  }));
  results.push_back(RunStep(4, "fillet_edge", [&](StepResult& r) {
    Step4_Fillet(r, geom);
  }));
  results.push_back(RunStep(5, "brep_roundtrip", [&](StepResult& r) {
    Step5_BRepRoundTrip(r, geom);
  }));
  results.push_back(RunStep(6, "step_xde_read", [&](StepResult& r) {
    Step6_ReadStepXde(r, step_path, step_meta);
  }));
  results.push_back(RunStep(7, "step_xde_roundtrip", [&](StepResult& r) {
    if (!results[5].pass) {
      r.detail = "skipped: step 6 did not pass";
      r.pass = false;
      return;
    }
    Step7_StepWriteReadRoundTrip(r, step_path, step_meta);
  }));
  results.push_back(RunStep(8, "tessellate", [&](StepResult& r) {
    Step8_Tessellate(r, geom);
  }));

  // Summary table.
  std::cout << '\n';
  std::cout << "==== summary ====\n";
  std::cout << std::left << std::setw(4) << "step"
            << std::setw(20) << "name"
            << std::setw(8) << "status"
            << std::setw(12) << "ms" << '\n';
  std::cout << std::string(44, '-') << '\n';

  int failed = 0;
  for (const StepResult& r : results) {
    if (!r.pass) {
      ++failed;
    }
    std::cout << std::left << std::setw(4) << r.id
              << std::setw(20) << r.name
              << std::setw(8) << (r.pass ? "PASS" : "FAIL")
              << std::right << std::fixed << std::setprecision(2)
              << std::setw(10) << r.ms << '\n';
  }
  std::cout << std::string(44, '-') << '\n';
  std::cout << "failed_steps=" << failed
            << "  exit_code=" << failed << '\n';

  return failed;
}
