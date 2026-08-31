# Use with -DCMAKE_TOOLCHAIN_FILE to make find_package(Jet) available during
# the first configure pass. The host C/C++ compiler remains CMake's choice.

get_filename_component(_jet_toolchain_dir "${CMAKE_CURRENT_LIST_FILE}" DIRECTORY)
set(Jet_ROOT "${_jet_toolchain_dir}" CACHE PATH "Jet host adapter directory")
list(PREPEND CMAKE_MODULE_PATH "${Jet_ROOT}")
