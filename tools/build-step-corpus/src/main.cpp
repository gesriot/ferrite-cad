// SPDX-License-Identifier: MIT
//
// Writes the synthetic STEP conformance corpus, and says what it wrote.
//
// Everything here is generated, so every file is FerriteCAD's own and can be
// redistributed without asking anyone. That is the point of it, and also its
// limit: a file written by Open CASCADE and read by Open CASCADE proves a
// round trip, not that a reader copes with what SolidWorks or NX produce.
// Those belong to a separate interoperability corpus and must not quietly
// become this one.
//
// Two things are deliberate and neither is obvious:
//
//   * The header timestamp is set through the STEP model before writing.
//     Left alone it is the wall clock, an irrelevant difference on every
//     regeneration even when the semantic model stays identical.
//
//   * Colours are written as sRGB and come back linear, because that is what
//     Quantity_Color stores. The expectation file carries the sRGB that was
//     asked for, the linear value computed independently from the standard
//     formula, and what the reader actually returned — so a mistake shared by
//     the writer and the reader cannot pass as agreement.

#include <BRepGProp.hxx>
#include <BRepBndLib.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <Bnd_Box.hxx>
#include <APIHeaderSection_MakeHeader.hxx>
#include <DESTEP_Parameters.hxx>
#include <GProp_GProps.hxx>
#include <HeaderSection_FileSchema.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_EntityIterator.hxx>
#include <Interface_HArray1OfHAsciiString.hxx>
#include <Quantity_TypeOfColor.hxx>
#include <STEPControl_StepModelType.hxx>
#include <Standard_Boolean.hxx>
#include <Standard_Version.hxx>
#include <TCollection_HAsciiString.hxx>
#include <UnitsMethods_LengthUnit.hxx>
#include <XCAFDoc_ColorType.hxx>
#include <gp_Vec.hxx>
#include <gp_XYZ.hxx>
#include <TColStd_SequenceOfAsciiString.hxx>
#include <TopLoc_Location.hxx>
#include <Quantity_Color.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <StepData_StepModel.hxx>
#include <TCollection_AsciiString.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Application.hxx>
#include <TDocStd_Document.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS_Shape.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Trsf.hxx>

#include <algorithm>
#include <cmath>
#include <functional>
#include <cstdio>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

