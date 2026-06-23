#[cfg(test)]
mod tests {
    use crate::completion::CompletionData;
    use crate::db::LapceDb;
    use crate::hover::HoverData;
    use crate::inline_completion::InlineCompletionData;
    use crate::keypress::KeyPressData;
    use crate::listener::Listener;
    use crate::ownstack_chat::{AgentMode, ChatRole, OwnStackChatData};
    use crate::terminal::event::{TermEvent, TermNotification};
    use crate::window_tab::{CommonData, Focus};
    use crate::workspace::LapceWorkspace;
    use floem::action::TimerToken;
    use floem::kurbo::{Point, Size};
    use floem::reactive::{Scope, SignalGet, SignalUpdate, provide_context};
    use lapce_rpc::ownstack::AgentModeState;
    use lapce_rpc::ownstack::OwnStackRpc;
    use lapce_rpc::proxy::ProxyRpcHandler;
    use lapce_rpc::terminal::TermId;
    use std::collections::BTreeMap;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::mpsc::channel;

    // Helper to setup a minimal test environment with a floem scope
    fn setup_test_data() -> (floem::reactive::Scope, OwnStackChatData) {
        let cx = Scope::new();

        // Mock DB
        let db = Arc::new(LapceDb::new().expect("failed to create LapceDb"));
        provide_context(db.clone());

        // Mock workspace
        let workspace = Arc::new(LapceWorkspace::default());

        // Mock signals
        let proxy_status = cx.create_rw_signal(None);
        let (term_tx, _) = channel::<(TermId, TermEvent)>();
        let (term_notification_tx, _) = channel::<TermNotification>();

        // Minimal mock of CommonData
        let config = crate::config::LapceConfig::default();
        let (config_read, _) = cx.create_signal(Arc::new(config.clone()));

        // Manually build WindowCommonData
        let window_common = Rc::new(crate::window::WindowCommonData {
            window_command: Listener::new_empty(cx),
            window_scale: cx.create_rw_signal(1.0),
            size: cx.create_rw_signal(Size::ZERO),
            num_window_tabs: cx.create_memo(|_| 1),
            window_maximized: cx.create_rw_signal(false),
            window_tab_header_height: cx.create_rw_signal(0.0),
            latest_release: cx.create_signal(Arc::new(None)).0,
            ime_allowed: cx.create_rw_signal(false),
            cursor_blink_timer: cx.create_rw_signal(TimerToken::INVALID),
            hide_cursor: cx.create_rw_signal(false),
            app_view_id: cx.create_rw_signal(floem::ViewId::new()),
            extra_plugin_paths: Arc::new(Vec::new()),
        });

        let common = CommonData {
            workspace: workspace.clone(),
            scope: cx,
            keypress: cx.create_rw_signal(KeyPressData::new(cx, &config)),
            focus: cx.create_rw_signal(Focus::Workbench),
            completion: cx.create_rw_signal(CompletionData::new(cx, config_read)),
            inline_completion: cx.create_rw_signal(InlineCompletionData::new(cx)),
            fim: crate::ownstack_fim::FimClientData::new(cx),
            hover: HoverData::new(cx),
            register: cx.create_rw_signal(Default::default()),
            find: crate::find::Find::new(cx),
            internal_command: Listener::new_empty(cx),
            lapce_command: Listener::new_empty(cx),
            workbench_command: Listener::new_empty(cx),
            term_tx,
            term_notification_tx,
            proxy: ProxyRpcHandler::new(),
            view_id: cx.create_rw_signal(floem::ViewId::new()),
            ui_line_height: cx.create_memo(|_| 15.0),
            dragging: cx.create_rw_signal(None),
            workbench_size: cx.create_rw_signal(Size::ZERO),
            config: config_read,
            proxy_status,
            mouse_hover_timer: cx.create_rw_signal(TimerToken::INVALID),
            window_origin: cx.create_rw_signal(Point::ZERO),
            breakpoints: cx.create_rw_signal(BTreeMap::new()),
            keyboard_focus: cx.create_rw_signal(None),
            window_common,
        };

        let chat_data = OwnStackChatData::new(common, db);
        chat_data.messages.set(Vec::new());
        chat_data.clear_history();
        (cx, chat_data)
    }

    #[test]
    fn test_chat_message_sending() {
        unsafe { std::env::set_var("OPENROUTER_API_KEY", "test-dummy-key") };
        let (_cx, chat_data) = setup_test_data();

        chat_data.bridge_connected.set(true);
        chat_data.input.set("Hello OwnStack".to_string());
        chat_data.send_message();

        let messages = chat_data.messages.get();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, ChatRole::User);
        assert_eq!(messages[0].content, "Hello OwnStack");
        assert!(chat_data.is_loading.get());
        assert_eq!(chat_data.input.get(), "");
    }

    #[test]
    fn test_streaming_updates() {
        let (_cx, chat_data) = setup_test_data();

        // Simulate start of stream
        chat_data.is_loading.set(true);

        // Receive chunks
        chat_data.receive_chunk(OwnStackRpc::AiStreamChunk {
            content_delta: Some("Hello".to_string()),
            tool_call_delta: None,
            finish_reason: None,
        });

        assert_eq!(chat_data.streaming_content.get(), "Hello");

        chat_data.receive_chunk(OwnStackRpc::AiStreamChunk {
            content_delta: Some(" world".to_string()),
            tool_call_delta: None,
            finish_reason: None,
        });

        assert_eq!(chat_data.streaming_content.get(), "Hello world");

        // Finalize stream
        chat_data.receive_chunk(OwnStackRpc::AiStreamChunk {
            content_delta: None,
            tool_call_delta: None,
            finish_reason: Some("stop".to_string()),
        });

        // Streaming content should be cleared, and message added to history
        assert_eq!(chat_data.streaming_content.get(), "");
        let messages = chat_data.messages.get();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, ChatRole::Assistant);
        assert_eq!(messages[0].content, "Hello world");
        assert!(!chat_data.is_loading.get());
    }

    #[test]
    fn test_mission_updates() {
        let (_cx, chat_data) = setup_test_data();

        let goal = "Test Goal".to_string();
        let steps = vec![
            ("Step 1".to_string(), "pending".to_string()),
            ("Step 2".to_string(), "active".to_string()),
        ];

        chat_data.receive_mission(goal.clone(), steps.clone());

        let mission = chat_data.current_mission.get().expect("mission set");
        assert_eq!(mission.0, goal);
        assert_eq!(mission.1.len(), 2);
        assert_eq!(mission.1[0].0, "Step 1");
    }

    #[test]
    fn test_agent_mode_runtime_updates() {
        let (_cx, chat_data) = setup_test_data();

        assert_eq!(chat_data.agent_mode.get(), AgentMode::Ask);
        chat_data.set_mode_from_runtime(AgentModeState::Auto);
        assert_eq!(chat_data.agent_mode.get(), AgentMode::Auto);
        chat_data.set_mode_from_runtime(AgentModeState::Plan);
        assert_eq!(chat_data.agent_mode.get(), AgentMode::Plan);
        chat_data.set_mode_from_runtime(AgentModeState::Ask);
        assert_eq!(chat_data.agent_mode.get(), AgentMode::Ask);
    }
}
