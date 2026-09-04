// SPDX-License-Identifier: MIT
//
// The independent oracle for §22B-1e2b, built from pinned ufbx 0.23.0.
//
// It reads exactly the bytes Unity is about to import, and it reads them
// without the importer. Three things it reports cannot be got from Unity at
// all: the FBX object name as the file spells it — Unity renames, truncates
// and disambiguates before anyone sees it — the graph topology as the file
// spells it, and a content digest of every geometry array, material colour and
// node transform.
//
// That last one is what makes a *structural* transformer measurable. §22B-1e2a
// only had to show that a rewriter did not change a name it was not asked to
// change; this slice adds and re-points objects, so the claim "geometry,
// materials, transforms and the existing object numbers are the control's" has
// to be checked against the control by a program that never saw the
// transformer. The digests below are what the verifier compares.
//
// The file it opened is hashed with the same 64-bit FNV-1a the editor
// computes, so "the oracle read a different file" is a refusal rather than an
// assumption. That is a content check between two programs, not a security
// digest.

#include "ufbx.h"

#include <inttypes.h>
#include <math.h>
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

// The last two path components, because every variant writes the same file
// names into its own directory and a report keyed on the base name alone would
// silently merge five variants into one.
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
// FBX standard defines: a variant is measured on what FerriteCAD wrote.
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

// ------------------------------------------------------------- the digests
//
// A content digest, mixed from the numbers themselves rather than from any
// formatting, so a comparison between a variant and the control is a
// comparison of what the arrays hold. Rounded to 1e-6 first, because two files
// that spell the same number differently are the same geometry and the
// transformer is not allowed to spell any of them at all.

static void mix(uint64_t *hash, uint64_t value)
{
    for (int i = 0; i < 8; i++) {
        *hash ^= (value >> (i * 8)) & 0xff;
        *hash *= 1099511628211ull;
    }
}

static void mix_real(uint64_t *hash, ufbx_real value)
{
    double scaled = (double)value * 1000000.0;
    double rounded = (scaled < 0.0) ? ceil(scaled - 0.5) : floor(scaled + 0.5);
    if (rounded == 0.0) rounded = 0.0;  // -0 and 0 are one number here
    mix(hash, (uint64_t)(int64_t)rounded);
}

static void mix_vec3(uint64_t *hash, ufbx_vec3 value)
{
    mix_real(hash, value.x);
    mix_real(hash, value.y);
    mix_real(hash, value.z);
}

static uint64_t geometry_digest(const ufbx_mesh *mesh)
{
    uint64_t hash = 14695981039346656037ull;
    mix(&hash, mesh->num_vertices);
    mix(&hash, mesh->num_indices);
    mix(&hash, mesh->num_faces);
    for (size_t i = 0; i < mesh->num_vertices; i++) {
        mix_vec3(&hash, mesh->vertices.data[i]);
    }
    for (size_t i = 0; i < mesh->num_indices; i++) {
        mix(&hash, mesh->vertex_indices.data[i]);
        if (mesh->vertex_normal.exists) {
            mix_vec3(&hash, ufbx_get_vertex_vec3(&mesh->vertex_normal, i));
        }
    }
    for (size_t i = 0; i < mesh->faces.count; i++) {
        mix(&hash, mesh->faces.data[i].index_begin);
        mix(&hash, mesh->faces.data[i].num_indices);
    }
    return hash;
}

// Kept out of the geometry digest above and reported beside it, because it is
// not a property of the arrays. ufbx resolves a face's material through the
// *node* that instantiates the mesh, so a variant that gives a geometry one
// more instance changes this without changing a single number the writer
// emitted. Separating the two is what lets the verifier insist the arrays are
// byte-for-byte the control's while still showing the slot mapping moving.
static uint64_t face_material_digest(const ufbx_mesh *mesh)
{
    uint64_t hash = 14695981039346656037ull;
    mix(&hash, mesh->face_material.count);
    for (size_t i = 0; i < mesh->face_material.count; i++) {
        mix(&hash, (uint64_t)(uint32_t)mesh->face_material.data[i]);
    }
    return hash;
}

