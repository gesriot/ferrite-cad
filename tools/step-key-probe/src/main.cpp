// SPDX-License-Identifier: MIT
//
// Asks whether a definition in a STEP file has an identity that survives
// being read again.
//
// Durable selection into an imported assembly — "this bolt, the one I picked"
// — needs a key that names a thing in the file rather than a thing in the
// reader. Four candidates are ruled out before measuring anything: a TDF_Label
// entry is a position in a document Open CASCADE built this run, a name is not
// unique and is often absent, a position in the definition list is what the
// current binding contract already refuses to trust, and geometry is a guess
// wearing a number. What is left is the STEP entity itself, and whether Open
// CASCADE will hand one back is a question about Open CASCADE.
//
// So this reports, for every definition of every corpus file:
//
//   * which entity produced its shape, and whether that entity carries the
//     identifier the file gave it (`#12`) or only the reader's own position in
//     the model (`(#12)`), which is not the same thing at all;
//   * the PRODUCT_DEFINITION reached from it, if any, and its identifier;
//   * the PRODUCT behind that, and the `id` string the file gave it.
//
// And then the three properties a key has to have, stated as measurements
// rather than hopes: present for every definition, unique within the file, and
// unchanged when the same bytes are read a second time in a second reader.
//
// It reports. It does not decide, and it does not pick a winner: a candidate
// that holds across the corpus and three platforms is evidence, and the
// decision belongs in FerriteCAD where it can be refused when the evidence
// runs out.
//
// The route it measures is the bridge's own, from
// `crates/ferritecad-occt-bridge/src/step_identity.hpp`, rather than a second
// implementation of it. A probe with its own copy would measure the probe.

#include "step_identity.hpp"

#include <IFSelect_ReturnStatus.hxx>
#include <Interface_EntityIterator.hxx>
#include <Interface_Graph.hxx>
#include <Interface_InterfaceModel.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <STEPControl_Reader.hxx>
#include <StepBasic_Product.hxx>
#include <StepBasic_ProductDefinition.hxx>
#include <StepBasic_ProductDefinitionFormation.hxx>
#include <StepBasic_ProductDefinitionRelationship.hxx>
#include <StepRepr_CharacterizedDefinition.hxx>
#include <StepRepr_NextAssemblyUsageOccurrence.hxx>
#include <StepRepr_PropertyDefinition.hxx>
#include <StepRepr_PropertyDefinitionRepresentation.hxx>
#include <StepRepr_ProductDefinitionShape.hxx>
#include <StepRepr_Representation.hxx>
#include <StepRepr_RepresentedDefinition.hxx>
#include <StepShape_AdvancedBrepShapeRepresentation.hxx>
#include <StepShape_ShapeDefinitionRepresentation.hxx>
#include <StepData_StepModel.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <TCollection_AsciiString.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Application.hxx>
#include <TDocStd_Document.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS_Shape.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <XSControl_TransferReader.hxx>
#include <XSControl_WorkSession.hxx>

#include <algorithm>
#include <cstdio>
#include <functional>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

namespace {

/// One candidate identifier, as read.
///
/// `source` is the whole point. Open CASCADE will happily print `(#12)` for an
/// entity whose identifier it made up from its own position in the model, and
/// a key built on that would be stable only for as long as nobody edited the
/// file above it. Only `#12` — an identifier the file itself wrote — counts.
struct Ident {
  bool found = false;
  bool source = false;
  int id = 0;
  std::string type;

