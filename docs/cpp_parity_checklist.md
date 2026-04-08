Siglus the original implementation Parity Checklist (Rust)

Legend
- DONE: Implemented and wired.
- PARTIAL: Implemented but behavior differs or is incomplete.
- TODO: Not implemented.
- EXCLUDED: Explicitly excluded (emote).

Scope
- Target: Match Siglus the original implementation behavior except tnm id mapping and emote.
- Note: Syscall definitions are not public in the the original implementation tree; current Rust syscall handling is no-op with logging.

Core Runtime
- Scene VM core (flow_script/flow_proc): DONE
- Numeric form dispatch (cmd_* forms): DONE (non-tnm ids still needed)
- Syscalls: DONE (no-op handler + default return values)
- Unknown op recording: DONE

Assets and Data
- Scene.pck decode (tnm_lexer, eng_scene): DONE
- Gameexe.dat decode (tnm_ini, eng_init): DONE
- CGTABLE/DBS/ThumbTable load (tnm_cg_table/tnm_database/tnm_thumb_table): PARTIAL (load supported, some paths/flags missing)
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
- Network (tnm_net): TODO

Excluded
- Emote (eng_emote): EXCLUDED

Notes
- This checklist is a living document. As we map more the original implementation files to Rust modules, each section will be refined.
