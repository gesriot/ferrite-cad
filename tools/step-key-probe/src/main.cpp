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
  bool assembly = false;
  bool root = false;
  ferritecad::StepIdentityRoute route = ferritecad::StepIdentityRoute::none;
  Ident shape_entity;
  Ident product_definition;
  Ident product;
  std::string product_id;
  std::string durable_key;
  std::vector<int> child_product_definitions;
};

struct ProbeRun {
  std::vector<Probe> definitions;
  ferritecad::StepIdentityMetrics metrics;
  std::size_t scene_nodes = 0;
  std::size_t roots = 0;
  std::size_t equal_child_assembly_pairs = 0;
  bool reversed_traversal_stable = false;
  bool foreign_entity_rejected = false;
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
ProbeRun probe(const std::string &path, std::string &complaint) {
  ProbeRun run;

  Handle(TDocStd_Application) app = new TDocStd_Application();
  Handle(TDocStd_Document) doc;
  app->NewDocument("BinXCAF", doc);

  STEPCAFControl_Reader reader;
  reader.SetNameMode(Standard_True);
  reader.SetColorMode(Standard_True);

  try {
    if (reader.ReadFile(path.c_str()) != IFSelect_RetDone) {
      complaint = "the file was not read";
      return run;
    }
    if (reader.Transfer(doc) != Standard_True) {
      complaint = "nothing was transferred";
      return run;
    }
  } catch (const Standard_Failure &failure) {
    complaint = std::string("threw: ") + failure.GetMessageString();
    return run;
  }

  Handle(StepData_StepModel) model =
      Handle(StepData_StepModel)::DownCast(reader.Reader().Model());
  Handle(XSControl_WorkSession) session = reader.Reader().WS();
  Handle(XSControl_TransferReader) transfer =
      session.IsNull() ? Handle(XSControl_TransferReader)() : session->TransferReader();
  if (model.IsNull() || session.IsNull()) {
    complaint = "the reader kept no model to ask";
    return run;
  }
  Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());

  // Every distinct definition, reached the same way the importer reaches them:
  // through references, never by assuming the free shapes are all there is.
  std::vector<TDF_Label> definitions;
  std::vector<bool> assemblies;
  // Which definitions sit inside which, only for reporting the deliberately
  // equal child multisets. The identity resolver does not consume this list:
  // assembly ownership comes from the XDE ProductDefinition transfer
  // association below.
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
    assemblies.push_back(shapes->IsAssembly(definition) == Standard_True);
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
  std::vector<std::size_t> root_definitions;
  for (int r = 1; r <= roots.Length(); ++r) {
    root_definitions.push_back(walk(roots.Value(r)));
  }
  run.roots = static_cast<std::size_t>(roots.Length());

  // Count the placed tree without definition deduplication. A shared
  // definition can contain children of its own, and every placement of that
  // definition places that whole subtree again.
  std::function<void(const TDF_Label &)> count_occurrences =
      [&](const TDF_Label &label) {
        ++run.scene_nodes;
        TDF_Label definition = label;
        if (shapes->IsReference(label)) {
          shapes->GetReferredShape(label, definition);
        }
        TDF_LabelSequence children;
        shapes->GetComponents(definition, children);
        for (int c = 1; c <= children.Length(); ++c) {
          count_occurrences(children.Value(c));
        }
      };
  for (int r = 1; r <= roots.Length(); ++r) {
    count_occurrences(roots.Value(r));
  }

  // The bridge's own resolution, not a second copy of it: parts through the
  // representation holding their solid, assemblies through the exact XDE
  // ProductDefinition transfer result associated with their definition label.
  std::vector<TopoDS_Shape> definition_shapes;
  definition_shapes.reserve(definitions.size());
  for (const TDF_Label &definition : definitions) {
    definition_shapes.push_back(shapes->GetShape(definition));
  }
  const ferritecad::StepIdentityIndex identity_index(
      model, transfer, reader.GetShapeLabelMap());
  const std::vector<ferritecad::StepDefinitionIdentity> identities =
      ferritecad::resolve_definition_identities(
          identity_index, definition_shapes, definitions, assemblies);
  const std::vector<std::string> durable_keys = ferritecad::definition_keys(
      identity_index, definition_shapes, definitions, assemblies);

  std::vector<TopoDS_Shape> reversed_shapes(definition_shapes.rbegin(),
                                             definition_shapes.rend());
  std::vector<TDF_Label> reversed_labels(definitions.rbegin(), definitions.rend());
  std::vector<bool> reversed_assemblies(assemblies.rbegin(), assemblies.rend());
  const std::vector<ferritecad::StepDefinitionIdentity> reversed =
      ferritecad::resolve_definition_identities(
          identity_index, reversed_shapes, reversed_labels, reversed_assemblies);
  run.reversed_traversal_stable = reversed.size() == identities.size();
  for (std::size_t i = 0; run.reversed_traversal_stable && i < identities.size(); ++i) {
    run.reversed_traversal_stable =
        identities[i].product == reversed[identities.size() - 1 - i].product;
  }
  run.metrics = identity_index.metrics();

  Handle(StepBasic_ProductDefinition) foreign =
      new StepBasic_ProductDefinition();
  Handle(StepData_StepModel) foreign_model = new StepData_StepModel();
  foreign_model->AddEntity(foreign);
  foreign_model->SetIdentLabel(foreign, 1);
  run.foreign_entity_rejected = identity_index.source_ident(foreign) == 0 &&
                                foreign_model->Contains(foreign) &&
                                foreign_model->IdentLabel(foreign) == 1;
  std::vector<Handle(StepBasic_ProductDefinition)> product_definitions(
      identities.size());
  for (std::size_t i = 0; i < identities.size(); ++i) {
    product_definitions[i] = identities[i].product;
  }

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
    entry.assembly = assemblies[i];
    entry.root = std::find(root_definitions.begin(), root_definitions.end(), i) !=
                 root_definitions.end();
    entry.route = identities[i].route;
    entry.durable_key = durable_keys[i];
    entry.shape_entity = shape_entities[i];
    entry.product_definition = ident_of(model, product_definitions[i]);
    if (identity_index.source_ident(product_definitions[i]) == 0) {
      entry.product_definition.id = 0;
      entry.product_definition.source = false;
    }

