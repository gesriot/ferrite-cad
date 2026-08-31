// SPDX-License-Identifier: MIT
//
// What identifies a definition in a STEP file, as opposed to in this reading
// of it.
//
// Shared by the bridge and by `tools/step-key-probe` on purpose. The probe is
// what the pin workflow measures on three platforms; the bridge is what the
// product runs. Two copies of this would make the measurement a statement
// about the probe, and the first time they drifted the gate would go on
// passing while the importer did something else.
//
// Four candidates were ruled out before any of this was written: a `TDF_Label`
// entry is a position in a document Open CASCADE built this run, a name is
// neither unique nor always present, a position in the definition list is what
// binding already refuses to trust, and geometry is a guess wearing a number.
// What is left is the STEP entity the file itself wrote down.
//
// # The key is local to its source
//
// `#31` identifies something within one file and nothing at all between two.
// Everything here produces source-local keys, and a durable reference has to
// carry the identity of the source alongside one.

#ifndef FERRITECAD_STEP_IDENTITY_HPP
#define FERRITECAD_STEP_IDENTITY_HPP

#include <Interface_EntityIterator.hxx>
#include <StepBasic_ProductDefinition.hxx>
#include <StepData_StepModel.hxx>
#include <StepRepr_ProductDefinitionShape.hxx>
#include <StepRepr_RepresentationRelationshipWithTransformation.hxx>
#include <StepRepr_ShapeRepresentationRelationship.hxx>
#include <StepShape_AdvancedBrepShapeRepresentation.hxx>
#include <StepShape_ShapeDefinitionRepresentation.hxx>
#include <TDF_Label.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS_Shape.hxx>
#include <TransferBRep.hxx>
#include <Transfer_TransientProcess.hxx>
#include <XCAFDoc_DataMapOfShapeLabel.hxx>
#include <XSControl_TransferReader.hxx>

#include <algorithm>
#include <cstddef>
#include <string>
#include <unordered_map>
#include <vector>

namespace ferritecad {

/// The entities Open CASCADE says produced a shape.
///
/// Four modes and two locations are tried because the shape XDE stores is not
/// obliged to be the shape the transfer recorded — it may be the same geometry
/// under a different location. Each attempt is cheap, and a definition with no
/// entity at all is an answer worth having, but only after asking properly.
inline std::vector<Handle(Standard_Transient)> entities_from(
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
          !std::any_of(found.begin(), found.end(),
                       [&](const auto &known) { return known == entity; })) {
        found.push_back(entity);
      }
    }
  }
  return found;
}

/// Evidence that the document-wide identity association was built once.
///
/// `model_scans` is deliberately observable by the probe. A correct answer
/// reached by scanning the Interface model once per definition would become a
/// denial of service on an ordinary assembly while still returning the right
/// keys, so cardinality alone cannot guard this contract.
struct StepIdentityMetrics {
  std::size_t model_scans = 0;
  std::size_t entities_scanned = 0;
  std::size_t nontransforming_relationships = 0;
  std::size_t transforming_relationships_ignored = 0;
  std::size_t xde_product_associations = 0;
  std::size_t ambiguous_source_identities = 0;
};

/// All PRODUCT_DEFINITION candidates supplied by typed source relationships.
///
/// Several routes to the same entity collapse to one candidate. More than one
/// distinct entity remains visible as ambiguity and is never resolved by
/// taking the first one.
using ProductCandidates = std::vector<Handle(StepBasic_ProductDefinition)>;

inline void remember_product(ProductCandidates &products,
                             const Handle(StepBasic_ProductDefinition) & product) {
  if (!product.IsNull() &&
      !std::any_of(products.begin(), products.end(),
                   [&](const auto &known) { return known == product; })) {
    products.push_back(product);
  }
}

/// One candidate is an answer; zero is absence and two is ambiguity.
inline Handle(StepBasic_ProductDefinition) unambiguous_product(
    const ProductCandidates &products) {
  return products.size() == 1 ? products.front()
                              : Handle(StepBasic_ProductDefinition)();
}

