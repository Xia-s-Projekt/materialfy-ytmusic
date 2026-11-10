#pragma once

namespace mf {

// Create the singletons and register the Materialfy.Core 1.0 types. Call after QApplication and
// crate::context::init(), before creating the QQmlApplicationEngine.
void register_qml_types();

} // namespace mf
