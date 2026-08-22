// SPDX-License-Identifier: MIT
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
#include <map>
#include <set>
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
#include <GC_MakeArcOfCircle.hxx>
#include <Poly_PolygonOnTriangulation.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepTools.hxx>
#include <BinTools.hxx>
#include <BinTools_FormatVersion.hxx>
#include <GProp_GProps.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Iterator.hxx>
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

// Step 9: what the prism says about the corners of its profile.
//
// This is the 19M-2a measurement, kept where it can be re-run rather than
// quoted. It is here and not in a scratch file because the association it
// checks is the one a durable vertex name will rest on, and the pin workflow
// runs this tool on Linux, macOS and Windows against the pinned build.
//
// The claim, stated exactly:
//
//   * for each POSITIONAL corner of the profile and each cap side,
//     FirstShape(corner) or LastShape(corner) produces exactly one
//     TopoDS_VERTEX;
//   * that vertex belongs to the finished solid by IsSame, lies on the cap it
//     is claimed for, and ends the edge swept from that same corner.
//
// It says nothing about ProfileJoint uniqueness. A profile whose unordered
// joint occurs at two corners - a two-segment loop - still yields two distinct
// positional vertices per side here, which is precisely why the ambiguity has
// to be resolved above this layer and not below it.
// A point no profile uses, marking a segment as straight.
const gp_Pnt kStraight(1.0e30, 1.0e30, 1.0e30);

struct CornerProbe {
  const char* name;
  std::vector<gp_Pnt> corners;  // in profile order
  // One entry per segment: a point the arc passes through, or kStraight.
  // Empty means every segment is a line.
  std::vector<gp_Pnt> through;
  double base;
  double top;
};

// Builds the profile the way the bridge does: corner vertices made once and
// shared, so the history is asked about the very vertex two segments meet at.
bool BuildSweep(const CornerProbe& probe, std::vector<TopoDS_Vertex>& corners,
                std::vector<TopoDS_Edge>& edges, TopoDS_Shape& solid,
                BRepPrimAPI_MakePrism*& out_prism, std::string& why)
{
  const size_t n = probe.corners.size();
  corners.clear();
  edges.clear();
  for (const gp_Pnt& at : probe.corners) {
    corners.push_back(BRepBuilderAPI_MakeVertex(
      gp_Pnt(at.X(), at.Y(), at.Z() + probe.base)));
  }

  BRepBuilderAPI_MakeWire wire;
  for (size_t i = 0; i < n; ++i) {
    const bool curved = i < probe.through.size() &&
                        probe.through[i].X() < 1.0e29;
    TopoDS_Edge edge;
    if (curved) {
      const gp_Pnt mid(probe.through[i].X(), probe.through[i].Y(),
                       probe.through[i].Z() + probe.base);
      GC_MakeArcOfCircle arc(BRep_Tool::Pnt(corners[i]), mid,
                             BRep_Tool::Pnt(corners[(i + 1) % n]));
      if (!arc.IsDone()) {
        why = std::string(probe.name) + ": segment " + std::to_string(i) +
              " describes no arc";
        return false;
      }
      BRepBuilderAPI_MakeEdge maker(arc.Value(), corners[i],
                                    corners[(i + 1) % n]);
      if (!maker.IsDone()) {
        why = std::string(probe.name) + ": an arc edge failed";
        return false;
      }
      edge = maker.Edge();
    } else {
      BRepBuilderAPI_MakeEdge maker(corners[i], corners[(i + 1) % n]);
      if (!maker.IsDone()) {
        why = std::string(probe.name) + ": a straight edge failed";
        return false;
      }
      edge = maker.Edge();
    }
    edges.push_back(edge);
    wire.Add(edge);
  }
  if (!wire.IsDone()) {
    why = std::string(probe.name) + ": the profile is not a closed wire";
    return false;
  }
  BRepBuilderAPI_MakeFace face(wire.Wire());
  if (!face.IsDone()) {
    why = std::string(probe.name) + ": the profile bounds no face";
    return false;
  }
  out_prism = new BRepPrimAPI_MakePrism(
    face.Face(), gp_Vec(0.0, 0.0, probe.top - probe.base));
  out_prism->Build();
  if (!out_prism->IsDone()) {
    why = std::string(probe.name) + ": the sweep produced no solid";
    return false;
  }
  solid = out_prism->Shape();
  return true;
}

