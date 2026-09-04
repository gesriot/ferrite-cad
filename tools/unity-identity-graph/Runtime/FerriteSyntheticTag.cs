// SPDX-License-Identifier: MIT
//
// The join between an object the synthetic importer published and the durable
// identity it was published under.
//
// The probe beside it is not allowed to find an object by its name — the whole
// point of part E is that the name and the identity are separate channels — and
// it is not allowed to find one by its position in the hierarchy, because
// positions move on purpose in these documents. So the importer writes the
// identity onto the object, and the probe reads it back.
//
// It lives outside `Assets/Editor` because Unity refuses to attach an editor
// script to a `GameObject`, and a probe that could not attach it could not
// join an object to the identity it was published under.
//
// This is fixture, not mechanism. It carries no identifier Unity uses: the
// local file identifier under test is the one `AddObjectToAsset` derived from
// the string in `identifier`, and this component only says which string that
// was.
using UnityEngine;

public sealed class FerriteSyntheticTag : MonoBehaviour
{
    public string identifier = string.Empty;
    public string definition_id = string.Empty;
    public string occurrence_id = string.Empty;
    public string kind = string.Empty;
    public string designation = string.Empty;
}
