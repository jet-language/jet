# Direct C/C++ driver fixture. The compiler paths are deliberately mandatory:
# Make must receive absolute jet-cc and jet-c++ paths from the caller.
PROJECT_ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BUILD_ROOT ?= $(PROJECT_ROOT)/build

ifeq ($(origin CC),command line)
else
$(error CC must be supplied as an absolute path to jet-cc)
endif
ifeq ($(origin CXX),command line)
else
$(error CXX must be supplied as an absolute path to jet-c++)
endif
ifeq ($(filter /%,$(CC)),)
$(error CC must be an absolute path to jet-cc)
endif
ifeq ($(filter /%,$(CXX)),)
$(error CXX must be an absolute path to jet-c++)
endif

JET_FLAGS := --project-root="$(PROJECT_ROOT)" --build-root="$(BUILD_ROOT)"
C_OBJECT := $(BUILD_ROOT)/main.o
CXX_OBJECT := $(BUILD_ROOT)/main-cxx.o
C_BINARY := $(BUILD_ROOT)/cc-driver
CXX_BINARY := $(BUILD_ROOT)/cxx-driver
C_DEPFILE := $(BUILD_ROOT)/main.d
CXX_DEPFILE := $(BUILD_ROOT)/main-cxx.d

.PHONY: all clean

all: $(C_BINARY) $(CXX_BINARY)

$(BUILD_ROOT):
	mkdir -p "$@"

$(C_OBJECT): main.c include/config.h flags.rsp | $(BUILD_ROOT)
	"$(CC)" $(JET_FLAGS) @flags.rsp -std=c11 -I include -MMD -MP -MF "$(C_DEPFILE)" -MT "$(C_OBJECT)" -c "$<" -o "$@"

$(CXX_OBJECT): main.cpp include/config.h flags.rsp | $(BUILD_ROOT)
	"$(CXX)" $(JET_FLAGS) @flags.rsp -std=c++17 -I include -MMD -MP -MF "$(CXX_DEPFILE)" -MT "$(CXX_OBJECT)" -c "$<" -o "$@"

$(C_BINARY): $(C_OBJECT) | $(BUILD_ROOT)
	"$(CC)" $(JET_FLAGS) "$<" -o "$@"

$(CXX_BINARY): $(CXX_OBJECT) | $(BUILD_ROOT)
	"$(CXX)" $(JET_FLAGS) "$<" -o "$@"

-include $(C_DEPFILE) $(CXX_DEPFILE)

clean:
	rm -rf "$(BUILD_ROOT)"
