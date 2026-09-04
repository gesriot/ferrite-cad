// SPDX-License-Identifier: MIT
//
// The independent oracle for §22B-1e1, built from pinned ufbx 0.23.0.
//
// Unity is asked what its references do. It is never asked which FerriteCAD
// definition an object is: §22B-1c measured that several definitions of one
// real assembly carry the same designation, so a name is not an answer. This
// program reads the same bytes and reports the raw FBX object numbers, the
// object classes, the hierarchy, the geometry sharing and the custom
// properties, so the Unity report can be joined to something that never went
// through the importer.
//
// It also hashes the file it opened. The measurement is worthless if this
// reader and the editor looked at different bytes, so both compute the same
// 64-bit FNV-1a over the whole file and the verifier refuses a mismatch. That
// is a content check between two programs, not a security digest.

#include "ufbx.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

static const char *basename_only(const char *path)
{
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

// The raw FBX object number, which is what the writer derives from a scene
// position and what a reader that relinks by identifier would use.
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

static void print_property(const ufbx_node *node, const char *name)
{
    ufbx_prop *prop = ufbx_find_prop(&node->props, name);
    if (prop && (prop->flags & UFBX_PROP_FLAG_USER_DEFINED)) {
        json_string(prop->value_str);
    } else {
        fputs("\"\"", stdout);
    }
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
        fprintf(stderr, "%s\n", message);
        return 1;
    }

    if (!first) fputs(",\n", stdout);
    fputs("  {\"file\":", stdout);
    json_string_data(basename_only(path), strlen(basename_only(path)));
    printf(",\"bytes\":%" PRIu64 ",\"fnv1a64\":\"%016" PRIx64 "\"", size, digest);
    printf(",\"version\":%u", scene->metadata.version);

    // Every object the file numbers, with the number the file gave it.
    fputs(",\"objects\":[", stdout);
    bool wrote = false;
    for (size_t i = 0; i < scene->elements.count; i++) {
        const ufbx_element *element = scene->elements.data[i];
        const char *class_name = element_class(element->type);
        if (strcmp(class_name, "Other") == 0) continue;
        int64_t number = object_number(element);
        if (number == 0) continue;  // ufbx's synthetic root has no file object
        if (wrote) putchar(',');
        wrote = true;
        printf("{\"class\":\"%s\",\"object_number\":%" PRId64 ",\"name\":", class_name, number);
        json_string(element->name);
        putchar('}');
    }
    fputs("]", stdout);

    // The hierarchy, the geometry each placement uses, and the durable keys.
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
        fputs(",\"definition_key\":", stdout);
        print_property(node, "FerriteCADDefinitionKey");
        fputs(",\"node_key\":", stdout);
        print_property(node, "FerriteCADNodeKey");
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
            putchar('}');
        }
        fputs("]}", stdout);
    }
    fputs("]", stdout);

    // The four confusions this measurement must actually contain, counted in
    // the file rather than assumed from the way it was built.
    size_t repeated_model_names = 0;
    size_t repeated_sibling_names = 0;
    size_t shared_geometry_placements = 0;
    size_t repeated_slot_names = 0;
    // Two definitions a source called the same thing. The writer appends its
    // own position to a geometry name, so the comparison stops before it.
    size_t repeated_geometry_display_names = 0;
    for (size_t i = 0; i < scene->meshes.count; i++) {
        for (size_t j = 0; j < i; j++) {
            ufbx_string a = scene->meshes.data[i]->element.name;
            ufbx_string b = scene->meshes.data[j]->element.name;
            size_t ka = a.length, kb = b.length;
            while (ka && a.data[ka - 1] != '#') ka--;
            while (kb && b.data[kb - 1] != '#') kb--;
            if (ka > 1 && kb > 1 && ka == kb && memcmp(a.data, b.data, ka - 1) == 0) {
                repeated_geometry_display_names++;
            }
        }
    }
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        for (size_t j = 0; j < i; j++) {
            const ufbx_node *other = scene->nodes.data[j];
            if (other->is_root) continue;
            if (node->name.length == other->name.length
                && memcmp(node->name.data, other->name.data, node->name.length) == 0) {
                repeated_model_names++;
                if (node->parent == other->parent) repeated_sibling_names++;
            }
        }
        for (size_t m = 0; m < node->materials.count; m++) {
            for (size_t n = 0; n < m; n++) {
                ufbx_string a = node->materials.data[m]->element.name;
                ufbx_string b = node->materials.data[n]->element.name;
                // The writer appends its own position to every material name,
                // so equal slot names are compared before that suffix.
                size_t ka = a.length, kb = b.length;
                while (ka && a.data[ka - 1] != '#') ka--;
                while (kb && b.data[kb - 1] != '#') kb--;
                if (ka > 1 && kb > 1 && ka == kb
                    && memcmp(a.data, b.data, ka - 1) == 0) {
                    repeated_slot_names++;
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
    printf(",\"facts\":{\"models\":%zu,\"geometries\":%zu,\"materials\":%zu,"
           "\"repeated_model_names\":%zu,\"repeated_sibling_names\":%zu,"
           "\"placements_sharing_one_geometry\":%zu,\"repeated_material_slot_names\":%zu,"
           "\"repeated_geometry_display_names\":%zu}",
           scene->nodes.count - 1, scene->meshes.count, scene->materials.count,
           repeated_model_names, repeated_sibling_names,
           shared_geometry_placements, repeated_slot_names,
           repeated_geometry_display_names);
    putchar('}');
    ufbx_free_scene(scene);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: read_identity FILE...\n");
        return 2;
    }
    fputs("{\n \"schema\": \"ferritecad.fbx-identity-oracle.v1\",\n \"files\": [\n", stdout);
    for (int i = 1; i < argc; i++) {
        if (read_one(argv[i], i == 1) != 0) {
            return 1;
        }
    }
    fputs("\n ]\n}\n", stdout);
    return 0;
}
