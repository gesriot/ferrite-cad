// SPDX-License-Identifier: MIT
//
// The independent oracle for §22B-1e2a, built from pinned ufbx 0.23.0.
//
// It reads exactly the bytes Unity is about to import, and it reads them
// without the importer. Two things it reports cannot be got from Unity at all:
// the FBX object name as the file spells it — Unity renames, truncates and
// disambiguates before anyone sees it — and whether a candidate's identity
// property is actually present on every node that claims it.
//
// It also counts the collision this slice exists for: how many source-local
// definition keys name more than one geometry in one file. On the production
// property that count is not zero, because two imported sources may legally
// carry the same `step.product_definition#42`. A candidate that claims a
// durable identity has to make it zero on the same document.
//
// The file it opened is hashed with the same 64-bit FNV-1a the editor
// computes, so "the oracle read a different file" is a refusal rather than an
// assumption. That is a content check between two programs, not a security
// digest.

#include "ufbx.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_OBJECTS 4096

static void json_string_data(const char *data, size_t length)
{
    putchar('"');
    for (size_t i = 0; i < length; i++) {
        unsigned char ch = (unsigned char)data[i];
        switch (ch) {
        case '"': fputs("\\\"", stdout); break;
        case '\\': fputs("\\\\", stdout); break;
        case '\b': fputs("\\b", stdout); break;
        case '\f': fputs("\\f", stdout); break;
        case '\n': fputs("\\n", stdout); break;
        case '\r': fputs("\\r", stdout); break;
        case '\t': fputs("\\t", stdout); break;
        default:
            if (ch < 0x20) {
                printf("\\u%04x", (unsigned)ch);
            } else {
                putchar((int)ch);
            }
            break;
        }
    }
    putchar('"');
}

static void json_string(ufbx_string value)
{
    json_string_data(value.data, value.length);
}

// The last two path components, because every candidate writes the same file
// names into its own directory and a report keyed on the base name alone would
// silently merge four candidates into one.
static const char *basename_only(const char *path)
{
    const char *slash = strrchr(path, '/');
    if (!slash) return path;
    for (const char *scan = path; scan != slash; scan++) {
        if (*scan == '/' && strchr(scan + 1, '/') == slash) return scan + 1;
    }
    return path;
}

static int64_t object_number(const ufbx_element *element)
{
    if (!element->dom_node || element->dom_node->values.count == 0) return 0;
    const ufbx_dom_value *value = &element->dom_node->values.data[0];
    return value->value_int;
}

static const char *element_class(ufbx_element_type type)
{
    switch (type) {
    case UFBX_ELEMENT_NODE: return "Model";
    case UFBX_ELEMENT_MESH: return "Geometry";
    case UFBX_ELEMENT_MATERIAL: return "Material";
    default: return "Other";
    }
}

static void print_path(const ufbx_node *node)
{
    if (node->parent && !node->parent->is_root) {
        print_path(node->parent);
    }
    putchar('/');
    for (size_t i = 0; i < node->name.length; i++) {
        char ch = node->name.data[i];
        if (ch == '"' || ch == '\\') putchar('\\');
        putchar(ch);
    }
}

// A user-defined property, or an empty string. Never a property Unity or the
// FBX standard defines: a candidate is measured on what FerriteCAD wrote.
static ufbx_string user_property(const ufbx_node *node, const char *name)
{
    static const ufbx_string empty = { "", 0 };
    ufbx_prop *prop = ufbx_find_prop(&node->props, name);
    if (prop && (prop->flags & UFBX_PROP_FLAG_USER_DEFINED)) {
        return prop->value_str;
    }
    return empty;
}

static void print_property(const ufbx_node *node, const char *name)
{
    json_string(user_property(node, name));
}

static int same(ufbx_string left, ufbx_string right)
{
    return left.length == right.length
        && (left.length == 0 || memcmp(left.data, right.data, left.length) == 0);
}

static int hash_file(const char *path, uint64_t *digest, uint64_t *size)
{
    FILE *file = fopen(path, "rb");
    if (!file) return 0;
    uint64_t hash = 14695981039346656037ull;
    uint64_t bytes = 0;
    unsigned char buffer[65536];
    size_t read;
    while ((read = fread(buffer, 1, sizeof(buffer), file)) > 0) {
        for (size_t i = 0; i < read; i++) {
            hash ^= (uint64_t)buffer[i];
            hash *= 1099511628211ull;
        }
        bytes += (uint64_t)read;
    }
    fclose(file);
    *digest = hash;
    *size = bytes;
    return 1;
}

