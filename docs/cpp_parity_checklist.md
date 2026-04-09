Siglus the original implementation Parity Checklist (Rust)

Legend
- DONE: Implemented and wired.
- PARTIAL: Implemented but behavior differs or is incomplete.
- TODO: Not implemented.
- EXCLUDED: Explicitly excluded (emote).
- REMOVED: Rust-only compatibility concept removed because it is not treated as an original-engine subsystem.

Scope
- Target: Match Siglus the original implementation behavior except tnm id mapping and emote.
- Note: the old Rust-only `syscall` compatibility layer has been removed; runtime dispatch is treated as numeric form/code dispatch only.

Core Runtime
- Scene VM core (flow_script/flow_proc): DONE
- Numeric form dispatch (cmd_* forms): DONE (non-tnm ids still needed)
- Syscalls: REMOVED (not treated as an original-engine subsystem anymore)
- Unknown op recording: DONE
- Stage/Object/Screen fallback edge paths: DONE (unhandled chains now record unknowns instead of auto-learning/runtime property bags)

Assets and Data
- Scene.pck decode (tnm_lexer, eng_scene): DONE
- Gameexe.dat decode (tnm_ini, eng_init): DONE
- CGTABLE/DBS/ThumbTable load (tnm_cg_table/tnm_database/tnm_thumb_table): DONE (runtime load + flags + DB queries + thumb save/load wiring)
- G00/BG image decode (eng_graphics/eng_g00_buf): DONE
- GAN (animation) load (tnm_gan/tnm_gan_data): DONE

Rendering
- Sprite/layer composition (eng_disp/eng_graphics): DONE
- Wipe/mask effects (cmd_wipe, eng_disp_wipe, eng_mask_*): DONE
- Screen filters (filter color/mask): DONE
- Text rendering (ifc_font/elm_object_string): PARTIAL (fontdue-based; basic word wrap/line metrics)
- Capture buffer (eng_disp_capture/eng_syscom_capture): DONE

Audio
- BGM/SE/PCM playback (elm_sound_* / ifc_sound): DONE
- KOE/voice (elm_sound_koe): DONE (mapped via PCM)
- Volume/mute/seek/fade (cmd_sound, eng_system sound): DONE
- Movie audio (mov + audio): DONE

Movie
- OMV/Theora video decode (elm_mov / eng_disp): DONE
  - 2026-04-09: moved decoder into `/Users/xmoe/Documents/siglus_rs-main/crates/siglus_omv_decoder`
  - 2026-04-09: validated on real testcase OMV assets (`ny_mv_lucia12aw.omv`, `mn_tt_rpa_sz00.omv`, `ef_ch_sks_mh00.omv`)
  - 2026-04-09: `cargo test -p siglus_scene_vm --test omv_decode` passed (4 tests)
- MPEG2 (na_mpeg2_decoder): DONE

UI and System Windows
- Config dialogs (cfg_wnd_*): DONE (in-engine overlay UI)
- Save/Load dialogs (sys_wnd_*): DONE (menu overlay + numeric slot select)
- Debug/info windows (db_wnd_info_*): TODO

Input and UI Logic
- Input polling (cmd_input / tnm_input): DONE (winit input)
- Message window/mwnd (cmd_mwnd, elm_mwnd_*): DONE
- Editbox (elm_editbox): DONE
- Button/select (elm_btn_sel*, elm_object_btn): DONE

Save/Load
- Save slot metadata (eng_save, tnm_save): DONE
- Full script/runtime state serialization: TODO
- Quick save/end save flags: DONE

System/Platform
- Window mode/size (eng_system): DONE
- Steam (eng_steam): EXCLUDED
- Twitter (eng_twitter): EXCLUDED
- Network (tnm_net): DONE (Rust utility layer added; public original visible class is an empty wrapper)

Excluded
- Emote (eng_emote): EXCLUDED

Notes
- This checklist is a living document. As we map more the original implementation files to Rust modules, each section will be refined.