/// A document-wide association from STEP/XDE source relationships to product
/// definitions.
///
/// The model is scanned exactly once. Part ownership follows only these typed
/// routes:
///
///     MANIFOLD_SOLID_BREP
///       <- item of  ADVANCED_BREP_SHAPE_REPRESENTATION
///       <- optional non-transforming SHAPE_REPRESENTATION_RELATIONSHIP
///       <- used by  SHAPE_DEFINITION_REPRESENTATION
///       -> refers to PRODUCT_DEFINITION_SHAPE
///       -> refers to PRODUCT_DEFINITION
///
/// A RepresentationRelationshipWithTransformation is an occurrence placement,
/// not part ownership, and is counted but never crossed. Assemblies have no
/// geometry item to start from. For them the association is the one the XDE
/// reader itself records: a typed ProductDefinition transfer result is present
/// as an exact key in the reader's shape-to-label map. No shape comparison,
/// coordinate comparison, placement, name, child set or traversal position is
/// used. TopoDS_Shape and TDF_Label exist only while this index is built and
/// queried; the only value emitted as durable identity is the source entity's
/// IdentLabel.
class StepIdentityIndex {
 public:
  StepIdentityIndex(const Handle(StepData_StepModel) & model,
                    const Handle(XSControl_TransferReader) & transfer,
                    const XCAFDoc_DataMapOfShapeLabel &shape_labels)
      : model_(model), transfer_(transfer) {
    if (model.IsNull()) {
      return;
    }

    ++metrics_.model_scans;
    std::vector<Handle(StepShape_AdvancedBrepShapeRepresentation)> breps;
    std::vector<Handle(StepShape_ShapeDefinitionRepresentation)> shape_definitions;
    std::vector<Handle(StepRepr_ShapeRepresentationRelationship)>
        representation_relationships;
    ProductCandidates product_definitions;

    const int count = model->NbEntities();
    for (int i = 1; i <= count; ++i) {
      Handle(Standard_Transient) entity = model->Value(i);
      ++metrics_.entities_scanned;
      const int source_identity = model->IdentLabel(entity);
      if (source_identity > 0) {
        ++source_identity_counts_[source_identity];
      }

      Handle(StepShape_AdvancedBrepShapeRepresentation) brep =
          Handle(StepShape_AdvancedBrepShapeRepresentation)::DownCast(entity);
      if (!brep.IsNull()) {
        breps.push_back(brep);
      }

      Handle(StepShape_ShapeDefinitionRepresentation) shape_definition =
          Handle(StepShape_ShapeDefinitionRepresentation)::DownCast(entity);
      if (!shape_definition.IsNull()) {
        shape_definitions.push_back(shape_definition);
      }

      Handle(StepRepr_ShapeRepresentationRelationship) relationship =
          Handle(StepRepr_ShapeRepresentationRelationship)::DownCast(entity);
      if (!relationship.IsNull()) {
        Handle(StepRepr_RepresentationRelationshipWithTransformation) transformed =
            Handle(StepRepr_RepresentationRelationshipWithTransformation)::DownCast(
                entity);
        if (transformed.IsNull()) {
          representation_relationships.push_back(relationship);
          ++metrics_.nontransforming_relationships;
        } else {
          ++metrics_.transforming_relationships_ignored;
        }
      }

      remember_product(
          product_definitions,
          Handle(StepBasic_ProductDefinition)::DownCast(entity));
    }
    for (const auto &entry : source_identity_counts_) {
      if (entry.second > 1) {
        ++metrics_.ambiguous_source_identities;
      }
    }

    // First record only direct SDR ownership. Relationship propagation below
    // consults this immutable map, so even a chain of generic relationships
    // cannot turn into an unbounded ownership crawl.
    RepresentationProducts direct_products;
    for (const auto &shape_definition : shape_definitions) {
      Handle(StepRepr_ProductDefinitionShape) property =
          Handle(StepRepr_ProductDefinitionShape)::DownCast(
              shape_definition->Definition().PropertyDefinition());
      if (property.IsNull()) {
        continue;
      }
      Handle(StepBasic_ProductDefinition) product =
          property->Definition().ProductDefinition();
      Handle(Standard_Transient) representation =
          shape_definition->UsedRepresentation();
      if (!representation.IsNull()) {
        remember_product(direct_products[representation.get()], product);
      }
    }
    representation_products_ = direct_products;

    // Exactly one non-transforming shape-representation relationship is an
    // ownership bridge. Both directions are considered because Rep1/Rep2 order
    // does not change what the typed relationship asserts. Transforming
    // relationships never entered this collection.
    for (const auto &relationship : representation_relationships) {
      Handle(Standard_Transient) first = relationship->Rep1();
      Handle(Standard_Transient) second = relationship->Rep2();
      if (first.IsNull() || second.IsNull()) {
        continue;
      }
      const auto first_products = direct_products.find(first.get());
      if (first_products != direct_products.end()) {
        for (const auto &product : first_products->second) {
          remember_product(representation_products_[second.get()], product);
        }
      }
      const auto second_products = direct_products.find(second.get());
      if (second_products != direct_products.end()) {
        for (const auto &product : second_products->second) {
          remember_product(representation_products_[first.get()], product);
        }
      }
    }

    // Index every B-Rep item once. EntityFromShapeResult may return either the
    // representation or one of these items; no Interface_Graph query is needed
    // when a definition is resolved later.
    for (const auto &brep : breps) {
      for (int item = 1; item <= brep->NbItems(); ++item) {
        Handle(Standard_Transient) value = brep->ItemsValue(item);
        if (!value.IsNull()) {
          auto &representations = item_representations_[value.get()];
          if (!std::any_of(representations.begin(), representations.end(),
                           [&](const auto &known) { return known == brep; })) {
            representations.push_back(brep);
          }
        }
      }
    }

    // This is the exact association used by STEPCAFControl_Reader while it
    // builds XDE. It is restricted to ProductDefinition starting entities and
    // exact entries already present in that reader's own map. There is no
    // Search fallback and no geometric or placement comparison.
    Handle(Transfer_TransientProcess) process =
        transfer.IsNull() ? Handle(Transfer_TransientProcess)()
                          : transfer->TransientProcess();
    if (!process.IsNull()) {
      for (const auto &product : product_definitions) {
        const TopoDS_Shape result = TransferBRep::ShapeResult(process, product);
        TDF_Label label;
        if (!result.IsNull() && shape_labels.Find(result, label) && !label.IsNull()) {
          ProductCandidates &associated = label_products(label);
          const std::size_t before = associated.size();
          remember_product(associated, product);
          if (associated.size() != before) {
            ++metrics_.xde_product_associations;
          }
        }
      }
    }
  }