// How many distinct values of `property` name more than one distinct geometry
// in this file. Zero means the property tells every geometry-owning definition
// apart; anything else means it does not, whatever it is called.
static size_t key_collisions(const ufbx_scene *scene, const char *property)
{
    ufbx_string keys[MAX_OBJECTS];
    int64_t geometries[MAX_OBJECTS];
    int split[MAX_OBJECTS];
    size_t count = 0;
    size_t collisions = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root || !node->mesh) continue;
        ufbx_string key = user_property(node, property);
        if (key.length == 0) continue;
        int64_t geometry = object_number(&node->mesh->element);
        size_t slot = count;
        for (size_t j = 0; j < count; j++) {
            if (same(keys[j], key)) { slot = j; break; }
        }
        if (slot == count) {
            if (count == MAX_OBJECTS) {
                fprintf(stderr, "more distinct keys than this reader counts\n");
                exit(1);
            }
            keys[count] = key;
            geometries[count] = geometry;
            split[count] = 0;
            count++;
            continue;
        }
        if (geometries[slot] != geometry && !split[slot]) {
            split[slot] = 1;
            collisions++;
        }
    }
    return collisions;
}

static size_t nodes_carrying(const ufbx_scene *scene, const char *property)
{
    size_t count = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (user_property(node, property).length > 0) count++;
    }
    return count;
}