// Which vertices of a solid the tessellation reaches, by the 19M-1a rule: an
// edge polyline on a meshed face begins and ends at that edge's own two
// topological ends. Handle equality throughout; no coordinate takes part.
std::set<int> TessellatedVertices(const TopoDS_Shape& solid,
                                  const TopTools_IndexedMapOfShape& vertices)
{
  BRepMesh_IncrementalMesh mesher(solid, 0.1, Standard_False, 0.5,
                                  Standard_True);
  mesher.Perform();
  std::set<int> reached;
  for (TopExp_Explorer ft(solid, TopAbs_FACE); ft.More(); ft.Next()) {
    const TopoDS_Face face = TopoDS::Face(ft.Current());
    TopLoc_Location loc;
    const Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
    if (tri.IsNull()) {
      continue;
    }
    for (TopExp_Explorer et(face, TopAbs_EDGE); et.More(); et.Next()) {
      const TopoDS_Edge edge = TopoDS::Edge(et.Current());
      const Handle(Poly_PolygonOnTriangulation) poly =
        BRep_Tool::PolygonOnTriangulation(edge, tri, loc);
      if (poly.IsNull()) {
        continue;
      }
      TopoDS_Vertex first_end;
      TopoDS_Vertex last_end;
      TopExp::Vertices(TopoDS::Edge(edge.Oriented(TopAbs_FORWARD)), first_end,
                       last_end);
      if (!first_end.IsNull()) {
        reached.insert(vertices.FindIndex(first_end));
      }
      if (!last_end.IsNull()) {
        reached.insert(vertices.FindIndex(last_end));
      }
    }
  }
  return reached;
}

