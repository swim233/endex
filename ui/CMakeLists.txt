cmake_minimum_required(VERSION 3.24)
project(endex-ui LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS DBus Gui Network Quick QuickControls2 Test Widgets)
find_package(LayerShellQt REQUIRED)
find_package(KF6KIO REQUIRED)
find_package(KF6WindowSystem REQUIRED)
qt_standard_project_setup(REQUIRES 6.8)

qt_add_executable(endex-ui src/main.cpp)
qt_add_qml_module(endex-ui
    URI Endex
    VERSION 1.0
    # Dynamic build: QML imports load at run time, nothing to link statically.
    NO_IMPORT_SCAN
    SOURCES
        src/daemon.cpp src/daemon.h
        src/evalrun.cpp src/evalrun.h
        src/navigation.cpp src/navigation.h
        src/popup.cpp src/popup.h
        src/results.cpp src/results.h
        src/settings.cpp src/settings.h
        src/settingswindow.cpp src/settingswindow.h
        src/uidebug.cpp src/uidebug.h
    QML_FILES
        qml/Main.qml
        qml/ResultList.qml
        qml/DocRow.qml
        qml/ImageCell.qml
        qml/Preview.qml
        qml/Relevance.qml
        qml/StatusLine.qml
        qml/StateView.qml
        qml/KeyCap.qml
        qml/Banner.qml
        qml/CommandBox.qml
        qml/settings/SettingsWindow.qml
        qml/settings/Card.qml
        qml/settings/Hint.qml
        qml/settings/Markup.js
        qml/settings/Notices.qml
        qml/settings/Sidebar.qml
        qml/settings/SettingChoice.qml
        qml/settings/SettingCombo.qml
        qml/settings/SettingSpin.qml
        qml/settings/SettingSwitch.qml
        qml/settings/PageDirs.qml
        qml/settings/PageExclude.qml
        qml/settings/PageWatch.qml
        qml/settings/PageModel.qml
        qml/settings/PageStatus.qml
        qml/settings/PageShortcut.qml
        qml/settings/PageResource.qml
        qml/settings/PageEval.qml
)
target_include_directories(endex-ui PRIVATE src)
target_link_libraries(endex-ui PRIVATE
    Qt6::DBus Qt6::Gui Qt6::Network Qt6::Quick Qt6::QuickControls2 Qt6::Widgets
    LayerShellQt::Interface KF6::KIOGui KF6::WindowSystem
)

enable_testing()
qt_add_executable(navigation_test tests/navigation_test.cpp src/navigation.cpp)
target_include_directories(navigation_test PRIVATE src)
target_link_libraries(navigation_test PRIVATE Qt6::Test)
add_test(NAME navigation COMMAND navigation_test)
