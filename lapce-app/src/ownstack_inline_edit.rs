use std::rc::Rc;

use floem::{
    keyboard::Modifiers,
    peniko::kurbo::Rect,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use lapce_core::{command::FocusCommand, mode::Mode, selection::Selection};
use lapce_rpc::ownstack::OwnStackRpc;
use lapce_xi_rope::Rope;

use crate::{
    command::{CommandExecuted, CommandKind, LapceCommand},
    editor::EditorData,
    keypress::{KeyPressFocus, condition::Condition},
    main_split::Editors,
    window_tab::{CommonData, Focus},
};

#[derive(Clone, Debug)]
pub struct InlineEditData {
    pub active: RwSignal<bool>,
    pub editor: EditorData,
    pub offset: RwSignal<usize>,
    pub selection_text: RwSignal<String>,
    pub file_path: RwSignal<String>,
    pub layout_rect: RwSignal<Rect>,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for InlineEditData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::ModalFocus)
    }

    fn run_command(
        &self,
        command: &LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Scroll(_) => {}
            CommandKind::Focus(cmd) => {
                self.run_focus_command(cmd);
            }
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                self.editor.run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::Yes
    }

    fn receive_char(&self, c: &str) {
        self.editor.receive_char(c);
    }
}

impl InlineEditData {
    pub fn new(cx: Scope, editors: Editors, common: Rc<CommonData>) -> Self {
        let active = cx.create_rw_signal(false);
        let offset = cx.create_rw_signal(0);
        let selection_text = cx.create_rw_signal(String::new());
        let file_path = cx.create_rw_signal(String::new());
        let layout_rect = cx.create_rw_signal(Rect::ZERO);
        let editor = editors.make_local(cx, common.clone());
        Self {
            active,
            editor,
            offset,
            selection_text,
            file_path,
            layout_rect,
            common,
        }
    }

    pub fn start(
        &self,
        file_path: String,
        selected_text: String,
        cursor_offset: usize,
    ) {
        self.editor.doc().reload(Rope::from(""), true);
        self.editor.cursor().update(|cursor| {
            cursor.set_insert(Selection::caret(0));
        });
        self.file_path.set(file_path);
        self.selection_text.set(selected_text);
        self.offset.set(cursor_offset);
        self.active.set(true);
        self.common.focus.set(Focus::InlineEdit);
    }

    fn run_focus_command(&self, cmd: &FocusCommand) -> CommandExecuted {
        match cmd {
            FocusCommand::ModalClose => {
                self.cancel();
            }
            FocusCommand::ConfirmRename => {
                self.confirm();
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn cancel(&self) {
        self.active.set(false);
        if let Focus::InlineEdit = self.common.focus.get_untracked() {
            self.common.focus.set(Focus::Workbench);
        }
    }

    fn confirm(&self) {
        let instruction = self
            .editor
            .doc()
            .buffer
            .with_untracked(|buffer| buffer.to_string());
        let instruction = instruction.trim().to_string();
        if !instruction.is_empty() {
            let selected = self.selection_text.get_untracked();
            let file_path = self.file_path.get_untracked();

            let prompt = if selected.is_empty() {
                format!(
                    "[Inline Edit] File: {}\nInstruction: {}",
                    file_path, instruction
                )
            } else {
                format!(
                    "[Inline Edit] File: {}\nSelected code:\n```\n{}\n```\nInstruction: {}",
                    file_path, selected, instruction
                )
            };

            self.common.proxy.ownstack(OwnStackRpc::AiPrompt {
                prompt,
            });
        }
        self.cancel();
    }
}