  std::string text() const {
    if (!found) {
      return "-";
    }
    char buffer[64];
    std::snprintf(buffer, sizeof(buffer), source ? "#%d" : "(#%d)", id);
    return buffer;
  }
};

/// What was learned about one definition.
struct Probe {
  std::string name;
  Ident shape_entity;
  Ident product_definition;
  Ident product;
  std::string product_id;
};

std::string name_of(const TDF_Label &label) {
  Handle(TDataStd_Name) attribute;
  if (!label.FindAttribute(TDataStd_Name::GetID(), attribute)) {
    return "(unnamed)";
  }
  return TCollection_AsciiString(attribute->Get(), Standard_False).ToCString();
}

/// Reads the identifier the file gave an entity, if it gave one.
Ident ident_of(const Handle(StepData_StepModel) & model,
               const Handle(Standard_Transient) & entity) {
  Ident ident;
  if (model.IsNull() || entity.IsNull()) {
    return ident;
  }
  ident.found = true;
  ident.type = model->TypeName(entity, Standard_False);

  // IdentLabel is the identifier read from the file and 0 when there is none.
  // Number is where the entity sits in the model this run. Reporting them the
  // same way is how a reader convinces itself it has a source-level key when
  // it has a position.
  const int from_file = model->IdentLabel(entity);
  if (from_file > 0) {
    ident.source = true;
    ident.id = from_file;
  } else {
    ident.source = false;
    ident.id = model->Number(entity);
  }
  return ident;
}

/// Reads one file and probes every definition it describes.
///
/// Returns an empty vector when the file did not get far enough to have
/// definitions, which the caller reports rather than treating as a failure.
std::vector<Probe> probe(const std::string &path, std::string &complaint) {
  std::vector<Probe> found;

  Handle(TDocStd_Application) app = new TDocStd_Application();
  Handle(TDocStd_Document) doc;
  app->NewDocument("BinXCAF", doc);

  STEPCAFControl_Reader reader;
  reader.SetNameMode(Standard_True);
  reader.SetColorMode(Standard_True);

  try {
    if (reader.ReadFile(path.c_str()) != IFSelect_RetDone) {
      complaint = "the file was not read";
      return found;
    }
    if (reader.Transfer(doc) != Standard_True) {
      complaint = "nothing was transferred";
      return found;
    }
  } catch (const Standard_Failure &failure) {
    complaint = std::string("threw: ") + failure.GetMessageString();
    return found;
  }

  Handle(StepData_StepModel) model =
      Handle(StepData_StepModel)::DownCast(reader.Reader().Model());
  Handle(XSControl_WorkSession) session = reader.Reader().WS();
  Handle(XSControl_TransferReader) transfer =
      session.IsNull() ? Handle(XSControl_TransferReader)() : session->TransferReader();
  if (model.IsNull() || session.IsNull()) {
    complaint = "the reader kept no model to ask";
    return found;
  }
  const Interface_Graph &graph = session->Graph();

  Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());

  // Every distinct definition, reached the same way the importer reaches them:
  // through references, never by assuming the free shapes are all there is.
  std::vector<TDF_Label> definitions;
  // Which definitions sit inside which, by position in `definitions`. An
  // assembly has no geometry and so no entity of its own; the file names it
  // only through the components it contains, and this is what makes that
  // route available.
  std::vector<std::vector<std::size_t>> children_of;

  std::function<std::size_t(const TDF_Label &)> walk =
      [&](const TDF_Label &label) -> std::size_t {
    TDF_Label definition = label;
    if (shapes->IsReference(label)) {
      shapes->GetReferredShape(label, definition);
    }
    for (std::size_t i = 0; i < definitions.size(); ++i) {
      if (definitions[i].IsEqual(definition)) {
        return i;
      }
    }
    const std::size_t index = definitions.size();
    definitions.push_back(definition);
    children_of.emplace_back();

    TDF_LabelSequence children;
    shapes->GetComponents(definition, children);
    for (int c = 1; c <= children.Length(); ++c) {
      const std::size_t child = walk(children.Value(c));
      children_of[index].push_back(child);
    }
    return index;
  };

  TDF_LabelSequence roots;
  shapes->GetFreeShapes(roots);
  for (int r = 1; r <= roots.Length(); ++r) {
    walk(roots.Value(r));
  }

