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
// that holds on twelve files and three platforms is evidence, and the decision
// belongs in FerriteCAD where it can be refused when the evidence runs out.

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

/// The entity Open CASCADE says produced a shape.
///
/// Four modes and two locations are tried because the shape XDE stores is not
/// obliged to be the shape the transfer recorded — it may be the same geometry
/// under a different location. Each attempt is cheap; a definition with no
/// entity at all is a measurement worth having, but only after asking properly.
std::vector<Handle(Standard_Transient)> entities_from(
    const Handle(XSControl_TransferReader) & reader, const TopoDS_Shape &shape) {
  std::vector<Handle(Standard_Transient)> found;
  if (reader.IsNull() || shape.IsNull()) {
    return found;
  }
  const TopoDS_Shape unplaced = shape.Located(TopLoc_Location());
  for (const TopoDS_Shape &candidate : {shape, unplaced}) {
    for (int mode = 0; mode <= 3; ++mode) {
      Handle(Standard_Transient) entity =
          reader->EntityFromShapeResult(candidate, mode);
      if (!entity.IsNull() &&
          !std::any_of(found.begin(), found.end(), [&](const auto &known) {
            return known == entity;
          })) {
        found.push_back(entity);
      }
    }
  }
  return found;
}

/// Walks outward from an entity to the PRODUCT_DEFINITION that owns it.
///
/// Typed, not a graph crawl. The first attempt at this followed sharings in
/// every direction and found nothing, because the chain does not run one way:
///
///     MANIFOLD_SOLID_BREP
///       <- shared by  ADVANCED_BREP_SHAPE_REPRESENTATION
///       <- shared by  SHAPE_DEFINITION_REPRESENTATION
///       -> refers to  PRODUCT_DEFINITION_SHAPE
///       -> refers to  PRODUCT_DEFINITION
///
/// Every transition is checked by its concrete STEP type and by the field that
/// points back to the entity just visited. Merely limiting a generic Sharings
/// crawl to two or three hops would not make it typed: it could still enter an
/// unrelated property representation and return a neighbouring part.
Handle(StepBasic_ProductDefinition) product_definition_of(
    const Interface_Graph &graph, const Handle(Standard_Transient) & start) {
  if (start.IsNull()) {
    return Handle(StepBasic_ProductDefinition)();
  }

  std::vector<Handle(StepShape_AdvancedBrepShapeRepresentation)> representations;
  const auto remember_representation = [&](const Handle(Standard_Transient) &entity) {
    Handle(StepShape_AdvancedBrepShapeRepresentation) representation =
        Handle(StepShape_AdvancedBrepShapeRepresentation)::DownCast(entity);
    if (!representation.IsNull() &&
        !std::any_of(representations.begin(), representations.end(),
                     [&](const auto &known) { return known == representation; })) {
      representations.push_back(representation);
    }
  };

  // EntityFromShapeResult may already return the representation, or one of
  // the representation's items. Those are the only two typed starting cases.
  remember_representation(start);
  Interface_EntityIterator representation_sharings = graph.Sharings(start);
  for (representation_sharings.Start(); representation_sharings.More();
       representation_sharings.Next()) {
    remember_representation(representation_sharings.Value());
  }

  std::vector<Handle(StepBasic_ProductDefinition)> products;
  for (const auto &representation : representations) {
    Interface_EntityIterator definition_sharings = graph.Sharings(representation);
    for (definition_sharings.Start(); definition_sharings.More();
         definition_sharings.Next()) {
      Handle(StepShape_ShapeDefinitionRepresentation) shape_definition =
          Handle(StepShape_ShapeDefinitionRepresentation)::DownCast(
              definition_sharings.Value());
      if (shape_definition.IsNull() ||
          shape_definition->UsedRepresentation() != representation) {
        continue;
      }

      Handle(StepRepr_ProductDefinitionShape) property =
          Handle(StepRepr_ProductDefinitionShape)::DownCast(
              shape_definition->Definition().PropertyDefinition());
      if (property.IsNull()) {
        continue;
      }
      Handle(StepBasic_ProductDefinition) product =
          property->Definition().ProductDefinition();
      if (!product.IsNull() &&
          !std::any_of(products.begin(), products.end(),
                       [&](const auto &known) { return known == product; })) {
        products.push_back(product);
      }
    }
  }

  // Several typed routes to the same product are harmless. Several products
  // are ambiguity, and ambiguity is absence rather than "the first one".
  return products.size() == 1 ? products.front()
                              : Handle(StepBasic_ProductDefinition)();
}

