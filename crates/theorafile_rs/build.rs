use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor_root = manifest_dir.join("../../omvdecoder/src/theorafile");
    let lib_root = vendor_root.join("lib");

    let mut build = cc::Build::new();
    build
        .include(&vendor_root)
        .include(&lib_root)
        .include(lib_root.join("ogg"))
        .include(lib_root.join("vorbis"))
        .include(lib_root.join("theora"))
        .file(vendor_root.join("theorafile.c"))
        .file(manifest_dir.join("src/wrapper.c"));

    // libogg
    build.file(lib_root.join("ogg/bitwise.c"));
    build.file(lib_root.join("ogg/framing.c"));

    // libvorbis (decoder subset, full list used by theorafile)
    let vorbis_files = [
        "analysis.c",
        "bitrate.c",
        "block.c",
        "codebook.c",
        "envelope.c",
        "floor0.c",
        "floor1.c",
        "lpc.c",
        "lsp.c",
        "lookup.c",
        "mapping0.c",
        "mdct.c",
        "psy.c",
        "registry.c",
        "res0.c",
        "sharedbook.c",
        "smallft.c",
        "synthesis.c",
        "window.c",
        "vinfo.c",
    ];
    for f in vorbis_files {
        build.file(lib_root.join("vorbis").join(f));
    }

    // libtheora (generic C sources; skip arch-specific SIMD)
    let theora_files = [
        "apiwrapper.c",
        "bitpack.c",
        "decapiwrapper.c",
        "decinfo.c",
        "decode.c",
        "dequant.c",
        "fragment.c",
        "huffdec.c",
        "idct.c",
        "internal.c",
        "quant.c",
        "state.c",
        "tinfo.c",
    ];
    for f in theora_files {
        build.file(lib_root.join("theora").join(f));
    }

    build.compile("theorafile");
}