namespace {

/// The date every file in the corpus claims to have been written.
///
/// A constant removes the wall clock. It does not promise byte-identical STEP:
/// OCCT emits independent colour entities in a varying order.
constexpr const char *FIXED_TIMESTAMP = "2020-01-01T00:00:00";

constexpr double COLOUR_TOLERANCE = 1e-4;
constexpr double PLACEMENT_TOLERANCE = 1e-7;
constexpr const char *AP242_SCHEMA =
    "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF {1 0 10303 442 1 1 4 }";

/// One colour, in the space it was asked for and the space it is stored in.
struct Colour {
  double srgb[3];
};

/// The standard sRGB to linear transfer function.
///
/// Written out rather than asked of Open CASCADE: the expectation has to be
/// arrived at independently, or the corpus only proves that the writer and
/// the reader agree with each other.
double to_linear(double channel) {
  return channel <= 0.04045 ? channel / 12.92
                            : std::pow((channel + 0.055) / 1.055, 2.4);
}

/// A label's name, or a marker that it has none.
std::string name_of(const TDF_Label &label) {
  Handle(TDataStd_Name) attribute;
  if (!label.FindAttribute(TDataStd_Name::GetID(), attribute)) {
    return "(unnamed)";
  }
  // Extended to UTF-8 rather than to ASCII: the corpus deliberately carries
  // names that ASCII cannot hold, and converting through it would turn the
  // test into a test of the conversion.
  TCollection_AsciiString utf8;
  utf8 = TCollection_AsciiString(attribute->Get(), Standard_False);
  return utf8.ToCString();
}

/// Whether Open CASCADE considers the shape well formed.
bool is_sound(const TopoDS_Shape &shape) {
  BRepCheck_Analyzer analyzer(shape);
  return analyzer.IsValid() == Standard_True;
}

void name_it(const TDF_Label &label, const char *text) {
  TDataStd_Name::Set(label, TCollection_ExtendedString(text, Standard_True));
}

/// One shape in the tree, as it was actually read back.
///
/// The path is what makes an entry addressable without depending on the order
/// Open CASCADE happened to write anything in: it is the chain of names from
/// the root, so two generations of the same model produce the same path even
/// when the file's entity numbering differs.
struct Node {
  std::string path;
  std::string name;
  bool is_assembly = false;
  bool has_colour = false;
  double linear[3] = {0.0, 0.0, 0.0};
  double translation[3] = {0.0, 0.0, 0.0};
  int solids = 0;
  double volume = 0.0;
  double box[6] = {0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
  bool valid = false;
};

/// One node the generator intended to put into a file.
///
/// Kept apart from `Node`: comparing two values both read by OCCT would only
/// show that two readers agree. This side is the independently stated intent.
struct ExpectedNode {
  const char *path;
  double translation[3];
  bool is_assembly;
  const Colour *colour;
};

/// Everything read back out of one file.
struct Observed {
  std::string file;
  std::string description;
  std::string schema;
  std::string source_unit;
  int roots = 0;
  std::vector<Node> nodes;
  std::vector<ExpectedNode> expected_nodes;
};

bool nearly_equal(double a, double b, double tolerance) {
  return std::abs(a - b) <= tolerance;
}

bool same_placement(const double a[3], const double b[3]) {
  return nearly_equal(a[0], b[0], PLACEMENT_TOLERANCE) &&
         nearly_equal(a[1], b[1], PLACEMENT_TOLERANCE) &&
         nearly_equal(a[2], b[2], PLACEMENT_TOLERANCE);
}

int count_solids(const TopoDS_Shape &shape) {
  int solids = 0;
  for (TopExp_Explorer it(shape, TopAbs_SOLID); it.More(); it.Next()) {
    ++solids;
  }
  return solids;
}

double volume_of(const TopoDS_Shape &shape) {
  GProp_GProps props;
  BRepGProp::VolumeProperties(shape, props);
  return props.Mass();
}

}  // namespace

int main(int argc, char **argv) {
  if (argc < 2) {
    std::fprintf(stderr, "usage: step_corpus <output-directory>\n");
    return 2;
  }
  const std::string out = argv[1];

  Handle(TDocStd_Application) app = new TDocStd_Application();
  std::vector<Observed> results;

  auto write_file = [&](const Handle(TDocStd_Document) & doc,
                        const std::string &name,
                        DESTEP_Parameters::WriteMode_StepSchema schema,
                        UnitsMethods_LengthUnit unit,
                        const char *expected_source_unit, int expected_roots,
                        const std::string &description,
                        const std::vector<ExpectedNode> &expected_nodes)
      -> bool {
    // Passed explicitly rather than through Interface_Static. Setting the
    // schema globally before the first writer is constructed does nothing:
    // the controller initialises afterwards and puts it back to AP214IS, and
    // the file says AUTOMOTIVE_DESIGN while the caller believes otherwise.
    DESTEP_Parameters parameters;
    parameters.WriteSchema = schema;
    parameters.WriteUnit = unit;

    STEPCAFControl_Writer writer;
    writer.SetNameMode(Standard_True);
    writer.SetColorMode(Standard_True);
    if (!writer.Transfer(doc, parameters, STEPControl_AsIs)) {
      std::fprintf(stderr, "%s: transfer failed\n", name.c_str());
      return false;
    }

    // Through the model rather than by editing the file afterwards: the
    // header is a STEP entity, and rewriting text in a finished file is a
    // guess about its layout.
    Handle(StepData_StepModel) model = writer.ChangeWriter().Model();
    if (model.IsNull()) {
      std::fprintf(stderr, "%s: no model to stamp\n", name.c_str());
      return false;
    }
    APIHeaderSection_MakeHeader header(model);
    header.SetTimeStamp(new TCollection_HAsciiString(FIXED_TIMESTAMP));
    header.SetName(new TCollection_HAsciiString(name.c_str()));
    header.SetOriginatingSystem(
        new TCollection_HAsciiString("FerriteCAD synthetic STEP corpus"));
    // The writer's version is part of where this file came from, so it is
    // recorded rather than hidden. What is removed is the wall clock, which
    // says nothing about the model.
    header.SetPreprocessorVersion(new TCollection_HAsciiString(
        (std::string("FerriteCAD STEP corpus v1 / OCCT ") + OCC_VERSION_COMPLETE)
            .c_str()));

    const std::string path = out + "/" + name;
    if (writer.Write(path.c_str()) != IFSelect_RetDone) {
      std::fprintf(stderr, "%s: write failed\n", name.c_str());
      return false;
    }

    // Read back. Everything below comes out of the file; nothing is copied
    // from what was intended, because a golden built from intent tests only
    // that the intent was written down twice.
    Handle(TDocStd_Document) back;
    app->NewDocument("BinXCAF", back);
    STEPCAFControl_Reader reader;
    reader.SetNameMode(Standard_True);
    reader.SetColorMode(Standard_True);
    if (reader.ReadFile(path.c_str()) != IFSelect_RetDone ||
        !reader.Transfer(back)) {
      std::fprintf(stderr, "%s: does not read back\n", name.c_str());
      return false;
    }

    Observed observed;
    observed.file = name;
    observed.description = description;
    observed.expected_nodes = expected_nodes;

    // The schema the file actually declares, not the one that was requested.
    // Asking the model rather than trusting the parameter is the whole point:
    // a schema set through Interface_Static before the controller starts is
    // silently replaced, and the file says AP214 while the caller believes
    // AP242.
    Handle(StepData_StepModel) read_model =
        Handle(StepData_StepModel)::DownCast(reader.Reader().Model());
    if (!read_model.IsNull()) {
      Interface_EntityIterator header = read_model->Header();
      for (header.Start(); header.More(); header.Next()) {
        Handle(HeaderSection_FileSchema) schema_entity =
            Handle(HeaderSection_FileSchema)::DownCast(header.Value());
        if (schema_entity.IsNull() ||
            schema_entity->SchemaIdentifiers().IsNull()) {
          continue;
        }
        Handle(Interface_HArray1OfHAsciiString) names =
            schema_entity->SchemaIdentifiers();
        for (int i = names->Lower(); i <= names->Upper(); ++i) {
          if (names->Value(i).IsNull()) {
            continue;
          }
          if (!observed.schema.empty()) {
            observed.schema += " + ";
          }
          observed.schema += names->Value(i)->ToCString();
        }
      }
    }
    if (observed.schema.empty()) {
      observed.schema = "(not declared)";
    }

    // And the unit the file was written in, read from the file.
    TColStd_SequenceOfAsciiString lengths;
    TColStd_SequenceOfAsciiString angles;
    TColStd_SequenceOfAsciiString solid_angles;
    reader.ChangeReader().FileUnits(lengths, angles, solid_angles);
    observed.source_unit =
        lengths.IsEmpty() ? "(none declared)" : lengths.First().ToCString();

    Handle(XCAFDoc_ShapeTool) shapes =
        XCAFDoc_DocumentTool::ShapeTool(back->Main());
    Handle(XCAFDoc_ColorTool) colours =
        XCAFDoc_DocumentTool::ColorTool(back->Main());

    // Walks the tree, naming each node by the path of names that reaches it.
    std::function<void(const TDF_Label &, const std::string &, const TopLoc_Location &)>
        walk = [&](const TDF_Label &label, const std::string &prefix,
                   const TopLoc_Location &placement) {
          TDF_Label definition = label;
          if (shapes->IsReference(label)) {
            shapes->GetReferredShape(label, definition);
          }

          Node node;
          node.name = name_of(definition);
          node.path = prefix.empty() ? node.name : prefix + "/" + node.name;
          node.is_assembly = shapes->IsAssembly(definition) == Standard_True;

          Quantity_Color colour;
          // The instance is asked first: a component may be painted over its
          // definition, and a reader that looks only at definitions loses it.
          if (colours->GetColor(label, XCAFDoc_ColorSurf, colour) ||
              colours->GetColor(definition, XCAFDoc_ColorSurf, colour)) {
            node.has_colour = true;
            node.linear[0] = colour.Red();
            node.linear[1] = colour.Green();
            node.linear[2] = colour.Blue();
          }

          const gp_XYZ offset = placement.Transformation().TranslationPart();
          node.translation[0] = offset.X();
          node.translation[1] = offset.Y();
          node.translation[2] = offset.Z();

          const TopoDS_Shape shape = shapes->GetShape(definition);
          if (!shape.IsNull()) {
            node.solids = count_solids(shape);
            node.volume = volume_of(shape);
            node.valid = is_sound(shape);
            Bnd_Box box;
            BRepBndLib::Add(shape, box);
            if (!box.IsVoid()) {
              box.Get(node.box[0], node.box[1], node.box[2], node.box[3],
                      node.box[4], node.box[5]);
            }
          }
          observed.nodes.push_back(node);

          TDF_LabelSequence children;
          shapes->GetComponents(definition, children);
          for (int i = 1; i <= children.Length(); ++i) {
            walk(children.Value(i), node.path,
                 shapes->GetShape(children.Value(i)).Location());
          }
        };

    TDF_LabelSequence roots;
    shapes->GetFreeShapes(roots);
    observed.roots = roots.Length();
    for (int i = 1; i <= roots.Length(); ++i) {
      walk(roots.Value(i), std::string(), TopLoc_Location());
    }

    // A file with the right names and no geometry is a thing Open CASCADE
    // will happily produce, so this is checked rather than assumed.
    int solids = 0;
    for (const Node &node : observed.nodes) {
      solids += node.solids;
    }
    if (solids == 0) {
      std::fprintf(stderr, "%s: read back with no solids\n", name.c_str());
      return false;
    }

    // Sorted by path, so the manifest does not depend on the order anything
    // was written or walked in.
    std::sort(observed.nodes.begin(), observed.nodes.end(),
              [](const Node &a, const Node &b) {
                if (a.path != b.path) {
                  return a.path < b.path;
                }
                return std::lexicographical_compare(
                    a.translation, a.translation + 3, b.translation,
                    b.translation + 3);
              });

    // Agreement between platforms is necessary but not sufficient: three
    // readers can lose the same name or colour in exactly the same way. Check
    // the read-back model against the independently stated fixture intent.
    if (observed.schema != AP242_SCHEMA) {
      std::fprintf(stderr, "%s: declared schema is %s, expected %s\n",
                   name.c_str(), observed.schema.c_str(), AP242_SCHEMA);
      return false;
    }
    if (observed.source_unit != expected_source_unit) {
      std::fprintf(stderr, "%s: source unit is %s, expected %s\n",
                   name.c_str(), observed.source_unit.c_str(),
                   expected_source_unit);
      return false;
    }
    if (observed.roots != expected_roots) {
      std::fprintf(stderr, "%s: read %d roots, expected %d\n", name.c_str(),
                   observed.roots, expected_roots);
      return false;
    }
    if (observed.nodes.size() != expected_nodes.size()) {
      std::fprintf(stderr, "%s: read %zu nodes, expected %zu\n", name.c_str(),
                   observed.nodes.size(), expected_nodes.size());
      return false;
    }

    std::vector<bool> used(observed.nodes.size(), false);
    for (const ExpectedNode &expected : expected_nodes) {
      std::size_t found = observed.nodes.size();
      for (std::size_t i = 0; i < observed.nodes.size(); ++i) {
        if (!used[i] && observed.nodes[i].path == expected.path &&
            same_placement(observed.nodes[i].translation,
                           expected.translation)) {
          found = i;
          break;
        }
      }
      if (found == observed.nodes.size()) {
        std::fprintf(stderr,
                     "%s: expected node %s at (%.4f, %.4f, %.4f) was lost\n",
                     name.c_str(), expected.path, expected.translation[0],
                     expected.translation[1], expected.translation[2]);
        return false;
      }
      used[found] = true;
      const Node &actual = observed.nodes[found];
      if (actual.is_assembly != expected.is_assembly) {
        std::fprintf(stderr, "%s: %s changed between part and assembly\n",
                     name.c_str(), expected.path);
        return false;
      }
      if (!actual.valid || actual.solids <= 0 || !(actual.volume > 0.0)) {
        std::fprintf(stderr,
                     "%s: %s has invalid or empty geometry after read-back\n",
                     name.c_str(), expected.path);
        return false;
      }
      if ((expected.colour != nullptr) != actual.has_colour) {
        std::fprintf(stderr, "%s: %s did not preserve colour presence\n",
                     name.c_str(), expected.path);
        return false;
      }
      if (expected.colour != nullptr) {
        for (int channel = 0; channel < 3; ++channel) {
          const double wanted = to_linear(expected.colour->srgb[channel]);
          if (!nearly_equal(actual.linear[channel], wanted,
                            COLOUR_TOLERANCE)) {
            std::fprintf(stderr,
                         "%s: %s colour channel %d is %.9f, expected %.9f\n",
                         name.c_str(), expected.path, channel,
                         actual.linear[channel], wanted);
            return false;
          }
        }
      }
    }

    results.push_back(observed);
    std::printf("  %-30s schema=%-18s unit=%-12s nodes=%zu solids=%d\n",
                name.c_str(), observed.schema.c_str(),
                observed.source_unit.c_str(), observed.nodes.size(), solids);
    return true;
  };

  // The colours the corpus uses, named so an expectation can be read.
  const Colour red{{0.80, 0.20, 0.20}};
  const Colour blue{{0.20, 0.40, 0.90}};
  const Colour green{{0.15, 0.70, 0.35}};

  auto set_colour = [&](const Handle(XCAFDoc_ColorTool) & tool,
                        const TDF_Label &label, const Colour &colour) {
    tool->SetColor(label,
                   Quantity_Color(colour.srgb[0], colour.srgb[1],
                                  colour.srgb[2], Quantity_TOC_sRGB),
                   XCAFDoc_ColorSurf);
  };

  bool ok = true;

  // 1. One part, nothing else. The floor: if this does not read back,
  //    nothing below means anything.
  {
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);
    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    TDF_Label part = shapes->AddShape(BRepPrimAPI_MakeBox(60, 40, 10).Shape(), Standard_False);
    name_it(part, "Plate");
    shapes->UpdateAssemblies();
    ok &= write_file(doc, "01-single-part.step",
                     DESTEP_Parameters::WriteMode_StepSchema_AP242DIS,
                     UnitsMethods_LengthUnit_Millimeter, "millimetre", 1,
                     "One named solid, no assembly, no colour.",
                     {{"Plate", {0.0, 0.0, 0.0}, false, nullptr}});
  }

