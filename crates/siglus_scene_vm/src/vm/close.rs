//! Discover a game's standalone exit action without relying on script names.

use super::*;
use crate::runtime::forms::codes;
use std::collections::{BTreeMap, BTreeSet};

fn word(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn pushed_int(stream: &SceneStream<'_>, pc: usize) -> Option<i32> {
    (stream.scn.get(pc) == Some(&CD_PUSH) && word(stream.scn, pc + 1) == Some(codes::FM_INT))
        .then(|| word(stream.scn, pc + 5))
        .flatten()
}

// Only accept a bounded, parameterless routine with a conditional confirmation
// path, a RETURN path, and a direct syscom.end_game call. Reject mixed system
// dispatchers (save/load/return-to-title/etc.) and branches outside the routine.
// This deliberately leaves unfamiliar or ambiguous scripts to the fallback UI.
fn is_close_routine(stream: &SceneStream<'_>, start: usize, end: usize, command: bool) -> bool {
    let inspect = || -> Option<bool> {
        if command {
            let mut pc = start;
            while stream.scn.get(pc) == Some(&CD_NL) {
                pc = stream.instruction_end(pc)?;
            }
            // Parameters are declared with DEC_PROP before ARG.
            if stream.scn.get(pc) != Some(&CD_ARG) {
                return Some(false);
            }
        }
        let mut pending = vec![start];
        let mut visited = BTreeSet::new();
        let mut exit = false;
        let mut returns = false;
        let mut conditional = false;
        while let Some(pc) = pending.pop() {
            if pc < start || pc >= end {
                return Some(false);
            }
            if !visited.insert(pc) {
                continue;
            }
            let opcode = *stream.scn.get(pc)?;
            let next = stream.instruction_end(pc)?;
            if next > end {
                return Some(false);
            }
            if opcode == CD_ELM_POINT {
                if let Some(root) = pushed_int(stream, next) {
                    if root == codes::ELM_GLOBAL_SYSCOM {
                        let op = pushed_int(stream, next + 9)?;
                        if op != codes::syscom_op::END_GAME {
                            return Some(false);
                        }
                        // Match a call, not merely a reference to the member.
                        let mut call_pc = next + 18;
                        while stream.scn.get(call_pc) == Some(&CD_PUSH) {
                            call_pc = stream.instruction_end(call_pc)?;
                        }
                        if stream.scn.get(call_pc) != Some(&CD_COMMAND)
                            || stream.instruction_end(call_pc)? > end
                        {
                            return Some(false);
                        }
                        exit = true;
                        // END_GAME transfers control to the host; the script
                        // need not put a RETURN after its final exit branch.
                        continue;
                    } else if matches!(root, codes::ELM_GLOBAL_RETURNMENU | codes::ELM_GLOBAL_JUMP)
                    {
                        return Some(false);
                    } else if root == codes::ELM_GLOBAL_EXCALL {
                        // Discovery runs without an allocated EXCALL menu. A
                        // cancel-menu callback can look like an exit action
                        // while still accessing that menu's buttons or locals.
                        // Only the allocation/call status queries are safe;
                        // indexed or computed access is not proven standalone.
                        let op = pushed_int(stream, next + 9)?;
                        if !matches!(op, codes::excall_op::OP_8 | codes::excall_op::OP_12) {
                            return Some(false);
                        }
                    }
                }
            }
            match opcode {
                CD_RETURN => {
                    returns = true;
                }
                CD_EOF | CD_TEXT => return Some(false),
                CD_GOTO | CD_GOTO_TRUE | CD_GOTO_FALSE | CD_GOSUB | CD_GOSUBSTR => {
                    let label = usize::try_from(word(stream.scn, pc + 1)?).ok()?;
                    let target =
                        usize::try_from(word(stream.label_list, label.checked_mul(4)?)?).ok()?;
                    pending.push(target);
                    if opcode != CD_GOTO {
                        pending.push(next);
                    }
                    conditional |= matches!(opcode, CD_GOTO_TRUE | CD_GOTO_FALSE);
                }
                _ => pending.push(next),
            }
        }
        Some(exit && returns && conditional)
    };
    inspect().unwrap_or(false)
}

#[derive(Clone, Debug)]
enum CloseEntry {
    Scene { scene: String, z: i32 },
    Command(ResolvedUserCommand),
    Local { offset: usize },
}

// Include every GOSUB target as a boundary, but only consider targets whose
// callers pass no arguments. Ordinary branch labels are not routine entries.
fn local_subroutines(stream: &SceneStream<'_>) -> Option<BTreeMap<usize, bool>> {
    let mut entries = BTreeMap::new();
    let mut pc = 0;
    while pc < stream.scn.len() {
        if matches!(stream.scn[pc], CD_GOSUB | CD_GOSUBSTR) {
            let label = usize::try_from(word(stream.scn, pc + 1)?).ok()?;
            let offset = usize::try_from(word(stream.label_list, label.checked_mul(4)?)?).ok()?;
            let parameterless = word(stream.scn, pc + 5)? == 0;
            entries
                .entry(offset)
                .and_modify(|value| *value &= parameterless)
                .or_insert(parameterless);
        }
        pc = stream.instruction_end(pc)?;
    }
    Some(entries)
}

impl SceneVm<'_> {
    /// Use a configured CLOSE_SCENE, or discover a unique standalone exit
    /// action among the game's cancel-menu labels, shared user commands, and
    /// the active title menu's local subroutines once its buttons are ready.
    /// The host must suspend/resume its wait using the SCRIPT proc requests.
    pub fn call_game_close_scene(&mut self) -> Result<bool> {
        if self.ctx.excall_state.ready || self.ctx.excall_state.ex_call_flag {
            return Ok(false);
        }
        if self.call_syscom_configured_scene("CLOSE_SCENE")? {
            self.ctx.excall_state.ex_call_flag = false;
            return Ok(true);
        }
        let Some(entry) = self.discover_close_entry()? else {
            return Ok(false);
        };
        log::debug!("game close action: {entry:?}");
        let opened = match entry {
            CloseEntry::Scene { scene, z } => self.call_syscom_scene(&scene, z).map(|()| true),
            CloseEntry::Command(command) => {
                self.enter_resolved_user_command(&command, self.cfg.fm_void, &[], true, false)
            }
            CloseEntry::Local { offset } => self.enter_current_scene_user_cmd_proc_at_offset(
                offset,
                self.cfg.fm_void,
                &[],
                true,
                false,
            ),
        }?;
        // The host needs a separate SCRIPT proc to suspend/resume its wait,
        // but these actions normally run on the game's regular stage. Setting
        // EXCALL's input flag without allocated EXCALL objects disables all
        // normal-stage buttons, including the confirmation's Yes/No buttons.
        if opened {
            self.ctx.excall_state.ex_call_flag = false;
        }
        Ok(opened)
    }

    fn discover_close_entry(&mut self) -> Result<Option<CloseEntry>> {
        self.ensure_scene_pck_cache()?;
        let pck = self.scene_pck_cache.as_ref().unwrap();
        let mut entries: BTreeMap<usize, BTreeMap<usize, CloseEntry>> = BTreeMap::new();
        for (index, target) in pck.inc_cmds.iter().enumerate() {
            if target.scn_no < 0 || target.offset < 0 {
                continue;
            }
            entries.entry(target.scn_no as usize).or_default().insert(
                target.offset as usize,
                CloseEntry::Command(ResolvedUserCommand {
                    encoded_no: index,
                    name: pck
                        .inc_cmd_name_map
                        .get(&(index as u32))
                        .cloned()
                        .unwrap_or_default(),
                    target_scene_no: target.scn_no as usize,
                    target_offset: target.offset as usize,
                    include_command: true,
                }),
            );
        }
        let cancel_scene = self
            .ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_entry("CANCEL_SCENE"))
            .and_then(|entry| entry.item_unquoted(0))
            .map(str::to_string);
        if let Some(scene) = cancel_scene {
            if let Some(scene_no) = Self::find_scene_no_by_name(pck, &scene) {
                let stream = self.cached_scene_stream(scene_no)?;
                for (z, offset) in stream.z_label_list.chunks_exact(4).enumerate() {
                    let offset = i32::from_le_bytes(offset.try_into().unwrap());
                    if offset < 0 {
                        continue;
                    }
                    entries
                        .entry(scene_no)
                        .or_default()
                        .entry(offset as usize)
                        .or_insert_with(|| CloseEntry::Scene {
                            scene: scene.clone(),
                            z: z as i32,
                        });
                }
            }
        }
        // A title button's local callback may use title-owned objects. Never
        // borrow it from another scene, or during title initialization.
        let active_title = self.ctx.wait.button_selection_waiting()
            && self
                .ctx
                .tables
                .gameexe
                .as_ref()
                .and_then(|cfg| cfg.get_entry("MENU_SCENE"))
                .and_then(|entry| entry.item_unquoted(0))
                .zip(self.current_scene_name())
                .is_some_and(|(menu, current)| siglus_name_eq(menu, current));
        let local_entries = if active_title {
            local_subroutines(&self.stream).unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        if let Some(scene_no) = self.current_scene_no {
            for (&offset, &parameterless) in &local_entries {
                if parameterless {
                    entries
                        .entry(scene_no)
                        .or_default()
                        .entry(offset)
                        .or_insert(CloseEntry::Local { offset });
                }
            }
        }
        let mut found = None;
        for (scene_no, candidates) in entries {
            let stream = self.cached_scene_stream(scene_no)?;
            let mut boundaries: BTreeSet<usize> = candidates.keys().copied().collect();
            boundaries.insert(stream.scn.len());
            if self.current_scene_no == Some(scene_no) {
                boundaries.extend(local_entries.keys().copied());
            }
            for z in stream.z_label_list.chunks_exact(4) {
                if let Ok(offset) = usize::try_from(i32::from_le_bytes(z.try_into().unwrap())) {
                    boundaries.insert(offset);
                }
            }
            for cmd in 0..stream.header.scn_cmd_cnt.max(0) as usize {
                if let Ok(offset) = stream.scn_cmd_offset(cmd) {
                    boundaries.insert(offset);
                }
            }
            for (start, entry) in candidates {
                let Some(&end) = boundaries
                    .range((std::ops::Bound::Excluded(start), std::ops::Bound::Unbounded))
                    .next()
                else {
                    continue;
                };
                if is_close_routine(&stream, start, end, matches!(entry, CloseEntry::Command(_))) {
                    if found.is_some() {
                        return Ok(None);
                    }
                    found = Some(entry);
                }
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn push_int(code: &mut Vec<u8>, value: i32) {
        code.push(CD_PUSH);
        code.extend(codes::FM_INT.to_le_bytes());
        code.extend(value.to_le_bytes());
    }

    fn conditional_exit(op: i32, parameters: bool, branch_outside: bool) -> SceneStream<'static> {
        conditional_exit_with_prefix(op, parameters, branch_outside, &[])
    }

    fn conditional_exit_with_prefix(
        op: i32,
        parameters: bool,
        branch_outside: bool,
        prefix: &[u8],
    ) -> SceneStream<'static> {
        let mut code = Vec::new();
        if parameters {
            code.push(CD_DEC_PROP);
            code.extend(codes::FM_INT.to_le_bytes());
            code.extend(0i32.to_le_bytes());
        }
        code.push(CD_ARG);
        code.extend(prefix);
        push_int(&mut code, 1);
        code.push(CD_GOTO_FALSE);
        code.extend(0i32.to_le_bytes());
        code.push(CD_ELM_POINT);
        push_int(&mut code, codes::ELM_GLOBAL_SYSCOM);
        push_int(&mut code, op);
        push_int(&mut code, 0);
        code.push(CD_COMMAND);
        for value in [0i32, 1, codes::FM_INT, 0, codes::FM_VOID] {
            code.extend(value.to_le_bytes());
        }
        let return_pc = code.len();
        code.push(CD_RETURN);
        code.extend(0i32.to_le_bytes());
        let mut header = [0i32; 33];
        for index in (1..33).step_by(2).chain(std::iter::once(0)) {
            header[index] = 132;
        }
        header[2] = code.len() as i32;
        header[7] = 132 + header[2];
        header[8] = 1;
        let mut chunk: Vec<u8> = header.into_iter().flat_map(i32::to_le_bytes).collect();
        chunk.extend(code);
        chunk.extend(
            (if branch_outside {
                header[2] + 10
            } else {
                return_pc as i32
            })
            .to_le_bytes(),
        );
        SceneStream::new(Box::leak(chunk.into_boxed_slice())).unwrap()
    }

    #[test]
    fn accepts_conditional_parameterless_exit() {
        let stream = conditional_exit(codes::syscom_op::END_GAME, false, false);
        assert!(is_close_routine(&stream, 0, stream.scn.len(), true));
    }

    #[test]
    fn rejects_parameterized_mixed_or_out_of_bounds_actions() {
        for (op, parameters, outside) in [
            (codes::syscom_op::END_GAME, true, false),
            (codes::syscom_op::RETURN_TO_MENU, false, false),
            (codes::syscom_op::END_GAME, false, true),
        ] {
            let stream = conditional_exit(op, parameters, outside);
            assert!(!is_close_routine(&stream, 0, stream.scn.len(), true));
        }
    }

    #[test]
    fn rejects_exit_callbacks_that_require_excall_storage() {
        for chain in [
            vec![65, 1, 6, -1, 1, 5],        // excall.front.objbtngroup[1].end()
            vec![65, -1, 1, 1, 6, -1, 1, 5], // explicit excall[1]
            vec![65, 0, -1, 0],              // excall's local flags
            vec![65, 4], // callbacks that allocate their own menu are not standalone
        ] {
            let mut prefix = vec![CD_ELM_POINT];
            for value in chain {
                push_int(&mut prefix, value);
            }
            prefix.push(CD_COMMAND);
            for value in [0i32, 0, 0, codes::FM_VOID] {
                prefix.extend(value.to_le_bytes());
            }
            let stream =
                conditional_exit_with_prefix(codes::syscom_op::END_GAME, false, false, &prefix);
            assert!(!is_close_routine(&stream, 0, stream.scn.len(), true));
        }
    }

    #[test]
    fn accepts_excall_status_queries_without_storage() {
        for op in [codes::excall_op::OP_8, codes::excall_op::OP_12] {
            let mut prefix = vec![CD_ELM_POINT];
            push_int(&mut prefix, codes::ELM_GLOBAL_EXCALL);
            push_int(&mut prefix, op);
            prefix.push(CD_COMMAND);
            for value in [0i32, 0, 0, codes::FM_INT] {
                prefix.extend(value.to_le_bytes());
            }
            let stream =
                conditional_exit_with_prefix(codes::syscom_op::END_GAME, false, false, &prefix);
            assert!(is_close_routine(&stream, 0, stream.scn.len(), true));
        }
    }

    #[test]
    #[ignore = "requires SIGLUS_CLOSE_TEST_PROJECT with game assets"]
    fn discovers_game_exit_action() {
        let project = PathBuf::from(std::env::var("SIGLUS_CLOSE_TEST_PROJECT").unwrap());
        let path = crate::resource::find_scene_pck_path(&project).unwrap();
        let options = crate::resource::load_scene_pck_decode_options(&project).unwrap();
        let pck = ScenePck::load_lazy(&path, &options).unwrap();
        let ctx = CommandContext::new(project);
        let menu = ctx
            .tables
            .gameexe
            .as_ref()
            .unwrap()
            .get_entry("MENU_SCENE")
            .unwrap()
            .item_unquoted(0)
            .unwrap()
            .to_string();
        let scene_no = SceneVm::find_scene_no_by_name(&pck, &menu).unwrap();
        let (owner, range) = pck.scn_data_shared(scene_no).unwrap();
        let stream =
            SceneStream::new_shared_range_with_string_codec(owner, range, pck.string_codec)
                .unwrap();
        let mut vm = SceneVm::new(stream, ctx);
        vm.install_initial_scene_pck(pck, String::new());
        vm.current_scene_no = Some(scene_no);
        vm.current_scene_name = Some(menu);
        vm.ctx.wait.wait_selbtn();
        let entry = vm.discover_close_entry().unwrap();
        eprintln!("discovered close entry: {entry:?}");
        assert!(entry.is_some());
        assert!(vm.call_game_close_scene().unwrap());
        assert!(vm.take_script_proc_request());
        // A cancelled game-owned action returns without requesting EndGame.
        vm.exec_return(vec![]).unwrap();
        assert!(vm.take_script_proc_pop_request());
        assert!(vm.ctx.globals.syscom.pending_proc.is_none());
    }
}
