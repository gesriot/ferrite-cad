// SPDX-License-Identifier: MIT
//
// Reports what Open CASCADE makes of a STEP file, and what it complained
// about on the way.
//
// This exists because the obvious question — "did the import work?" — has no
// single answer. Measured on the corpus: two of five damaged files are
// refused outright, and three are read successfully and produce the same
// geometry as the undamaged original. Of those three, two are described
// precisely in the reader's check list and one passes without a word.
//
// So a policy cannot be built on the return status, and cannot be built on
// the check list alone either. What this tool does is collect everything that
// is available for this corpus — the read status, the diagnostics from loading,
// whether the transfer succeeded, the diagnostics from transferring, and the
// XDE semantics exercised by the fixtures — and print it in a form that can be
// compared between platforms and between Open CASCADE versions.
//
// It reports. It does not decide: the policy lives in FerriteCAD, and its
// job is to be built on this rather than on a guess.

#include <BRepBndLib.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepGProp.hxx>
#include <Bnd_Box.hxx>
#include <GProp_GProps.hxx>
#include <HeaderSection_FileSchema.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Check.hxx>
#include <Interface_CheckIterator.hxx>
#include <Interface_EntityIterator.hxx>
#include <Interface_HArray1OfHAsciiString.hxx>
#include <Interface_InterfaceModel.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Quantity_Color.hxx>
#include <Quantity_TypeOfColor.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <IFSelect_PrintCount.hxx>
#include <STEPControl_Reader.hxx>
#include <StepData_StepModel.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <TCollection_AsciiString.hxx>
#include <TColStd_SequenceOfAsciiString.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Application.hxx>
#include <TDocStd_Document.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS_Shape.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_ColorType.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Trsf.hxx>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <functional>
#include <iostream>
#include <iterator>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

/// What a check report said, counted and kept.
struct Diagnostics {
  int fails = 0;
  int warnings = 0;
  std::vector<std::string> messages;
};

/// Splits Open CASCADE's own check report into countable lines.
///
/// Taken from `PrintCheckLoad` and `PrintCheckTransfer` rather than from the
/// check iterators behind them: those are the documented way to ask, they
/// keep what loading noticed apart from what transferring noticed, and they
/// do not move between releases the way the internals do.
///
/// `IFSelect_CountByItem` is the mode that groups by message, one line each:
///
///     Count	Check Model Complete Check List
///     -----	-----------
///         1	F: Unresolved Reference, Ent.Id.#1 Param.n0 4 (Id.#9999999)
///         1	W: some warning
///        Nb Total:3  for 3 items
///
/// Note `F:` and `W:` — the words "fail" and "warning" do not appear at all,
/// which is how the first version of this function reported nothing wrong
/// with a file carrying three unresolved references.
Diagnostics parse_report(const std::string &report) {
  Diagnostics found;
  std::istringstream lines(report);
  std::string line;
  while (std::getline(lines, line)) {
    while (!line.empty() && (line.back() == '\r' || line.back() == ' ')) {
      line.pop_back();
    }
    const std::size_t tab = line.find('\t');
    if (tab == std::string::npos) {
      continue;
    }

    const std::string count_text = line.substr(0, tab);
    const std::string message = line.substr(tab + 1);
    int count = 0;
    try {
      count = std::stoi(count_text);
    } catch (const std::exception &) {
      continue;  // The header row, which counts nothing.
    }
    if (count <= 0) {
      continue;
    }

    if (message.rfind("F:", 0) == 0) {
      found.fails += count;
    } else if (message.rfind("W:", 0) == 0) {
      found.warnings += count;
    } else {
      continue;
    }
    found.messages.push_back(std::to_string(count) + " x " + message);
  }
  // Sorted so two runs, and two platforms, report the same list in the same
  // order however the entities happened to be visited.
  std::sort(found.messages.begin(), found.messages.end());
  return found;
}

