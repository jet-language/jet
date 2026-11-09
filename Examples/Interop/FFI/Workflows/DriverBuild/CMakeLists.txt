cmake_minimum_required(VERSION 3.20)
project(jet_driver_full_build LANGUAGES C CXX)

add_library(driver_support STATIC Native/Support.cpp)
add_executable(driver_app Native/Main.c)
target_link_libraries(driver_app PRIVATE driver_support)

# This action is intentionally opaque to the Jet compiler.  The imported plan
# must retain its inner owner and declare its input/output instead of claiming
# that a compilation database proves the complete graph.
add_custom_command(
    OUTPUT generated/native-action.txt
    COMMAND sh "${CMAKE_CURRENT_SOURCE_DIR}/Unsupported/NativeAction.sh" "${CMAKE_CURRENT_BINARY_DIR}/generated/native-action.txt"
    DEPENDS Unsupported/NativeAction.sh
    VERBATIM
)
add_custom_target(native_action ALL DEPENDS generated/native-action.txt)
add_dependencies(driver_app native_action)