  // The bridge's own resolution, not a second copy of it: parts through the
  // representation holding their solid, assemblies through the occurrences
  // that put their components inside them.
  std::vector<TopoDS_Shape> definition_shapes;
  definition_shapes.reserve(definitions.size());
  for (const TDF_Label &definition : definitions) {
    definition_shapes.push_back(shapes->GetShape(definition));
  }
  const std::vector<Handle(StepBasic_ProductDefinition)> product_definitions =
      ferritecad::resolve_product_definitions(graph, transfer, definition_shapes,
                                              children_of);

  std::vector<Ident> shape_entities(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i) {
    const std::vector<Handle(Standard_Transient)> entities =
        ferritecad::entities_from(transfer, definition_shapes[i]);
    if (!entities.empty()) {
      shape_entities[i] = ident_of(model, entities.front());
    }
  }

  for (std::size_t i = 0; i < definitions.size(); ++i) {
    Probe entry;
    entry.name = name_of(definitions[i]);
    entry.shape_entity = shape_entities[i];
    entry.product_definition = ident_of(model, product_definitions[i]);

    const Handle(StepBasic_ProductDefinition) &product_definition =
        product_definitions[i];
    if (!product_definition.IsNull() && !product_definition->Formation().IsNull()) {
      Handle(StepBasic_Product) product =
          product_definition->Formation()->OfProduct();
      entry.product = ident_of(model, product);
      if (!product.IsNull() && !product->Id().IsNull()) {
        entry.product_id = product->Id()->ToCString();
      }
    }
    found.push_back(entry);
  }

  // Sorted so two reads, and two platforms, list the same definitions in the
  // same order however the document happened to be walked.
  std::sort(found.begin(), found.end(), [](const Probe &a, const Probe &b) {
    if (a.name != b.name) {
      return a.name < b.name;
    }
    return a.shape_entity.id < b.shape_entity.id;
  });
  return found;
}

/// The key a candidate would supply, as the text a comparison would use.
std::string key_of(const Probe &entry, int candidate) {
  switch (candidate) {
    case 0:
      return entry.shape_entity.source ? entry.shape_entity.text() : std::string();
    case 1:
      return entry.product_definition.source ? entry.product_definition.text()
                                             : std::string();
    case 2:
      return entry.product.source ? entry.product.text() : std::string();
    default:
      return std::string();
  }
}

const char *candidate_name(int candidate) {
  switch (candidate) {
    case 0:
      return "shape entity     ";
    case 1:
      return "product definition";
    case 2:
      return "product          ";
    default:
      return "?";
  }
}

}  // namespace

