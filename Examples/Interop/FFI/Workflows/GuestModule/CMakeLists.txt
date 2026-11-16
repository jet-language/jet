cmake_minimum_required(VERSION 3.20)
project(jet_guest_rules LANGUAGES C CXX)

# The module is a guest replacement inside this existing foreign build.  The
# host compiler and native game remain CMake-owned; Jet only supplies rules_jet.
list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_LIST_DIR}/../../../../../Tools/foreign-build-hosts/cmake")
find_package(Jet REQUIRED)

jet_library(rules_jet
    ENTRY library.jet
    OUTPUT core
    LIBRARY rules
    LOADABLE
)

add_executable(game Native/Game.cpp Native/RulesBridge.cpp)
target_link_libraries(game PRIVATE rules_jet)
target_include_directories(game PRIVATE Native "${CMAKE_CURRENT_SOURCE_DIR}")

# Keep an unchanged native baseline to demonstrate that the caller's source
# contract is not rewritten by the guest route.
add_executable(game_native Native/Game.cpp Native/Rules.cpp)
target_include_directories(game_native PRIVATE Native)