/// The assemblies a part is a component of, according to the file.
///
/// An assembly has no geometry of its own, so the route above has nothing to
/// start from. What it does have is components, and the file says so in
/// NEXT_ASSEMBLY_USAGE_OCCURRENCE: the relating product definition is the
/// assembly, the related one is the part. This reads that relation backwards,
/// from a known child to its possible parents.
std::vector<Handle(StepBasic_ProductDefinition)> assemblies_containing(
    const Interface_Graph &graph,
    const Handle(StepBasic_ProductDefinition) & child) {
  std::vector<Handle(StepBasic_ProductDefinition)> parents;
  if (child.IsNull()) {
    return parents;
  }
  Interface_EntityIterator sharings = graph.Sharings(child);
  for (sharings.Start(); sharings.More(); sharings.Next()) {
    Handle(StepRepr_NextAssemblyUsageOccurrence) usage =
        Handle(StepRepr_NextAssemblyUsageOccurrence)::DownCast(sharings.Value());
    if (usage.IsNull()) {
      continue;
    }
    // Only the occurrences where this really is the component. The same
    // relation type is used the other way round elsewhere in the schema.
    if (usage->RelatedProductDefinition() != child) {
      continue;
    }
    Handle(StepBasic_ProductDefinition) parent = usage->RelatingProductDefinition();
    // One assembly containing the same part twice writes two occurrences, and
    // that is ordinary: it is how four bolts in one plate are said. What is
    // being collected here is which assemblies, not how many times, so the
    // same assembly named twice is named once.
    if (!parent.IsNull() &&
        !std::any_of(parents.begin(), parents.end(),
                     [&](const Handle(StepBasic_ProductDefinition) &known) {
                       return known == parent;
                     })) {
      parents.push_back(parent);
    }
  }
  return parents;
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

  // What the geometry itself can say. A part reaches its product definition
  // through the representation holding its solid; an assembly has no solid and
  // is left empty here.
  std::vector<Handle(StepBasic_ProductDefinition)> product_definitions(
      definitions.size());
  std::vector<Ident> shape_entities(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i) {
    const std::vector<Handle(Standard_Transient)> entities =
        entities_from(transfer, shapes->GetShape(definitions[i]));
    if (!entities.empty()) {
      shape_entities[i] = ident_of(model, entities.front());
    }

    std::vector<Handle(StepBasic_ProductDefinition)> products;
    for (const auto &entity : entities) {
      Handle(StepBasic_ProductDefinition) product =
          product_definition_of(graph, entity);
      if (!product.IsNull() &&
          !std::any_of(products.begin(), products.end(),
                       [&](const auto &known) { return known == product; })) {
        products.push_back(product);
      }
    }
    if (products.size() == 1) {
      product_definitions[i] = products.front();
    }
  }

  // And what the assembly structure can say about the rest. A definition with
  // no geometry is named by the occurrences that put its components inside it:
  // the parent every one of them agrees on, if they agree on exactly one.
  // Repeated to a fixed point so an assembly of assemblies resolves from the
  // parts upward rather than depending on the order they were walked in.
  for (bool changed = true; changed;) {
    changed = false;
    for (std::size_t i = 0; i < definitions.size(); ++i) {
      if (!product_definitions[i].IsNull() || children_of[i].empty()) {
        continue;
      }

      std::vector<Handle(StepBasic_ProductDefinition)> agreed;
      bool first = true;
      bool usable = true;
      for (const std::size_t child : children_of[i]) {
        if (product_definitions[child].IsNull()) {
          usable = false;
          break;
        }
        std::vector<Handle(StepBasic_ProductDefinition)> parents =
            assemblies_containing(graph, product_definitions[child]);
        if (first) {
          agreed = parents;
          first = false;
          continue;
        }
        // Only the parents every component names. A component used in two
        // assemblies offers both, and the intersection is what narrows it.
        std::vector<Handle(StepBasic_ProductDefinition)> both;
        for (const Handle(StepBasic_ProductDefinition) &candidate : agreed) {
          if (std::any_of(parents.begin(), parents.end(),
                          [&](const Handle(StepBasic_ProductDefinition) &other) {
                            return other == candidate;
                          })) {
            both.push_back(candidate);
          }
        }
        agreed.swap(both);
      }

      // Exactly one, or none. Two assemblies containing the same components
      // are indistinguishable from here, and guessing between them is the
      // failure this whole exercise exists to avoid.
      if (usable && agreed.size() == 1) {
        product_definitions[i] = agreed.front();
        changed = true;
      }
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
  }

  std::cout << out.str();
  return 0;
}