  // 2. A flat assembly of two named, coloured parts.
  {
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);
    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    Handle(XCAFDoc_ColorTool) colours = XCAFDoc_DocumentTool::ColorTool(doc->Main());

    TDF_Label base = shapes->AddShape(BRepPrimAPI_MakeBox(60, 40, 10).Shape(), Standard_False);
    name_it(base, "Base");
    set_colour(colours, base, red);
    TDF_Label pin = shapes->AddShape(BRepPrimAPI_MakeCylinder(5, 25).Shape(), Standard_False);
    name_it(pin, "Pin");
    set_colour(colours, pin, blue);

    TDF_Label assembly = shapes->NewShape();
    name_it(assembly, "Bracket");
    shapes->AddComponent(assembly, base, TopLoc_Location());
    gp_Trsf up;
    up.SetTranslation(gp_Vec(30, 20, 10));
    shapes->AddComponent(assembly, pin, TopLoc_Location(up));
    shapes->UpdateAssemblies();

    ok &= write_file(
        doc, "02-flat-assembly.step",
        DESTEP_Parameters::WriteMode_StepSchema_AP242DIS,
        UnitsMethods_LengthUnit_Millimeter, "millimetre", 1,
        "Two named, coloured parts in one assembly.",
        {{"Bracket", {0.0, 0.0, 0.0}, true, nullptr},
         {"Bracket/Base", {0.0, 0.0, 0.0}, false, &red},
         {"Bracket/Pin", {30.0, 20.0, 10.0}, false, &blue}});
  }

  // 3. Nesting: an assembly whose component is itself an assembly.
  {
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);
    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    Handle(XCAFDoc_ColorTool) colours = XCAFDoc_DocumentTool::ColorTool(doc->Main());

    TDF_Label body = shapes->AddShape(BRepPrimAPI_MakeBox(20, 20, 20).Shape(), Standard_False);
    name_it(body, "Cube");
    set_colour(colours, body, green);

    TDF_Label inner = shapes->NewShape();
    name_it(inner, "InnerGroup");
    shapes->AddComponent(inner, body, TopLoc_Location());
    gp_Trsf across;
    across.SetTranslation(gp_Vec(30, 0, 0));
    shapes->AddComponent(inner, body, TopLoc_Location(across));

    TDF_Label outer = shapes->NewShape();
    name_it(outer, "OuterGroup");
    shapes->AddComponent(outer, inner, TopLoc_Location());
    gp_Trsf lift;
    lift.SetTranslation(gp_Vec(0, 40, 0));
    shapes->AddComponent(outer, inner, TopLoc_Location(lift));
    shapes->UpdateAssemblies();

    ok &= write_file(
        doc, "03-nested-assembly.step",
        DESTEP_Parameters::WriteMode_StepSchema_AP242DIS,
        UnitsMethods_LengthUnit_Millimeter, "millimetre", 1,
        "Two levels of nesting, one part reused four times.",
        {{"OuterGroup", {0.0, 0.0, 0.0}, true, nullptr},
         {"OuterGroup/InnerGroup", {0.0, 0.0, 0.0}, true, nullptr},
         {"OuterGroup/InnerGroup", {0.0, 40.0, 0.0}, true, nullptr},
         {"OuterGroup/InnerGroup/Cube", {0.0, 0.0, 0.0}, false,
          &green},
         {"OuterGroup/InnerGroup/Cube", {30.0, 0.0, 0.0}, false,
          &green},
         {"OuterGroup/InnerGroup/Cube", {0.0, 0.0, 0.0}, false,
          &green},
         {"OuterGroup/InnerGroup/Cube", {30.0, 0.0, 0.0}, false,
          &green}});
  }

  // 4. The same definition instanced with different transforms, which is
  //    where a reader that confuses a definition with an instance shows it.
  {
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);
    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    Handle(XCAFDoc_ColorTool) colours = XCAFDoc_DocumentTool::ColorTool(doc->Main());

    TDF_Label bolt = shapes->AddShape(BRepPrimAPI_MakeCylinder(3, 12).Shape(), Standard_False);
    name_it(bolt, "Bolt");
    set_colour(colours, bolt, red);

    TDF_Label pattern = shapes->NewShape();
    name_it(pattern, "BoltPattern");
    for (int i = 0; i < 4; ++i) {
      gp_Trsf placed;
      placed.SetTranslation(gp_Vec(i * 15.0, (i % 2) * 15.0, 0.0));
      TDF_Label instance = shapes->AddComponent(pattern, bolt, TopLoc_Location(placed));
      // The third instance is painted over, which is the case a reader has
      // to keep apart from the definition's own colour.
      if (i == 2) {
        set_colour(colours, instance, blue);
      }
    }
    shapes->UpdateAssemblies();

    ok &= write_file(
        doc, "04-instance-colours.step",
        DESTEP_Parameters::WriteMode_StepSchema_AP242DIS,
        UnitsMethods_LengthUnit_Millimeter, "millimetre", 1,
        "One definition, four placements, one instance recoloured.",
        {{"BoltPattern", {0.0, 0.0, 0.0}, true, nullptr},
         {"BoltPattern/Bolt", {0.0, 0.0, 0.0}, false, &red},
         {"BoltPattern/Bolt", {15.0, 15.0, 0.0}, false, &red},
         {"BoltPattern/Bolt", {30.0, 0.0, 0.0}, false, &blue},
         {"BoltPattern/Bolt", {45.0, 15.0, 0.0}, false, &red}});
  }

  // 5. The same model in inches, so a reader that ignores units is caught by
  //    a number rather than by a shrug.
  {
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);
    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    TDF_Label part = shapes->AddShape(BRepPrimAPI_MakeBox(50.8, 25.4, 12.7).Shape(), Standard_False);
    name_it(part, "InchPlate");
    shapes->UpdateAssemblies();
    ok &= write_file(doc, "05-inch-units.step",
                     DESTEP_Parameters::WriteMode_StepSchema_AP242DIS,
                     UnitsMethods_LengthUnit_Inch, "INCH", 1,
                     "A 2 x 1 x 0.5 inch plate, written in inches.",
                     {{"InchPlate", {0.0, 0.0, 0.0}, false, nullptr}});
  }

  // 6. Names outside ASCII. A reader that assumes one byte per character
  //    mangles these rather than failing, which is worse.
  {
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);
    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    TDF_Label left = shapes->AddShape(BRepPrimAPI_MakeBox(20, 20, 20).Shape(), Standard_False);
    name_it(left, "Кронштейн");
    TDF_Label right = shapes->AddShape(BRepPrimAPI_MakeBox(10, 10, 30).Shape(), Standard_False);
    name_it(right, "Épaisseur — 30µm");

    TDF_Label assembly = shapes->NewShape();
    name_it(assembly, "組立て");
    shapes->AddComponent(assembly, left, TopLoc_Location());
    gp_Trsf across;
    across.SetTranslation(gp_Vec(30, 0, 0));
    shapes->AddComponent(assembly, right, TopLoc_Location(across));
    shapes->UpdateAssemblies();

    ok &= write_file(
        doc, "06-unicode-names.step",
        DESTEP_Parameters::WriteMode_StepSchema_AP242DIS,
        UnitsMethods_LengthUnit_Millimeter, "millimetre", 1,
        "Cyrillic, accented Latin and Japanese names.",
        {{"組立て", {0.0, 0.0, 0.0}, true, nullptr},
         {"組立て/Кронштейн", {0.0, 0.0, 0.0}, false, nullptr},
         {"組立て/Épaisseur — 30µm", {30.0, 0.0, 0.0}, false,
          nullptr}});
  }

  // 7. Nothing but geometry: no explicit name, colour or assembly. The STEP
  //    writer supplies the product name SOLID; that default is recorded rather
  //    than misreported as absent metadata.
  {
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);
    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    shapes->AddShape(BRepPrimAPI_MakeBox(15, 15, 15).Shape(), Standard_False);
    shapes->UpdateAssemblies();
    ok &= write_file(doc, "07-bare-geometry.step",
                     DESTEP_Parameters::WriteMode_StepSchema_AP242DIS,
                     UnitsMethods_LengthUnit_Millimeter, "millimetre", 1,
                     "One solid with writer-default name and no colour.",
                     {{"SOLID", {0.0, 0.0, 0.0}, false, nullptr}});
  }

  if (!ok) {
    std::fprintf(stderr, "the corpus is incomplete\n");
    return 1;
  }

  // The manifest. Observed values came out of the file; expected colours are
  // computed independently from the requested sRGB and were already compared
  // above. Keeping both makes the numerical contract visible to a reviewer.
  std::sort(results.begin(), results.end(),
            [](const Observed &a, const Observed &b) { return a.file < b.file; });

  std::ostringstream manifest;
  manifest
      << "# What the STEP corpus contains, as read back out of it.\n#\n"
      << "# Generated by tools/build-step-corpus. Two generations of the\n"
      << "# corpus must produce this file identically; the STEP files\n"
      << "# themselves need not be byte-identical, because Open CASCADE\n"
      << "# writes colour statements in an order that varies between runs.\n"
      << "# What is compared is the model, not the encoding of it.\n#\n"
      << "# Entries are sorted by file and by path, so nothing here depends\n"
      << "# on the order anything was written or walked in.\n\n";

  for (const Observed &item : results) {
    manifest << item.file << "\n";
    manifest << "    " << item.description << "\n";
    manifest << "    schema " << item.schema << "\n";
    manifest << "    source unit " << item.source_unit << "\n";
    manifest << "    roots " << item.roots << "\n";
    for (const ExpectedNode &expected : item.expected_nodes) {
      if (expected.colour == nullptr) {
        continue;
      }
      char line[512];
      std::snprintf(
          line, sizeof(line),
          "    expected colour %s at (%.4f, %.4f, %.4f) = "
          "sRGB(%.4f, %.4f, %.4f) -> linear(%.6f, %.6f, %.6f) +/- %.0e",
          expected.path, expected.translation[0], expected.translation[1],
          expected.translation[2], expected.colour->srgb[0],
          expected.colour->srgb[1], expected.colour->srgb[2],
          to_linear(expected.colour->srgb[0]),
          to_linear(expected.colour->srgb[1]),
          to_linear(expected.colour->srgb[2]), COLOUR_TOLERANCE);
      manifest << line << "\n";
    }
    for (const Node &node : item.nodes) {
      char line[512];
      std::snprintf(line, sizeof(line),
                    "    %-44s %-9s solids %d volume %12.4f valid %s",
                    node.path.c_str(),
                    node.is_assembly ? "assembly" : "part", node.solids,
                    node.volume, node.valid ? "yes" : "no");
      manifest << line << "\n";
      std::snprintf(line, sizeof(line),
                    "        at (%.4f, %.4f, %.4f)  box (%.4f, %.4f, %.4f) to "
                    "(%.4f, %.4f, %.4f)",
                    node.translation[0], node.translation[1],
                    node.translation[2], node.box[0], node.box[1], node.box[2],
                    node.box[3], node.box[4], node.box[5]);
      manifest << line << "\n";
      if (node.has_colour) {
        std::snprintf(line, sizeof(line),
                      "        colour linear(%.6f, %.6f, %.6f)", node.linear[0],
                      node.linear[1], node.linear[2]);
        manifest << line << "\n";
      }
    }
    manifest << "\n";
  }

  const std::string manifest_path = out + "/SEMANTIC-MANIFEST.txt";
  std::ofstream file(manifest_path, std::ios::binary);
  if (!file) {
    std::fprintf(stderr, "could not write %s\n", manifest_path.c_str());
    return 1;
  }
  file << manifest.str();
  file.close();

  std::printf("corpus written to %s\n", out.c_str());
  return 0;
}