  const StepIdentityMetrics &metrics() const { return metrics_; }

  /// The source-local entity identifier, only when the source wrote it once.
  ///
  /// A typed XDE association can still point at an entity whose textual STEP
  /// identifier occurred more than once. That association identifies the
  /// transient entity OCCT retained, but it cannot repair the ambiguity in the
  /// source bytes, so no durable key is emitted for it.
  int source_ident(const Handle(Standard_Transient) & entity) const {
    if (model_.IsNull() || entity.IsNull() || !model_->Contains(entity)) {
      return 0;
    }
    const int from_file = model_->IdentLabel(entity);
    const auto count = source_identity_counts_.find(from_file);
    if (from_file <= 0 || count == source_identity_counts_.end() ||
        count->second != 1) {
      return 0;
    }
    return from_file;
  }

  /// Candidates reached from a geometry-producing STEP entity.
  ProductCandidates products_from(const Handle(Standard_Transient) & start) const {
    ProductCandidates products;
    if (start.IsNull()) {
      return products;
    }

    const auto direct = representation_products_.find(start.get());
    if (direct != representation_products_.end()) {
      for (const auto &product : direct->second) {
        remember_product(products, product);
      }
    }

    const auto item = item_representations_.find(start.get());
    if (item != item_representations_.end()) {
      for (const auto &representation : item->second) {
        const auto represented = representation_products_.find(representation.get());
        if (represented == representation_products_.end()) {
          continue;
        }
        for (const auto &product : represented->second) {
          remember_product(products, product);
        }
      }
    }
    return products;
  }

  /// Candidates associated by the XDE reader with one definition label.
  ProductCandidates products_from(const TDF_Label &label) const {
    for (const LabelProducts &entry : label_products_) {
      if (entry.label.IsEqual(label)) {
        return entry.products;
      }
    }
    return ProductCandidates();
  }

  const Handle(XSControl_TransferReader) &transfer() const { return transfer_; }

 private:
  using RepresentationProducts =
      std::unordered_map<const Standard_Transient *, ProductCandidates>;
  using ItemRepresentations = std::unordered_map<
      const Standard_Transient *,
      std::vector<Handle(StepShape_AdvancedBrepShapeRepresentation)>>;

  struct LabelProducts {
    TDF_Label label;
    ProductCandidates products;
  };

  ProductCandidates &label_products(const TDF_Label &label) {
    for (LabelProducts &entry : label_products_) {
      if (entry.label.IsEqual(label)) {
        return entry.products;
      }
    }
    label_products_.push_back(LabelProducts{label, ProductCandidates()});
    return label_products_.back().products;
  }

  Handle(StepData_StepModel) model_;
  Handle(XSControl_TransferReader) transfer_;
  std::unordered_map<int, std::size_t> source_identity_counts_;
  RepresentationProducts representation_products_;
  ItemRepresentations item_representations_;
  std::vector<LabelProducts> label_products_;
  StepIdentityMetrics metrics_;
};

