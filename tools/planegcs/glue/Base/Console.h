/* SPDX-License-Identifier: MIT
 *
 * FerriteCAD's build glue, not planegcs.
 *
 * Enough of FreeCAD's console for planegcs to build outside FreeCAD. It uses
 * two printf-style calls; both are silent here, because writing to a terminal
 * inside a timed region would measure the terminal.
 */
#pragma once

namespace Base {
class ConsoleSingleton {
public:
    void Log(const char*, ...) {}
    void Warning(const char*, ...) {}
};
inline ConsoleSingleton& Console() {
    static ConsoleSingleton instance;
    return instance;
}
}  // namespace Base