static uint64_t material_digest(const ufbx_material *material)
{
    uint64_t hash = 14695981039346656037ull;
    mix_vec3(&hash, material->fbx.diffuse_color.value_vec3);
    mix_real(&hash, material->fbx.diffuse_factor.value_real);
    mix_vec3(&hash, material->fbx.specular_color.value_vec3);
    mix_real(&hash, material->fbx.transparency_factor.value_real);
    return hash;
}

static uint64_t transform_digest(const ufbx_node *node)
{
    uint64_t hash = 14695981039346656037ull;
    mix_vec3(&hash, node->local_transform.translation);
    mix_real(&hash, node->local_transform.rotation.x);
    mix_real(&hash, node->local_transform.rotation.y);
    mix_real(&hash, node->local_transform.rotation.z);
    mix_real(&hash, node->local_transform.rotation.w);
    mix_vec3(&hash, node->local_transform.scale);
    return hash;
}

static uint64_t world_digest(const ufbx_node *node)
{
    uint64_t hash = 14695981039346656037ull;
    for (int row = 0; row < 3; row++) {
        for (int column = 0; column < 4; column++) {
            mix_real(&hash, node->node_to_world.v[column * 3 + row]);
        }
    }
    return hash;
}

static size_t triangle_count(const ufbx_mesh *mesh)
{
    size_t triangles = 0;
    for (size_t i = 0; i < mesh->faces.count; i++) {
        if (mesh->faces.data[i].num_indices >= 3) {
            triangles += mesh->faces.data[i].num_indices - 2;
        }
    }
    return triangles;
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

    // Every geometry, with the digest of the arrays it holds. The verifier
    // compares this set with the control's, so a transformer that touched a
    // vertex is a refusal rather than a footnote.
    fputs(",\"geometries\":[", stdout);
    for (size_t i = 0; i < scene->meshes.count; i++) {
        const ufbx_mesh *mesh = scene->meshes.data[i];
        if (i) putchar(',');
        printf("{\"object_number\":%" PRId64 ",\"name\":", object_number(&mesh->element));
        json_string(mesh->element.name);
        printf(",\"vertices\":%zu,\"indices\":%zu,\"triangles\":%zu,\"instances\":%zu,"
               "\"digest\":\"%016" PRIx64 "\",\"face_material_digest\":\"%016" PRIx64 "\"}",
               mesh->num_vertices, mesh->num_indices, triangle_count(mesh),
               mesh->instances.count, geometry_digest(mesh), face_material_digest(mesh));
    }
    fputs("]", stdout);

    fputs(",\"materials\":[", stdout);
    for (size_t i = 0; i < scene->materials.count; i++) {
        const ufbx_material *material = scene->materials.data[i];
        if (i) putchar(',');
        printf("{\"object_number\":%" PRId64 ",\"name\":", object_number(&material->element));
        json_string(material->element.name);
        printf(",\"digest\":\"%016" PRIx64 "\"}", material_digest(material));
    }
    fputs("]", stdout);

    fputs(",\"nodes\":[", stdout);
    bool wrote = false;
    size_t longest_name_bytes = 0;
    size_t non_ascii_names = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (node->name.length > longest_name_bytes) longest_name_bytes = node->name.length;
        for (size_t c = 0; c < node->name.length; c++) {
            if ((unsigned char)node->name.data[c] >= 0x80) { non_ascii_names++; break; }
        }
        if (wrote) putchar(',');
        wrote = true;
        fputs("{\"path\":\"", stdout);
        print_path(node);
        fputs("\",\"name\":", stdout);
        json_string(node->name);
        printf(",\"object_number\":%" PRId64, object_number(&node->element));
        printf(",\"parent_object_number\":%" PRId64,
               node->parent && !node->parent->is_root ? object_number(&node->parent->element) : 0);
        printf(",\"children\":%zu", node->children.count);
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
        fputs(",\"graph_role\":", stdout);
        print_property(node, "FerriteCADGraphRole");
        fputs(",\"omission\":", stdout);
        print_property(node, "FerriteCADGeometryOmission");
        printf(",\"local_transform_digest\":\"%016" PRIx64 "\"", transform_digest(node));
        printf(",\"world_transform_digest\":\"%016" PRIx64 "\"", world_digest(node));
        if (node->mesh) {
            printf(",\"geometry_object_number\":%" PRId64
                   ",\"geometry_vertices\":%zu,\"geometry_triangles\":%zu,\"geometry_name\":",
                   object_number(&node->mesh->element), node->mesh->num_vertices,
                   triangle_count(node->mesh));
            json_string(node->mesh->element.name);
        } else {
            fputs(",\"geometry_object_number\":0,\"geometry_vertices\":0"
                  ",\"geometry_triangles\":0,\"geometry_name\":\"\"",
                  stdout);
        }
        fputs(",\"materials\":[", stdout);
        for (size_t m = 0; m < node->materials.count; m++) {
            if (m) putchar(',');
            printf("{\"object_number\":%" PRId64 ",\"name\":",
                   object_number(&node->materials.data[m]->element));
            json_string(node->materials.data[m]->element.name);
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
    size_t structural_nodes = 0;
    size_t omitted_nodes = 0;
    size_t geometry_bearing_nodes = 0;
    size_t carrier_nodes = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (node->mesh) {
            geometry_bearing_nodes++;
        } else if (user_property(node, "FerriteCADGeometryOmission").length > 0) {
            omitted_nodes++;
        } else {
            structural_nodes++;
        }
        ufbx_string role = user_property(node, "FerriteCADGraphRole");
        if (role.length > 0 && role.data[0] != 'o') carrier_nodes++;
        for (size_t j = 0; j < i; j++) {
            const ufbx_node *other = scene->nodes.data[j];
            if (other->is_root) continue;
            if (same(node->name, other->name)) {
                repeated_model_names++;
                if (node->parent == other->parent) repeated_sibling_names++;
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
           "\"placements_sharing_one_geometry\":%zu,"
           "\"geometry_bearing_nodes\":%zu,\"machine_named_carrier_nodes\":%zu,"
           "\"structural_nodes\":%zu,\"omitted_nodes\":%zu,"
           "\"definition_key_collisions\":%zu,\"definition_id_collisions\":%zu,"
           "\"nodes_with_source_id\":%zu,\"nodes_with_definition_id\":%zu,"
           "\"nodes_with_occurrence_id\":%zu,"
           "\"longest_object_name_bytes\":%zu,\"non_ascii_object_names\":%zu}",
           placed, scene->meshes.count, scene->materials.count,
           repeated_model_names, repeated_sibling_names,
           shared_geometry_placements, geometry_bearing_nodes, carrier_nodes,
           structural_nodes, omitted_nodes,
           key_collisions(scene, "FerriteCADDefinitionKey"),
           key_collisions(scene, "FerriteCADDefinitionId"),
           nodes_carrying(scene, "FerriteCADSourceId"),
           nodes_carrying(scene, "FerriteCADDefinitionId"),
           nodes_carrying(scene, "FerriteCADOccurrenceId"),
           longest_name_bytes, non_ascii_names);
    putchar('}');
    ufbx_free_scene(scene);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: read_graphs FILE...\n");
        return 2;
    }
    fputs("{\n \"schema\": \"ferritecad.fbx-graph-oracle.v1\",\n \"files\": [\n", stdout);
    for (int i = 1; i < argc; i++) {
        if (read_one(argv[i], i == 1) != 0) {
            return 1;
        }
    }
    fputs("\n ]\n}\n", stdout);
    return 0;
}
