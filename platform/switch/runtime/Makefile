DEVKITPRO ?= /opt/devkitpro
DEVKITA64 ?= $(DEVKITPRO)/devkitA64

TARGET := siglus_switch
BUILD := build
ROMFS ?= romfs
CC := $(DEVKITA64)/bin/aarch64-none-elf-gcc
ELF2NRO := $(DEVKITPRO)/tools/bin/elf2nro
NACPTOOL := $(DEVKITPRO)/tools/bin/nacptool
UAM := $(DEVKITPRO)/tools/bin/uam

CPPFLAGS := -I$(DEVKITPRO)/libnx/include
CFLAGS := -std=gnu17 -O2 -g -Wall -Wextra -ffunction-sections -fdata-sections \
	-march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE $(CPPFLAGS)
LDFLAGS := -specs=$(DEVKITPRO)/libnx/switch.specs -g \
	-march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE \
	-Wl,--gc-sections -Wl,-Map,$(TARGET).map \
	-Wl,--wrap=nvMapCreate -Wl,--wrap=nvAddressSpaceMap -L$(DEVKITPRO)/libnx/lib
LDLIBS := -ldeko3dd -lnx -lm
RUST_LIB := ../../../target/aarch64-switch/release/libsiglus_scene_vm.a
# Crates patched for the pinned nightly (see platform/switch/patches/README.md).
# Opt-in and scoped to this build: nothing in the current NRO depends on
# unwinding, and an unused patch only produces a Cargo warning.
# Use `make UNWINDING_PATCH=1` when adding a Rust dependency that needs it (nx).
#
# libc is always patched: std's `struct stat` and `struct dirent` for
# aarch64 Horizon must match devkitA64's newlib, or every metadata call
# (is_dir, file sizes) and directory listing reads the wrong fields. It is
# fetched from crates.io, checked and patched under $(BUILD).
LIBC_VERSION := 0.2.178
LIBC_SHA256 := 37c93d8daa9d8a012fd8ab92f088405fb202ea0b6ab73ee2482ae66af4f42091
LIBC_PATCH := ../patches/libc-$(LIBC_VERSION)-horizon-aarch64-stat.patch
LIBC_DIR := $(BUILD)/libc-$(LIBC_VERSION)
RUST_PATCHES := --config 'patch.crates-io.libc.path="platform/switch/runtime/$(LIBC_DIR)"'
ifeq ($(UNWINDING_PATCH),1)
RUST_PATCHES += --config 'patch.crates-io.unwinding.path="platform/switch/patches/unwinding-0.2.10"'
endif

OBJECTS := $(BUILD)/main.o $(BUILD)/gpu.o
# The desktop renderer's WGSL, translated to GLSL by ../shaderc (naga) and
# compiled by uam: the Switch draws with the desktop's shaders.
WGSL_DIR := ../../../crates/siglus_scene_vm/src/render/shaders
GLSL_DIR := $(BUILD)/glsl
SHADER_NAMES := sprite mesh shadow wipe page emote
SHADERS := $(foreach n,$(SHADER_NAMES),$(ROMFS)/shaders/$(n)_vsh.dksh $(ROMFS)/shaders/$(n)_fsh.dksh)

# `rust` always runs cargo (incremental): the library depends on the whole
# engine source, which make cannot see, so the ELF is always relinked.
.PHONY: all clean rust
all: $(TARGET).nro

rust: $(LIBC_DIR)/.patched
	cd ../../.. && RUSTFLAGS='--cfg getrandom_backend="custom"' rustup run nightly-2026-01-20 cargo build $(RUST_PATCHES) -p siglus_scene_vm --lib --release --target platform/switch/rust/aarch64-switch.json -Zbuild-std=std,panic_abort

$(BUILD) $(ROMFS)/shaders:
	mkdir -p $@

$(LIBC_DIR)/.patched: $(LIBC_PATCH) | $(BUILD)
	rm -rf $(LIBC_DIR) $(BUILD)/libc.crate
	curl -sSfL -o $(BUILD)/libc.crate https://static.crates.io/crates/libc/libc-$(LIBC_VERSION).crate
	echo "$(LIBC_SHA256)  $(BUILD)/libc.crate" | sha256sum -c --quiet -
	tar -xzf $(BUILD)/libc.crate -C $(BUILD)
	cd $(LIBC_DIR) && patch -s -p1 < ../../$(LIBC_PATCH)
	rm $(BUILD)/libc.crate
	touch $@

$(BUILD)/%.o: source/%.c | $(BUILD)
	$(CC) $(CFLAGS) -MMD -MP -c $< -o $@

$(GLSL_DIR)/.stamp: $(wildcard $(WGSL_DIR)/*.wgsl) ../shaderc/src/main.rs | $(BUILD)
	mkdir -p $(GLSL_DIR)
	cargo run --release --manifest-path ../shaderc/Cargo.toml -- $(WGSL_DIR) $(GLSL_DIR)
	touch $@

$(ROMFS)/shaders/%_vsh.dksh: $(GLSL_DIR)/.stamp | $(ROMFS)/shaders
	$(UAM) --stage=vert --out=$@ $(GLSL_DIR)/$*_vsh.glsl

$(ROMFS)/shaders/%_fsh.dksh: $(GLSL_DIR)/.stamp | $(ROMFS)/shaders
	$(UAM) --stage=frag --out=$@ $(GLSL_DIR)/$*_fsh.glsl

$(TARGET).elf: $(OBJECTS) rust
	$(CC) $(LDFLAGS) $(OBJECTS) $(RUST_LIB) $(LDLIBS) -o $@

$(TARGET).nacp:
	$(NACPTOOL) --create "Siglus Switch" "siglus_rs" "0.1.0" $@ --titleid=0100A11C5EED0001

$(TARGET).nro: $(TARGET).elf $(TARGET).nacp $(SHADERS)
	$(ELF2NRO) $< $@ --nacp=$(TARGET).nacp --romfsdir=$(ROMFS)

clean:
	rm -rf $(BUILD) $(ROMFS) $(TARGET).elf $(TARGET).map $(TARGET).nacp $(TARGET).nro

-include $(OBJECTS:.o=.d)
