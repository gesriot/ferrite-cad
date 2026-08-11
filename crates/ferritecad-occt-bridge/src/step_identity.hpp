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
#include <Interface_Graph.hxx>
#include <StepBasic_ProductDefinition.hxx>
#include <StepData_StepModel.hxx>
#include <StepRepr_NextAssemblyUsageOccurrence.hxx>
#include <StepRepr_ProductDefinitionShape.hxx>
#include <StepShape_AdvancedBrepShapeRepresentation.hxx>
#include <StepShape_ShapeDefinitionRepresentation.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS_Shape.hxx>
#include <XSControl_TransferReader.hxx>

#include <algorithm>
#include <string>
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

/// Walks from an entity to the PRODUCT_DEFINITION that owns it.
///
/// Typed, not a graph crawl. The chain does not run one way:
///
///     MANIFOLD_SOLID_BREP
///       <- shared by  ADVANCED_BREP_SHAPE_REPRESENTATION
///       <- shared by  SHAPE_DEFINITION_REPRESENTATION
///       -> refers to  PRODUCT_DEFINITION_SHAPE
///       -> refers to  PRODUCT_DEFINITION
///
/// Every transition is checked by its concrete STEP type and by the field that
/// points back to the entity just visited. Merely limiting a generic `Sharings`
/// crawl to two or three hops would not make it typed: it could still enter an
/// unrelated property representation and come back with a neighbouring part.
inline Handle(StepBasic_ProductDefinition) product_definition_of(
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
/// from a known component to its possible parents.
inline std::vector<Handle(StepBasic_ProductDefinition)> assemblies_containing(
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
    // collected here is which assemblies, not how many times, so an assembly
    // named twice is named once.
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

/// The identifier the file gave an entity, or 0 when it gave none.
///
/// `IdentLabel` is read from the file. `Number` is where the entity sits in
/// the model this run, and Open CASCADE prints the two alike — returning it
/// here is how a reader talks itself into believing it has an identity when
/// what it has is a position. So it is not returned.
inline int source_ident(const Handle(StepData_StepModel) & model,
                        const Handle(Standard_Transient) & entity) {
  if (model.IsNull() || entity.IsNull()) {
    return 0;
  }
  const int from_file = model->IdentLabel(entity);
  return from_file > 0 ? from_file : 0;
}

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

/// Resolves the product definition behind each definition, parts first and
/// then their assemblies.
///
/// `children` gives, for each definition, the positions of the definitions
/// directly inside it. Assemblies are resolved to a fixed point so that an
/// assembly of assemblies comes out whichever order the tree was walked in.
/// A definition that cannot be identified is left null; the caller decides
/// what that means, and this refuses to guess on its behalf.
///
/// The probe reports what this found and the bridge keys a scene by it. They
/// share the resolution rather than the conclusion, which is the part that
/// must not differ between the measurement and the product.
inline std::vector<Handle(StepBasic_ProductDefinition)> resolve_product_definitions(
    const Interface_Graph &graph,
    const Handle(XSControl_TransferReader) & transfer,
    const std::vector<TopoDS_Shape> &shapes,
    const std::vector<std::vector<std::size_t>> &children) {
  const std::size_t count = shapes.size();
  std::vector<Handle(StepBasic_ProductDefinition)> products(count);

  // What the geometry itself can say. A part reaches its product definition
  // through the representation holding its solid.
  for (std::size_t i = 0; i < count; ++i) {
    for (const Handle(Standard_Transient) &entity :
         entities_from(transfer, shapes[i])) {
      Handle(StepBasic_ProductDefinition) product =
          product_definition_of(graph, entity);
      if (!product.IsNull()) {
        products[i] = product;
        break;
      }
    }
  }

  // And what the assembly structure can say about the rest: the parent every
  // component agrees on, if they agree on exactly one.
  for (bool changed = true; changed;) {
    changed = false;
    for (std::size_t i = 0; i < count; ++i) {
      if (!products[i].IsNull() || i >= children.size() || children[i].empty()) {
        continue;
      }

      std::vector<Handle(StepBasic_ProductDefinition)> agreed;
      bool first = true;
      bool usable = true;
      for (const std::size_t child : children[i]) {
        if (child >= count || products[child].IsNull()) {
          usable = false;
          break;
        }
        std::vector<Handle(StepBasic_ProductDefinition)> parents =
            assemblies_containing(graph, products[child]);
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
        products[i] = agreed.front();
        changed = true;
      }
    }
  }

  return products;
}

/// The key of every definition, empty where there is none.
inline std::vector<std::string> definition_keys(
    const Interface_Graph &graph, const Handle(StepData_StepModel) & model,
    const Handle(XSControl_TransferReader) & transfer,
    const std::vector<TopoDS_Shape> &shapes,
    const std::vector<std::vector<std::size_t>> &children) {
  const std::vector<Handle(StepBasic_ProductDefinition)> products =
      resolve_product_definitions(graph, transfer, shapes, children);
  std::vector<std::string> keys(products.size());
  for (std::size_t i = 0; i < products.size(); ++i) {
    keys[i] = definition_key(source_ident(model, products[i]));
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
  for (std::size_t i = 0; i < keys.size(); ++i) {
    for (std::size_t j = 0; j < i; ++j) {
      if (!keys[i].empty() && keys[i] == keys[j]) {
        return i;
      }
    }
  }
  return keys.size();
}

}  // namespace ferritecad

#endif  // FERRITECAD_STEP_IDENTITY_HPP