const char *status_name(IFSelect_ReturnStatus status) {
  switch (status) {
    case IFSelect_RetVoid:
      return "RetVoid";
    case IFSelect_RetDone:
      return "RetDone";
    case IFSelect_RetError:
      return "RetError";
    case IFSelect_RetFail:
      return "RetFail";
    case IFSelect_RetStop:
      return "RetStop";
  }
  return "unknown";
}

std::string name_of(const TDF_Label &label) {
  Handle(TDataStd_Name) attribute;
  if (!label.FindAttribute(TDataStd_Name::GetID(), attribute)) {
    return "(unnamed)";
  }
  // Kept as UTF-8: the corpus carries names ASCII cannot hold, and converting
  // through it would test the conversion instead of the reader.
  return TCollection_AsciiString(attribute->Get(), Standard_False).ToCString();
}

/// One node of the assembly, as read.
struct Node {
  std::string path;
  bool is_assembly = false;
  bool is_instance = false;
  TDF_Label definition;
  int definition_id = 0;
  bool has_colour = false;
  const char *colour_source = "none";
  double linear[3] = {0.0, 0.0, 0.0};
  double placement[12] = {0.0};
  int solids = 0;
  int invalid_solids = 0;
  double volume = 0.0;
  double box[6] = {0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
};

}  // namespace

