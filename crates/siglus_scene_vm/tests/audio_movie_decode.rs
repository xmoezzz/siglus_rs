use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use siglus_scene_vm::audio::bgm::{
    decode_ovk_entry_by_no_to_wav_bytes, resolve_koe_source, KoeSource,
};

fn project_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testcase")
        .canonicalize()
        .expect("canonical testcase dir")
}

#[test]
fn testcase_koe_number_resolves_to_ovk_line_no_and_decodes() -> Result<()> {
    let project = project_dir();
    let source = resolve_koe_source(&project, 206_300_076)?;
    let (path, entry_no) = match source {
        KoeSource::OvkEntryByNo { path, entry_no } => (path, entry_no),
        other => anyhow::bail!("expected OVK source, got {other:?}"),
    };

    assert_eq!(path.file_name().and_then(|s| s.to_str()), Some("z2063.ovk"));
    assert_eq!(entry_no, 76);

    let decoded = decode_ovk_entry_by_no_to_wav_bytes(&path, entry_no)
        .with_context(|| format!("decode testcase KOE: {}", path.display()))?;
    assert!(
        decoded.wav_bytes.starts_with(b"RIFF"),
        "decoded KOE must be WAV bytes"
    );
    Ok(())
}

#[test]
fn testcase_opening_mpeg_metadata_is_available() -> Result<()> {
    let project = project_dir();
    let mut movie = siglus_scene_vm::movie::MovieManager::new(project);
    let info = movie.prepare("op00")?;
    assert_eq!(info.width, Some(1280));
    assert_eq!(info.height, Some(720));
    Ok(())
}

#[test]
fn testcase_gameexe_mwnd_and_waku_templates_are_loaded() -> Result<()> {
    let project = project_dir();
    let mut unknown = siglus_scene_vm::runtime::unknown::UnknownOpRecorder::default();
    let tables = siglus_scene_vm::runtime::tables::AssetTables::load(&project, &mut unknown);
    let mwnd = tables
        .mwnd_templates
        .first()
        .context("missing MWND.000 template")?;
    assert_eq!(mwnd.window_pos, (0, 440));
    assert_eq!(mwnd.window_size, (1280, 280));
    assert_eq!(mwnd.message_pos, (240, 120));
    assert_eq!(mwnd.waku_no, 0);

    let waku = tables
        .waku_templates
        .first()
        .context("missing WAKU.000 template")?;
    assert_eq!(waku.waku_file, "mn_mw_mw00a00");
    assert_eq!(waku.filter_file, "mn_mw_mw00a01");
    Ok(())
}