    for (const std::size_t child : children_of[i]) {
      if (child < product_definitions.size()) {
        entry.child_product_definitions.push_back(
            identity_index.source_ident(product_definitions[child]));
      }
    }
    std::sort(entry.child_product_definitions.begin(),
              entry.child_product_definitions.end());

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
    run.definitions.push_back(entry);
  }

  // Sorted so two reads, and two platforms, list the same definitions in the
  // same order however the document happened to be walked.
  std::sort(run.definitions.begin(), run.definitions.end(), [](const Probe &a, const Probe &b) {
    if (a.name != b.name) {
      return a.name < b.name;
    }
    if (a.product_definition.id != b.product_definition.id) {
      return a.product_definition.id < b.product_definition.id;
    }
    return a.shape_entity.id < b.shape_entity.id;
  });

  for (std::size_t i = 0; i < run.definitions.size(); ++i) {
    if (!run.definitions[i].assembly) {
      continue;
    }
    for (std::size_t j = i + 1; j < run.definitions.size(); ++j) {
      if (run.definitions[j].assembly &&
          run.definitions[i].child_product_definitions ==
              run.definitions[j].child_product_definitions &&
          run.definitions[i].product_definition.id !=
              run.definitions[j].product_definition.id) {
        ++run.equal_child_assembly_pairs;
      }
    }
  }
  return run;
}

/// The key a candidate would supply, as the text a comparison would use.
std::string key_of(const Probe &entry, int candidate) {
  switch (candidate) {
    case 0:
      return entry.shape_entity.source ? entry.shape_entity.text() : std::string();
    case 1:
      return entry.durable_key;
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

const char *route_name(ferritecad::StepIdentityRoute route) {
  switch (route) {
    case ferritecad::StepIdentityRoute::part_representation:
      return "part representation";
    case ferritecad::StepIdentityRoute::assembly_xde:
      return "assembly XDE product transfer";
    case ferritecad::StepIdentityRoute::ambiguous:
      return "ambiguous";
    case ferritecad::StepIdentityRoute::none:
      return "none";
  }
  return "none";
}

}  // namespace

int main(int argc, char **argv) {
  if (argc < 2) {
    std::fprintf(stderr, "usage: step_key_probe <file.step>...\n");
    return 2;
  }

  Handle(StepBasic_ProductDefinition) first_product =
      new StepBasic_ProductDefinition();
  Handle(StepBasic_ProductDefinition) second_product =
      new StepBasic_ProductDefinition();
  ferritecad::ProductCandidates one_product;
  ferritecad::remember_product(one_product, first_product);
  ferritecad::remember_product(one_product, first_product);
  ferritecad::ProductCandidates two_products = one_product;
  ferritecad::remember_product(two_products, second_product);
  if (ferritecad::unambiguous_product(one_product) != first_product ||
      !ferritecad::unambiguous_product(two_products).IsNull()) {
    std::cerr << "typed ambiguity was accepted\n";
    return 1;
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
      << "# This reports. What FerriteCAD does about it is decided elsewhere.\n\n"
      << "# typed ambiguity rejected yes\n\n";

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
    const ProbeRun first_run = probe(path, complaint);
    const std::vector<Probe> &first = first_run.definitions;
    if (first.empty()) {
      out << "    " << (complaint.empty() ? "no definitions" : complaint) << "\n\n";
      continue;
    }

    ++with_definitions;
    out << "    definitions " << first.size() << "\n";
    out << "    roots " << first_run.roots << "\n";
    out << "    placed occurrences "
        << first_run.scene_nodes - first_run.roots << "\n";
    out << "    assemblies with equal children but distinct products "
        << first_run.equal_child_assembly_pairs << " pair(s)\n";
    out << "    same after reversed traversal "
        << (first_run.reversed_traversal_stable ? "yes" : "no") << "\n";
    out << "    foreign source entity rejected "
        << (first_run.foreign_entity_rejected ? "yes" : "no") << "\n";
    out << "    identity index model scans " << first_run.metrics.model_scans
        << "  entities " << first_run.metrics.entities_scanned
        << "  non-transforming relationships "
        << first_run.metrics.nontransforming_relationships
        << "  transforming relationships ignored "
        << first_run.metrics.transforming_relationships_ignored
        << "  XDE product associations "
        << first_run.metrics.xde_product_associations
        << "  ambiguous source identities "
        << first_run.metrics.ambiguous_source_identities << "\n";
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
      out << "        definition          "
          << (entry.assembly ? "assembly" : "part")
          << (entry.root ? " root" : "") << " via " << route_name(entry.route)
          << "\n";
      out << "        durable key         "
          << (entry.durable_key.empty() ? "-" : entry.durable_key) << "\n";
      if (entry.assembly) {
        out << "        child products      ";
        if (entry.child_product_definitions.empty()) {
          out << "-";
        }
        for (const int child : entry.child_product_definitions) {
          out << " #" << child;
        }
        out << "\n";
      }
    }

    // The same bytes, a second reader, a second document. Two readings that
    // disagree would end the candidate outright, and are the reason this runs
    // twice rather than trusting that a read is a function of its input.
    std::string second_complaint;
    const ProbeRun second_run = probe(path, second_complaint);
    const std::vector<Probe> &second = second_run.definitions;

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