/// The text form of a definition key, or empty when there is no key.
///
/// Prefixed by what kind of identity it is, so a second kind can be added for
/// a format without product definitions without either being mistaken for the
/// other. Local to one file: see the note at the top of this header.
inline std::string definition_key(int ident) {
  if (ident <= 0) {
    return std::string();
  }
  return "step.product_definition#" + std::to_string(ident);
}

enum class StepIdentityRoute {
  none,
  part_representation,
  assembly_xde,
  ambiguous,
};

struct StepDefinitionIdentity {
  Handle(StepBasic_ProductDefinition) product;
  StepIdentityRoute route = StepIdentityRoute::none;
};

/// Resolves every XDE definition through the already-built association.
///
/// A simple definition is required to have the typed representation ownership
/// route. The XDE ProductDefinition association is a corroborating route for a
/// part, not a fallback that could hide a missing representation relationship.
/// An assembly is required to have the XDE ProductDefinition association,
/// because its compound has no geometry-owning representation item. If two
/// routes name different source entities, the result is ambiguity.
inline std::vector<StepDefinitionIdentity> resolve_definition_identities(
    const StepIdentityIndex &index, const std::vector<TopoDS_Shape> &shapes,
    const std::vector<TDF_Label> &labels,
    const std::vector<bool> &assemblies) {
  const std::size_t count = shapes.size();
  std::vector<StepDefinitionIdentity> resolved(count);
  for (std::size_t i = 0; i < count; ++i) {
    const ProductCandidates xde =
        i < labels.size() ? index.products_from(labels[i]) : ProductCandidates();
    ProductCandidates candidates;

    if (i < assemblies.size() && assemblies[i]) {
      candidates = xde;
      resolved[i].product = unambiguous_product(candidates);
      if (!resolved[i].product.IsNull()) {
        resolved[i].route = StepIdentityRoute::assembly_xde;
      } else if (candidates.size() > 1) {
        resolved[i].route = StepIdentityRoute::ambiguous;
      }
      continue;
    }

    for (const Handle(Standard_Transient) &entity :
         entities_from(index.transfer(), shapes[i])) {
      for (const auto &product : index.products_from(entity)) {
        remember_product(candidates, product);
      }
    }

    // No ownership relationship means no part key. The XDE transfer
    // association must not silently replace the missing typed source route.
    if (candidates.empty()) {
      continue;
    }
    for (const auto &product : xde) {
      remember_product(candidates, product);
    }
    resolved[i].product = unambiguous_product(candidates);
    if (!resolved[i].product.IsNull()) {
      resolved[i].route = StepIdentityRoute::part_representation;
    } else {
      resolved[i].route = StepIdentityRoute::ambiguous;
    }
  }

  return resolved;
}

inline std::vector<Handle(StepBasic_ProductDefinition)> resolve_product_definitions(
    const StepIdentityIndex &index, const std::vector<TopoDS_Shape> &shapes,
    const std::vector<TDF_Label> &labels,
    const std::vector<bool> &assemblies) {
  const std::vector<StepDefinitionIdentity> identities =
      resolve_definition_identities(index, shapes, labels, assemblies);
  std::vector<Handle(StepBasic_ProductDefinition)> products(identities.size());
  for (std::size_t i = 0; i < identities.size(); ++i) {
    products[i] = identities[i].product;
  }
  return products;
}

/// The key of every definition, empty where there is none.
inline std::vector<std::string> definition_keys(
    const StepIdentityIndex &index, const std::vector<TopoDS_Shape> &shapes,
    const std::vector<TDF_Label> &labels,
    const std::vector<bool> &assemblies) {
  const std::vector<Handle(StepBasic_ProductDefinition)> products =
      resolve_product_definitions(index, shapes, labels, assemblies);
  std::vector<std::string> keys(products.size());
  for (std::size_t i = 0; i < products.size(); ++i) {
    keys[i] = definition_key(index.source_ident(products[i]));
  }
  return keys;
}

/// The first definition with no key, or `count` when every one has a key.
inline std::size_t first_without_key(const std::vector<std::string> &keys) {
  for (std::size_t i = 0; i < keys.size(); ++i) {
    if (keys[i].empty()) {
      return i;
    }
  }
  return keys.size();
}

/// The first key that some earlier definition already used, or `count`.
inline std::size_t first_repeated_key(const std::vector<std::string> &keys) {
  std::unordered_map<std::string, std::size_t> first_seen;
  first_seen.reserve(keys.size());
  for (std::size_t i = 0; i < keys.size(); ++i) {
    if (!keys[i].empty() && !first_seen.emplace(keys[i], i).second) {
      return i;
    }
  }
  return keys.size();
}

}  // namespace ferritecad

#endif  // FERRITECAD_STEP_IDENTITY_HPP
