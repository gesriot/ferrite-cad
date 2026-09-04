// SPDX-License-Identifier: MIT
//
// A real asset holding real references, which is the only honest way to ask
// what a prefab, a scene or a material would do.
//
// Unity writes each of these fields as `{fileID, guid, type}` into a file on
// disk. Reimporting the model and reading these fields back is the same
// question a project asks every time it opens a scene that points into an
// imported model, and it is a different question from "is some object with
// this name still there".
using System.Collections.Generic;
using UnityEngine;

internal sealed class FerriteGraphReferences : ScriptableObject
{
    public List<Mesh> meshes = new List<Mesh>();
    public List<Material> materials = new List<Material>();
    public List<GameObject> objects = new List<GameObject>();
}
