use floem::prelude::*;
use floem::reactive::RwSignal;
use floem::{
    peniko::Color,
    style::{CursorStyle, Style},
    text::Weight,
    views::{dyn_stack, h_stack, label, scroll, v_stack},
    View,
};
use std::rc::Rc;

use crate::{
    app::clickable_icon,
    config::icon::LapceIcons,
    window_tab::{CommonData, WindowTabData},
};

#[derive(Clone)]
pub struct McpServerInfo {
    pub name: String,
    pub status: String, // "Active", "Connecting", "Error"
    pub config_path: String,
}

#[derive(Clone)]
pub struct OwnStackMcpData {
    pub common: Rc<CommonData>,
    pub servers: RwSignal<Vec<McpServerInfo>>,
}

impl OwnStackMcpData {
    pub fn new(common: Rc<CommonData>) -> Self {
        // Initializing with mock data based on the HTML demo
        let cx = common.scope;
        let servers = cx.create_rw_signal(vec![
            McpServerInfo {
                name: "Docker Executor".into(),
                status: "Active".into(),
                config_path: "/.ownstack/mcp.json".into(),
            },
            McpServerInfo {
                name: "GitHub API".into(),
                status: "Active".into(),
                config_path: "Global Config".into(),
            },
            McpServerInfo {
                name: "Postgres DB".into(),
                status: "Error".into(),
                config_path: "/.ownstack/mcp.json".into(),
            },
        ]);

        Self { common, servers }
    }
}

pub fn mcp_panel(window_tab_data: Rc<WindowTabData>, _position: crate::panel::position::PanelPosition) -> impl View {
    let mcp_data = window_tab_data.ownstack_mcp.clone();
    let config = window_tab_data.common.config.clone();

    let header = h_stack((
        h_stack((
            label(|| "MCP SERVERS".to_string()).style(|s| {
                s.font_size(10.0)
                    .font_weight(Weight::BOLD)
                    .color(Color::from_rgb8(156, 163, 175))
            }),
        ))
        .style(|s| s.items_center()),
        h_stack((
            clickable_icon(
                || LapceIcons::ADD,
                || {},
                || false,
                || false,
                || "Add MCP Server",
                config,
            ),
            clickable_icon(
                || LapceIcons::SETTINGS,
                || {},
                || false,
                || false,
                || "MCP Settings",
                config,
            ),
        ))
        .style(|s| s.items_center().gap(2.0)),
    ))
    .style(|s| {
        s.width_full()
            .justify_between()
            .items_center()
            .padding_horiz(14.0)
            .padding_vert(8.0)
            .background(Color::from_rgb8(17, 17, 27))
            .border_bottom(1.0)
            .border_color(Color::from_rgba8(51, 65, 85, 100))
    });

    let servers_list = scroll(
        dyn_stack(
            move || mcp_data.servers.get(),
            |server| server.name.clone(),
            move |server| {
                let status_color = match server.status.as_str() {
                    "Active" => Color::from_rgb8(52, 211, 153),
                    "Error" => Color::from_rgb8(248, 113, 113),
                    _ => Color::from_rgb8(250, 204, 21),
                };

                v_stack((
                    h_stack((
                        h_stack((
                            label(|| "".to_string()).style(move |s| {
                                s.width(8.0)
                                    .height(8.0)
                                    .border_radius(99.0)
                                    .background(status_color)
                                    .margin_right(8.0)
                            }),
                            label(move || server.name.clone()).style(|s| {
                                s.font_size(13.0)
                                    .font_weight(Weight::SEMIBOLD)
                                    .color(Color::from_rgb8(203, 213, 225))
                            }),
                        ))
                        .style(|s| s.items_center()),
                        label(|| "•••").style(|s| {
                            s.color(Color::from_rgb8(100, 116, 139))
                                .font_size(14.0)
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| s.color(Color::from_rgb8(203, 213, 225)))
                        }),
                    ))
                    .style(|s| s.width_full().justify_between().items_center()),
                    label(move || server.config_path.clone()).style(|s| {
                        s.font_size(10.0)
                            .color(Color::from_rgb8(100, 116, 139))
                            .margin_top(4.0)
                            .margin_left(16.0)
                    }),
                ))
                .style(|s| {
                    s.width_full()
                        .padding_horiz(14.0)
                        .padding_vert(10.0)
                        .border_bottom(1.0)
                        .border_color(Color::from_rgba8(51, 65, 85, 50))
                        .hover(|s| s.background(Color::from_rgba8(255, 255, 255, 10)))
                })
            },
        )
        .style(|s| s.width_full().flex_col()),
    )
    .style(|s| s.width_full().flex_grow(1.0));

    v_stack((header, servers_list)).style(|s: Style| {
        s.width_full()
            .height_full()
            .background(Color::from_rgb8(13, 13, 18))
    })
}