int main(int argc, char **argv) {
  if (argc < 2) {
    std::fprintf(stderr, "usage: step_key_probe <file.step>...\n");
    return 2;
  }

  // Open CASCADE prints to stdout by default, which would interleave with the
  // report and differ between platforms.
  Message::DefaultMessenger()->ChangePrinters().Clear();

  std::ostringstream out;
  out << "# Whether a definition has an identity that comes from the file\n#\n"
      << "# Measured with Open CASCADE " << OCC_VERSION_COMPLETE << ".\n"
      << "# `#12` is an identifier the file wrote. `(#12)` is where the entity\n"
      << "# sits in the model this run, which is not an identity and is never\n"
      << "# counted as a key below. A candidate is usable only where it is\n"
      << "# present for every definition, unique within the file, and identical\n"
      << "# when the same bytes are read again in a second reader.\n#\n"
      << "# This reports. What FerriteCAD does about it is decided elsewhere.\n\n";

  // Counted across the whole corpus, so a gate can be set on a stated fact
  // rather than on a human reading three columns per file.
  int with_definitions = 0;
  int usable[3] = {0, 0, 0};
  // Named, not just counted. A count says how many files kept a key; the names
  // say which lost one, and that is what a gate can hold to exactly — a
  // candidate that stops working somewhere new and a candidate that starts
  // working where it must not are both regressions, and a count catches
  // neither on its own.
  std::vector<std::string> unusable[3];

  for (int i = 1; i < argc; ++i) {
    const std::string path = argv[i];
    const std::size_t slash = path.find_last_of("/\\");
    const std::string name =
        slash == std::string::npos ? path : path.substr(slash + 1);

    out << name << "\n";

    std::string complaint;
    const std::vector<Probe> first = probe(path, complaint);
    if (first.empty()) {
      out << "    " << (complaint.empty() ? "no definitions" : complaint) << "\n\n";
      continue;
    }

    ++with_definitions;
    out << "    definitions " << first.size() << "\n";
    char line[512];
    for (const Probe &entry : first) {
      std::snprintf(line, sizeof(line), "    %-32s", entry.name.c_str());
      out << line << "\n";
      std::snprintf(line, sizeof(line), "        shape entity       %-10s %s",
                    entry.shape_entity.text().c_str(),
                    entry.shape_entity.type.empty() ? "-"
                                                    : entry.shape_entity.type.c_str());
      out << line << "\n";
      std::snprintf(line, sizeof(line), "        product definition %-10s %s",
                    entry.product_definition.text().c_str(),
                    entry.product_definition.type.empty()
                        ? "-"
                        : entry.product_definition.type.c_str());
      out << line << "\n";
      std::snprintf(line, sizeof(line), "        product            %-10s id %s",
                    entry.product.text().c_str(),
                    entry.product_id.empty() ? "-" : entry.product_id.c_str());
      out << line << "\n";
    }

    // The same bytes, a second reader, a second document. Two readings that
    // disagree would end the candidate outright, and are the reason this runs
    // twice rather than trusting that a read is a function of its input.
    std::string second_complaint;
    const std::vector<Probe> second = probe(path, second_complaint);

    for (int candidate = 0; candidate < 3; ++candidate) {
      std::vector<std::string> keys;
      for (const Probe &entry : first) {
        std::string key = key_of(entry, candidate);
        if (!key.empty()) {
          keys.push_back(key);
        }
      }
      std::vector<std::string> again;
      for (const Probe &entry : second) {
        std::string key = key_of(entry, candidate);
        if (!key.empty()) {
          again.push_back(key);
        }
      }

      std::vector<std::string> unique = keys;
      std::sort(unique.begin(), unique.end());
      unique.erase(std::unique(unique.begin(), unique.end()), unique.end());

      const bool complete = keys.size() == first.size();
      const bool distinct = unique.size() == keys.size();
      const bool stable = keys == again;

      std::snprintf(line, sizeof(line),
                    "    %s  present %zu/%zu  unique %-3s  same on a second read %s",
                    candidate_name(candidate), keys.size(), first.size(),
                    distinct && !keys.empty() ? "yes" : "no",
                    stable && !keys.empty() ? "yes" : "no");
      out << line << "\n";
      // Stated rather than left to be worked out from three columns: this is
      // the sentence the decision will be made on.
      const bool ok = complete && distinct && stable && !keys.empty();
      out << "        usable as a key: " << (ok ? "yes" : "no") << "\n";
      if (ok) {
        ++usable[candidate];
      } else {
        unusable[candidate].push_back(name);
      }
    }
    out << "\n";
  }

  // A file that was refused has no definitions and so nothing to key; it is
  // not counted against a candidate. Only files that produced a scene are.
  out << "# summary\n";
  char summary[256];
  std::snprintf(summary, sizeof(summary), "    files examined %d", argc - 1);
  out << summary << "\n";
  std::snprintf(summary, sizeof(summary), "    files with definitions %d",
                with_definitions);
  out << summary << "\n";
  for (int candidate = 0; candidate < 3; ++candidate) {
    std::snprintf(summary, sizeof(summary), "    %s usable on %d/%d files",
                  candidate_name(candidate), usable[candidate], with_definitions);
    out << summary << "\n";

    std::vector<std::string> named = unusable[candidate];
    std::sort(named.begin(), named.end());
    out << "        unusable on:";
    if (named.empty()) {
      out << " nothing";
    }
    for (const std::string &file : named) {
      out << " " << file;
    }
    out << "\n";
  }

  std::cout << out.str();
  return 0;
}