void Step9_CapVertices(StepResult& r)
{
  // The arc profile of the measurement: two straight segments and one arc
  // closing the loop, so a joint between two lines and a joint against a curve
  // are both present. The lens is the ambiguous case: two segments meeting at
  // two corners, whose one unordered pair can name neither of them.
  const gp_Pnt S = kStraight;
  const std::vector<CornerProbe> probes = {
    {"plate blind",
     {gp_Pnt(0, 0, 0), gp_Pnt(60, 0, 0), gp_Pnt(60, 40, 0), gp_Pnt(0, 40, 0)},
     {}, 0.0, 10.0},
    {"plate reversed",
     {gp_Pnt(0, 0, 0), gp_Pnt(60, 0, 0), gp_Pnt(60, 40, 0), gp_Pnt(0, 40, 0)},
     {}, 0.0, -10.0},
    {"plate symmetric",
     {gp_Pnt(0, 0, 0), gp_Pnt(60, 0, 0), gp_Pnt(60, 40, 0), gp_Pnt(0, 40, 0)},
     {}, -5.0, 5.0},
    {"triangle blind",
     {gp_Pnt(0, 0, 0), gp_Pnt(30, 0, 0), gp_Pnt(10, 25, 0)},
     {}, 0.0, 10.0},
    {"arc profile blind",
     {gp_Pnt(0, 0, 0), gp_Pnt(30, 0, 0), gp_Pnt(30, 20, 0)},
     {S, S, gp_Pnt(5, 25, 0)}, 0.0, 10.0},
    {"arc profile symmetric",
     {gp_Pnt(0, 0, 0), gp_Pnt(30, 0, 0), gp_Pnt(30, 20, 0)},
     {S, S, gp_Pnt(5, 25, 0)}, -5.0, 5.0},
    {"two-segment lens",
     {gp_Pnt(0, 0, 0), gp_Pnt(30, 0, 0)},
     {S, gp_Pnt(15, 15, 0)}, 0.0, 10.0},
  };

  int checked = 0;
  for (const CornerProbe& probe : probes) {
    const size_t n = probe.corners.size();
    std::vector<TopoDS_Vertex> corners;
    std::vector<TopoDS_Edge> edges;
    TopoDS_Shape solid;
    BRepPrimAPI_MakePrism* prism = nullptr;
    std::string why;
    if (!BuildSweep(probe, corners, edges, solid, prism, why)) {
      delete prism;
      r.detail = why;
      return;
    }

    TopTools_IndexedMapOfShape solid_vertices;
    TopExp::MapShapes(solid, TopAbs_VERTEX, solid_vertices);
    const std::set<int> reached = TessellatedVertices(solid, solid_vertices);

    std::set<int> claimed;
    for (size_t j = 0; j < n; ++j) {
      TopoDS_Shape swept;
      const NCollection_List<TopoDS_Shape>& generated =
        prism->Generated(corners[j]);
      if (generated.Extent() == 1 &&
          generated.First().ShapeType() == TopAbs_EDGE) {
        swept = generated.First();
      }

      for (int side = 0; side < 2; ++side) {
        const TopoDS_Shape got = side == 0 ? prism->FirstShape(corners[j])
                                           : prism->LastShape(corners[j]);
        const std::string where = std::string(probe.name) + " corner " +
                                  std::to_string(j) +
                                  (side == 0 ? " start" : " end");
        if (got.IsNull()) {
          delete prism;
          r.detail = where + ": the prism named no vertex";
          return;
        }
        if (got.ShapeType() != TopAbs_VERTEX) {
          delete prism;
          r.detail = where + ": the prism named something that is not a vertex";
          return;
        }
        const int which = solid_vertices.FindIndex(got);
        if (which == 0) {
          delete prism;
          r.detail = where + ": the named vertex is not in the finished solid";
          return;
        }
        if (!claimed.insert(which).second) {
          delete prism;
          r.detail = where + ": two corners named one vertex";
          return;
        }
        const TopoDS_Shape cap =
          side == 0 ? prism->FirstShape() : prism->LastShape();
        bool on_cap = false;
        if (!cap.IsNull()) {
          for (TopExp_Explorer it(cap, TopAbs_VERTEX); it.More(); it.Next()) {
            if (it.Current().IsSame(got)) {
              on_cap = true;
              break;
            }
          }
        }
        if (!on_cap) {
          delete prism;
          r.detail = where + ": the named vertex is not on that cap";
          return;
        }
        if (swept.IsNull()) {
          delete prism;
          r.detail = where + ": this corner swept no edge to check against";
          return;
        }
        TopoDS_Vertex first_end;
        TopoDS_Vertex last_end;
        TopExp::Vertices(
          TopoDS::Edge(TopoDS::Edge(swept).Oriented(TopAbs_FORWARD)),
          first_end, last_end);
        const bool ends = (!first_end.IsNull() && first_end.IsSame(got)) ||
                          (!last_end.IsNull() && last_end.IsSame(got));
        if (!ends) {
          delete prism;
          r.detail = where + ": the named vertex does not end this corner's edge";
          return;
        }
        // Reached by the tessellation association, by handle identity.
        if (reached.find(which) == reached.end()) {
          delete prism;
          r.detail = where + ": the named vertex is never reached by the mesh";
          return;
        }
        ++checked;
      }
    }
    if (claimed.size() != n * 2) {
      delete prism;
      r.detail = std::string(probe.name) + ": expected " +
                 std::to_string(n * 2) + " distinct cap vertices, found " +
                 std::to_string(claimed.size());
      return;
    }

    // The naming rule this measurement exists to justify. A corner is named by
    // the unordered pair of the segments meeting there. With two segments both
    // corners carry the same pair, so the kernel's four distinct vertices are
    // still two unnameable pairs, and that is stated here rather than assumed
    // by whatever reads this next.
    const size_t distinct_pairs = n == 2 ? 1 : n;
    if (n == 2) {
      if (claimed.size() != 4 || distinct_pairs != 1) {
        delete prism;
        r.detail = std::string(probe.name) +
                   ": the lens must give four positional vertices under one "
                   "unordered pair";
        return;
      }
    } else if (distinct_pairs != n) {
      delete prism;
      r.detail = std::string(probe.name) + ": a corner pair repeats unexpectedly";
      return;
    }
    delete prism;
  }

  // Traversal independence: the plate rebuilt from each starting segment must
  // put the same cap vertex at the corner defined by the same two segments.
  // Compared by the segment-defined meaning, never by position in the loop.
  {
    const std::vector<gp_Pnt> square = {gp_Pnt(0, 0, 0), gp_Pnt(60, 0, 0),
                                        gp_Pnt(60, 40, 0), gp_Pnt(0, 40, 0)};
    std::map<std::string, std::pair<gp_Pnt, gp_Pnt>> by_meaning;
    for (size_t start = 0; start < square.size(); ++start) {
      CornerProbe rolled{"plate rolled", {}, {}, 0.0, 10.0};
      for (size_t i = 0; i < square.size(); ++i) {
        rolled.corners.push_back(square[(start + i) % square.size()]);
      }
      std::vector<TopoDS_Vertex> corners;
      std::vector<TopoDS_Edge> edges;
      TopoDS_Shape solid;
      BRepPrimAPI_MakePrism* prism = nullptr;
      std::string why;
      if (!BuildSweep(rolled, corners, edges, solid, prism, why)) {
        delete prism;
        r.detail = why;
        return;
      }
      for (size_t j = 0; j < corners.size(); ++j) {
        // The corner where segment (j-1) meets segment j, named by the two
        // original segment numbers rather than by where they now sit.
        const size_t before =
          (start + j + square.size() - 1) % square.size();
        const size_t at = (start + j) % square.size();
        const std::string meaning =
          std::to_string(std::min(before, at)) + "-" +
          std::to_string(std::max(before, at));
        const TopoDS_Shape s0 = prism->FirstShape(corners[j]);
        const TopoDS_Shape s1 = prism->LastShape(corners[j]);
        if (s0.IsNull() || s1.IsNull()) {
          delete prism;
          r.detail = "rolled plate: a corner lost its cap vertices";
          return;
        }
        const gp_Pnt a = BRep_Tool::Pnt(TopoDS::Vertex(s0));
        const gp_Pnt b = BRep_Tool::Pnt(TopoDS::Vertex(s1));
        auto seen = by_meaning.find(meaning);
        if (seen == by_meaning.end()) {
          by_meaning.emplace(meaning, std::make_pair(a, b));
        } else if (!seen->second.first.IsEqual(a, 1.0e-7) ||
                   !seen->second.second.IsEqual(b, 1.0e-7)) {
          delete prism;
          r.detail = "rolled plate: corner " + meaning +
                     " moved when the loop started elsewhere";
          return;
        }
      }
      delete prism;
    }
    if (by_meaning.size() != 4) {
      r.detail = "rolled plate: expected four corner meanings, found " +
                 std::to_string(by_meaning.size());
      return;
    }
  }

  r.detail = "checked " + std::to_string(checked) +
             " (corner, side) associations across " +
             std::to_string(probes.size()) +
             " sweeps, plus traversal independence";
  r.pass = true;
}

