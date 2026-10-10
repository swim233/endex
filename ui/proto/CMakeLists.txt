cmake_minimum_required(VERSION 3.24)
project(endex-ui-proto LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS Gui Quick DBus)
find_package(LayerShellQt REQUIRED)
qt_standard_project_setup(REQUIRES 6.8)

qt_add_executable(endex-ui-proto main.cpp)
qt_add_qml_module(endex-ui-proto
    URI EndexProto
    VERSION 1.0
    QML_FILES Main.qml
)
target_link_libraries(endex-ui-proto PRIVATE Qt6::Gui Qt6::Quick Qt6::DBus LayerShellQt::Interface)