static int read_one(const char *path, bool first)
{
    uint64_t digest = 0;
    uint64_t size = 0;
    if (!hash_file(path, &digest, &size)) {
        fprintf(stderr, "cannot read %s\n", path);
        return 1;
    }

    ufbx_load_opts opts;
    memset(&opts, 0, sizeof(opts));
    opts.strict = true;
    opts.retain_dom = true;
    ufbx_error error;
    ufbx_scene *scene = ufbx_load_file(path, &opts, &error);
    if (!scene) {
        char message[512];
        ufbx_format_error(message, sizeof(message), &error);
        fprintf(stderr, "%s: %s\n", path, message);
        return 1;
    }

    if (!first) fputs(",\n", stdout);
    fputs("  {\"file\":", stdout);
    json_string_data(basename_only(path), strlen(basename_only(path)));
    printf(",\"bytes\":%" PRIu64 ",\"fnv1a64\":\"%016" PRIx64 "\"", size, digest);
    printf(",\"version\":%u", scene->metadata.version);

    fputs(",\"objects\":[", stdout);
    bool wrote = false;
    size_t longest_name_bytes = 0;
    size_t non_ascii_names = 0;
    for (size_t i = 0; i < scene->elements.count; i++) {
        const ufbx_element *element = scene->elements.data[i];
        const char *class_name = element_class(element->type);
        if (strcmp(class_name, "Other") == 0) continue;
        int64_t number = object_number(element);
        if (number == 0) continue;  // ufbx's synthetic root has no file object
        if (element->name.length > longest_name_bytes) longest_name_bytes = element->name.length;
        for (size_t c = 0; c < element->name.length; c++) {
            if ((unsigned char)element->name.data[c] >= 0x80) { non_ascii_names++; break; }
        }
        if (wrote) putchar(',');
        wrote = true;
        printf("{\"class\":\"%s\",\"object_number\":%" PRId64 ",\"name\":", class_name, number);
        json_string(element->name);
        printf(",\"name_bytes\":%zu}", element->name.length);
    }
    fputs("]", stdout);

    fputs(",\"nodes\":[", stdout);
    wrote = false;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (wrote) putchar(',');
        wrote = true;
        fputs("{\"path\":\"", stdout);
        print_path(node);
        fputs("\",\"name\":", stdout);
        json_string(node->name);
        printf(",\"object_number\":%" PRId64, object_number(&node->element));
        fputs(",\"node_key\":", stdout);
        print_property(node, "FerriteCADNodeKey");
        fputs(",\"definition_key\":", stdout);
        print_property(node, "FerriteCADDefinitionKey");
        fputs(",\"source_id\":", stdout);
        print_property(node, "FerriteCADSourceId");
        fputs(",\"definition_id\":", stdout);
        print_property(node, "FerriteCADDefinitionId");
        fputs(",\"occurrence_id\":", stdout);
        print_property(node, "FerriteCADOccurrenceId");
        fputs(",\"display_name\":", stdout);
        print_property(node, "FerriteCADDisplayName");
        fputs(",\"geometry_display_name\":", stdout);
        print_property(node, "FerriteCADGeometryDisplayName");
        fputs(",\"omission\":", stdout);
        print_property(node, "FerriteCADGeometryOmission");
        if (node->mesh) {
            printf(",\"geometry_object_number\":%" PRId64
                   ",\"geometry_vertices\":%zu,\"geometry_name\":",
                   object_number(&node->mesh->element), node->mesh->num_vertices);
            json_string(node->mesh->element.name);
        } else {
            fputs(",\"geometry_object_number\":0,\"geometry_vertices\":0,\"geometry_name\":\"\"",
                  stdout);
        }
        fputs(",\"materials\":[", stdout);
        for (size_t m = 0; m < node->materials.count; m++) {
            if (m) putchar(',');
            printf("{\"object_number\":%" PRId64 ",\"name\":",
                   object_number(&node->materials.data[m]->element));
            json_string(node->materials.data[m]->element.name);
            fputs(",\"display_name\":", stdout);
            {
                char property[64];
                snprintf(property, sizeof(property), "FerriteCADMaterialDisplayName%zu", m);
                print_property(node, property);
            }
            putchar('}');
        }
        fputs("]}", stdout);
    }
    fputs("]", stdout);

    // The confusions this measurement must actually contain, counted in the
    // file rather than assumed from the way it was built.
    size_t repeated_model_names = 0;
    size_t repeated_sibling_names = 0;
    size_t shared_geometry_placements = 0;
    size_t repeated_slot_designations = 0;
    size_t structural_nodes = 0;
    size_t omitted_nodes = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (!node->mesh) {
            if (user_property(node, "FerriteCADGeometryOmission").length > 0) {
                omitted_nodes++;
            } else {
                structural_nodes++;
            }
        }
        for (size_t j = 0; j < i; j++) {
            const ufbx_node *other = scene->nodes.data[j];
            if (other->is_root) continue;
            if (same(node->name, other->name)) {
                repeated_model_names++;
                if (node->parent == other->parent) repeated_sibling_names++;
            }
        }
    }
    // Two definitions whose *designations* are equal. Under the control that
    // designation is the object name with the writer's position suffix, and
    // under a candidate it is the display-name property, so both are counted
    // the same way: on what a person would read.
    size_t repeated_designations = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *left = scene->nodes.data[i];
        if (left->is_root || !left->mesh) continue;
        ufbx_string a = user_property(left, "FerriteCADGeometryDisplayName");
        if (a.length == 0) a = left->name;
        for (size_t j = 0; j < i; j++) {
            const ufbx_node *right = scene->nodes.data[j];
            if (right->is_root || !right->mesh) continue;
            if (left->mesh == right->mesh) continue;
            ufbx_string b = user_property(right, "FerriteCADGeometryDisplayName");
            if (b.length == 0) b = right->name;
            if (same(a, b)) repeated_designations++;
        }
    }
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        for (size_t m = 0; m < node->materials.count; m++) {
            for (size_t n = 0; n < m; n++) {
                char left[64], right[64];
                snprintf(left, sizeof(left), "FerriteCADMaterialDisplayName%zu", m);
                snprintf(right, sizeof(right), "FerriteCADMaterialDisplayName%zu", n);
                ufbx_string a = user_property(node, left);
                ufbx_string b = user_property(node, right);
                if (a.length == 0) a = node->materials.data[m]->element.name;
                if (b.length == 0) b = node->materials.data[n]->element.name;
                // The control appends the material's own position to the name,
                // so equal designations are compared before that suffix.
                size_t ka = a.length, kb = b.length;
                while (ka && a.data[ka - 1] != '#') ka--;
                while (kb && b.data[kb - 1] != '#') kb--;
                if (ka > 1 && kb > 1 && ka == kb && memcmp(a.data, b.data, ka - 1) == 0) {
                    repeated_slot_designations++;
                } else if (ka <= 1 && kb <= 1 && same(a, b)) {
                    repeated_slot_designations++;
                }
            }
        }
    }
    for (size_t i = 0; i < scene->meshes.count; i++) {
        const ufbx_mesh *mesh = scene->meshes.data[i];
        if (mesh->instances.count > 1) {
            shared_geometry_placements += mesh->instances.count;
        }
    }

    size_t placed = scene->nodes.count - 1;
    printf(",\"facts\":{\"models\":%zu,\"geometries\":%zu,\"materials\":%zu,"
           "\"repeated_model_names\":%zu,\"repeated_sibling_names\":%zu,"
           "\"placements_sharing_one_geometry\":%zu,\"repeated_material_slot_names\":%zu,"
           "\"repeated_geometry_display_names\":%zu,"
           "\"structural_nodes\":%zu,\"omitted_nodes\":%zu,"
           "\"definition_key_collisions\":%zu,\"definition_id_collisions\":%zu,"
           "\"nodes_with_source_id\":%zu,\"nodes_with_definition_id\":%zu,"
           "\"nodes_with_occurrence_id\":%zu,\"nodes_with_display_name\":%zu,"
           "\"longest_object_name_bytes\":%zu,\"non_ascii_object_names\":%zu}",
           placed, scene->meshes.count, scene->materials.count,
           repeated_model_names, repeated_sibling_names,
           shared_geometry_placements, repeated_slot_designations,
           repeated_designations, structural_nodes, omitted_nodes,
           key_collisions(scene, "FerriteCADDefinitionKey"),
           key_collisions(scene, "FerriteCADDefinitionId"),
           nodes_carrying(scene, "FerriteCADSourceId"),
           nodes_carrying(scene, "FerriteCADDefinitionId"),
           nodes_carrying(scene, "FerriteCADOccurrenceId"),
           nodes_carrying(scene, "FerriteCADDisplayName"),
           longest_name_bytes, non_ascii_names);
    putchar('}');
    ufbx_free_scene(scene);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: read_channels FILE...\n");
        return 2;
    }
    fputs("{\n \"schema\": \"ferritecad.fbx-channel-oracle.v1\",\n \"files\": [\n", stdout);
    for (int i = 1; i < argc; i++) {
        if (read_one(argv[i], i == 1) != 0) {
            return 1;
        }
    }
    fputs("\n ]\n}\n", stdout);
    return 0;
}