int main(int argc, char **argv) {
  if (argc < 2) {
    std::fprintf(stderr, "usage: step_diagnostic <file.step>...\n");
    return 2;
  }

  // Open CASCADE prints to stdout by default, which would interleave with the
  // report and differ between platforms.
  Message::DefaultMessenger()->ChangePrinters().Clear();

  std::ostringstream out;
  out << "# What Open CASCADE " << OCC_VERSION_COMPLETE
      << " makes of each file\n#\n"
      << "# Read status, the diagnostics from loading, whether the transfer\n"
      << "# succeeded, the diagnostics from transferring, and the XDE\n"
      << "# semantics exercised by the corpus. Nothing here is a verdict: a file that\n"
      << "# reads without complaint is not thereby known to be sound.\n\n";

  for (int i = 1; i < argc; ++i) {
    const std::string path = argv[i];
    const std::size_t slash = path.find_last_of("/\\");
    const std::string name =
        slash == std::string::npos ? path : path.substr(slash + 1);

    out << name << "\n";

    Handle(TDocStd_Application) app = new TDocStd_Application();
    Handle(TDocStd_Document) doc;
    app->NewDocument("BinXCAF", doc);

    STEPCAFControl_Reader reader;
    reader.SetNameMode(Standard_True);
    reader.SetColorMode(Standard_True);

    IFSelect_ReturnStatus status = IFSelect_RetVoid;
    try {
      status = reader.ReadFile(path.c_str());
    } catch (const Standard_Failure &failure) {
      // Not DynamicType(): Open CASCADE 8.0 removed it from Standard_Failure,
      // which reparented to std::exception. GetMessageString is in both.
      out << "    read threw: " << failure.GetMessageString() << "\n\n";
      continue;
    }
    out << "    read " << status_name(status) << "\n";

    // Everything the loader noticed, before anything is built from it. Ask
    // even when ReadFile returned RetFail: an unsuccessful read may still
    // have a useful check list, and a dash here must not mean "we did not
    // look".
    std::ostringstream load_report;
    reader.Reader().PrintCheckLoad(load_report, Standard_False, IFSelect_CountByItem);
    const Diagnostics load = parse_report(load_report.str());
    out << "    load fails " << load.fails << " warnings " << load.warnings
        << "\n";
    for (const std::string &message : load.messages) {
      out << "        " << message << "\n";
    }
    if (status != IFSelect_RetDone) {
      out << "    nothing was transferred\n\n";
      continue;
    }

    bool transferred = false;
    try {
      transferred = reader.Transfer(doc) == Standard_True;
    } catch (const Standard_Failure &failure) {
      out << "    transfer threw: " << failure.GetMessageString() << "\n\n";
      continue;
    }
    out << "    transfer " << (transferred ? "succeeded" : "failed") << "\n";

    // Asked separately and after the transfer, because loading and
    // transferring notice different things and an earlier reading of one
    // list loses the other entirely.
    std::ostringstream transfer_report;
    reader.Reader().PrintCheckTransfer(transfer_report, Standard_False,
                                       IFSelect_CountByItem);
    const Diagnostics after = parse_report(transfer_report.str());
    out << "    transfer fails " << after.fails << " warnings " << after.warnings
        << "\n";
    for (const std::string &message : after.messages) {
      out << "        " << message << "\n";
    }
    if (!transferred) {
      out << "\n";
      continue;
    }

    // The schema the file declares, rather than the one anyone expected.
    std::string schema;
    Handle(StepData_StepModel) model =
        Handle(StepData_StepModel)::DownCast(reader.Reader().Model());
    if (!model.IsNull()) {
      Interface_EntityIterator header = model->Header();
      for (header.Start(); header.More(); header.Next()) {
        Handle(HeaderSection_FileSchema) entity =
            Handle(HeaderSection_FileSchema)::DownCast(header.Value());
        if (entity.IsNull() || entity->SchemaIdentifiers().IsNull()) {
          continue;
        }
        Handle(Interface_HArray1OfHAsciiString) identifiers =
            entity->SchemaIdentifiers();
        for (int k = identifiers->Lower(); k <= identifiers->Upper(); ++k) {
          if (identifiers->Value(k).IsNull()) {
            continue;
          }
          if (!schema.empty()) {
            schema += " + ";
          }
          schema += identifiers->Value(k)->ToCString();
        }
      }
    }
    out << "    schema " << (schema.empty() ? "(not declared)" : schema) << "\n";

    TColStd_SequenceOfAsciiString lengths;
    TColStd_SequenceOfAsciiString angles;
    TColStd_SequenceOfAsciiString solid_angles;
    reader.ChangeReader().FileUnits(lengths, angles, solid_angles);
    out << "    source unit "
        << (lengths.IsEmpty() ? "(none declared)" : lengths.First().ToCString())
        << "\n";

    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    Handle(XCAFDoc_ColorTool) colours = XCAFDoc_DocumentTool::ColorTool(doc->Main());

    std::vector<Node> nodes;
    std::function<void(const TDF_Label &, const std::string &, const TopLoc_Location &)>
        walk = [&](const TDF_Label &label, const std::string &prefix,
                   const TopLoc_Location &placement) {
          TDF_Label definition = label;
          if (shapes->IsReference(label)) {
            shapes->GetReferredShape(label, definition);
          }

          Node node;
          node.is_instance = shapes->IsReference(label) == Standard_True;
          const std::string own = name_of(definition);
          node.path = prefix.empty() ? own : prefix + "/" + own;
          node.is_assembly = shapes->IsAssembly(definition) == Standard_True;
          node.definition = definition;

          Quantity_Color colour;
          // The instance first: a component may be painted over its
          // definition, and a reader that looks only at definitions loses it.
          if (node.is_instance &&
              colours->GetColor(label, XCAFDoc_ColorSurf, colour)) {
            node.has_colour = true;
            node.colour_source = "instance";
            node.linear[0] = colour.Red();
            node.linear[1] = colour.Green();
            node.linear[2] = colour.Blue();
          } else if (colours->GetColor(definition, XCAFDoc_ColorSurf, colour)) {
            node.has_colour = true;
            node.colour_source = "definition";
            node.linear[0] = colour.Red();
            node.linear[1] = colour.Green();
            node.linear[2] = colour.Blue();
          }

          // XDE component locations are local to the parent. The tree plus
          // every local 3x4 affine matrix is the complete placement; keeping
          // only TranslationPart would silently lose rotations and scale.
          const gp_Trsf transform = placement.Transformation();
          for (int row = 1; row <= 3; ++row) {
            for (int column = 1; column <= 4; ++column) {
              double value = transform.Value(row, column);
              // Do not let a platform's choice of signed zero change an
              // otherwise identical diagnostic artefact.
              if (std::abs(value) < 0.0000005) {
                value = 0.0;
              }
              node.placement[(row - 1) * 4 + column - 1] = value;
            }
          }

          const TopoDS_Shape shape = shapes->GetShape(definition);
          if (!shape.IsNull()) {
            for (TopExp_Explorer it(shape, TopAbs_SOLID); it.More(); it.Next()) {
              ++node.solids;
              if (!BRepCheck_Analyzer(it.Current()).IsValid()) {
                ++node.invalid_solids;
              }
              GProp_GProps props;
              BRepGProp::VolumeProperties(it.Current(), props);
              node.volume += props.Mass();
            }
            Bnd_Box box;
            BRepBndLib::Add(shape, box);
            if (!box.IsVoid()) {
              box.Get(node.box[0], node.box[1], node.box[2], node.box[3],
                      node.box[4], node.box[5]);
            }
          }
          nodes.push_back(node);

          TDF_LabelSequence children;
          shapes->GetComponents(definition, children);
          for (int c = 1; c <= children.Length(); ++c) {
            walk(children.Value(c), node.path,
                 shapes->GetShape(children.Value(c)).Location());
          }
        };

    TDF_LabelSequence roots;
    shapes->GetFreeShapes(roots);
    out << "    roots " << roots.Length() << "\n";
    for (int r = 1; r <= roots.Length(); ++r) {
      walk(roots.Value(r), std::string(), TopLoc_Location());
    }

    std::sort(nodes.begin(), nodes.end(), [](const Node &a, const Node &b) {
      if (a.path != b.path) {
        return a.path < b.path;
      }
      return std::lexicographical_compare(a.placement, a.placement + 12,
                                          b.placement, b.placement + 12);
    });

    // Label entries are implementation details, so do not print them. Assign
    // a deterministic group number after sorting instead: equal numbers mean
    // that several occurrences refer to one definition, which distinguishes
    // the corpus' four Bolt instances from four copied solids.
    std::vector<TDF_Label> definitions;
    for (Node &node : nodes) {
      auto same = std::find_if(
          definitions.begin(), definitions.end(), [&](const TDF_Label &known) {
            return known.IsEqual(node.definition);
          });
      if (same == definitions.end()) {
        definitions.push_back(node.definition);
        node.definition_id = static_cast<int>(definitions.size());
      } else {
        node.definition_id =
            static_cast<int>(std::distance(definitions.begin(), same)) + 1;
      }
    }

    char line[512];
    for (const Node &node : nodes) {
      std::snprintf(line, sizeof(line),
                    "    %-40s %-9s definition %d %-10s solids %d invalid %d "
                    "volume %12.4f",
                    node.path.c_str(), node.is_assembly ? "assembly" : "part",
                    node.definition_id, node.is_instance ? "instance" : "direct",
                    node.solids, node.invalid_solids, node.volume);
      out << line << "\n";
      std::snprintf(line, sizeof(line),
                    "        local placement ((%.6f, %.6f, %.6f, %.6f), "
                    "(%.6f, %.6f, %.6f, %.6f), "
                    "(%.6f, %.6f, %.6f, %.6f))",
                    node.placement[0], node.placement[1], node.placement[2],
                    node.placement[3], node.placement[4], node.placement[5],
                    node.placement[6], node.placement[7], node.placement[8],
                    node.placement[9], node.placement[10], node.placement[11]);
      out << line << "\n";
      std::snprintf(line, sizeof(line),
                    "        box (%.4f, %.4f, %.4f) to (%.4f, %.4f, %.4f)",
                    node.box[0], node.box[1], node.box[2],
                    node.box[3], node.box[4], node.box[5]);
      out << line << "\n";
      if (node.has_colour) {
        std::snprintf(line, sizeof(line),
                      "        colour %s linear(%.6f, %.6f, %.6f)",
                      node.colour_source, node.linear[0], node.linear[1],
                      node.linear[2]);
        out << line << "\n";
      }
    }
    out << "\n";
  }

  std::cout << out.str();
  return 0;
}