// Step 10: can the named-shape archive carry vertices without changing their
// identity?
//
// This mirrors the shape layout written by fc_occt_encode_shape_named: the
// root first, followed by every requested sub-shape in caller order. The
// decoder does not accept vertices yet; this measurement establishes whether
// BinTools preserves enough information for that decoder extension to be
// honest, or whether the 19M-2a2 design has to stop.
void Step10_NamedVertexArchive(StepResult& r)
{
  const TopoDS_Shape solid = BRepPrimAPI_MakeBox(60.0, 40.0, 10.0).Shape();
  TopTools_IndexedMapOfShape vertices;
  TopExp::MapShapes(solid, TopAbs_VERTEX, vertices);
  if (vertices.Extent() != 8) {
    r.detail = "the box has " + std::to_string(vertices.Extent()) +
               " vertices instead of eight";
    return;
  }

  BRep_Builder builder;
  TopoDS_Compound archive;
  builder.MakeCompound(archive);
  builder.Add(archive, solid);
  for (int index = 1; index <= vertices.Extent(); ++index) {
    builder.Add(archive, vertices(index));
  }

  std::ostringstream output(std::ios::out | std::ios::binary);
  BinTools::Write(archive, output, false, false,
                  BinTools_FormatVersion_CURRENT);
  if (!output.good() || output.str().empty()) {
    r.detail = "BinTools wrote no named-shape archive";
    return;
  }
  const std::string bytes = output.str();

  std::istringstream input(bytes, std::ios::in | std::ios::binary);
  TopoDS_Shape restored;
  BinTools::Read(restored, input);
  if (restored.IsNull() || restored.ShapeType() != TopAbs_COMPOUND) {
    r.detail = "the named-shape archive did not restore as a compound";
    return;
  }
  if (input.peek() != std::char_traits<char>::eof()) {
    r.detail = "the named-shape archive has trailing bytes";
    return;
  }

  std::vector<TopoDS_Shape> entries;
  for (TopoDS_Iterator it(restored); it.More(); it.Next()) {
    entries.push_back(it.Value());
  }
  const size_t expected_entries = static_cast<size_t>(vertices.Extent()) + 1;
  if (entries.size() != expected_entries) {
    r.detail = "the archive restored " + std::to_string(entries.size()) +
               " children instead of " + std::to_string(expected_entries);
    return;
  }

  const TopoDS_Shape& root = entries[0];
  TopTools_IndexedMapOfShape restored_vertices;
  TopExp::MapShapes(root, TopAbs_VERTEX, restored_vertices);
  if (restored_vertices.Extent() != vertices.Extent()) {
    r.detail = "the restored root has " +
               std::to_string(restored_vertices.Extent()) +
               " vertices instead of " + std::to_string(vertices.Extent());
    return;
  }

  std::set<int> distinct;
  for (size_t entry = 1; entry < entries.size(); ++entry) {
    const TopoDS_Shape& named = entries[entry];
    if (named.ShapeType() != TopAbs_VERTEX) {
      r.detail = "archive entry " + std::to_string(entry) +
                 " did not restore as a vertex";
      return;
    }

    int canonical = 0;
    for (TopExp_Explorer it(root, TopAbs_VERTEX); it.More(); it.Next()) {
      if (it.Current().IsSame(named)) {
        canonical = restored_vertices.FindIndex(it.Current());
        break;
      }
    }
    if (canonical == 0) {
      r.detail = "archive entry " + std::to_string(entry) +
                 " is not a vertex of the restored root";
      return;
    }
    if (!distinct.insert(canonical).second) {
      r.detail = "archive entry " + std::to_string(entry) +
                 " collapsed onto an earlier named vertex";
      return;
    }
  }

  if (distinct.size() != static_cast<size_t>(vertices.Extent())) {
    r.detail = "the archive preserved only " + std::to_string(distinct.size()) +
               " of " + std::to_string(vertices.Extent()) +
               " named vertex identities";
    return;
  }

  r.detail = "vertices=8 children=9 distinct=8 archive_bytes=" +
             std::to_string(bytes.size());
  r.pass = true;
}

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
  results.reserve(10);

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
  results.push_back(RunStep(9, "cap_vertices", [&](StepResult& r) {
    Step9_CapVertices(r);
  }));
  results.push_back(RunStep(10, "vertex_archive", [&](StepResult& r) {
    Step10_NamedVertexArchive(r);
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
