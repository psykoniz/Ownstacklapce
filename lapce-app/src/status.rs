use std::{
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
};

use floem::{
    View,
    event::EventPropagation,
    reactive::{
        Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo,
    },
    style::{AlignItems, CursorStyle, Display},
    text::Weight,
    views::{Decorators, dyn_stack, empty, label, stack, svg},
};
use indexmap::IndexMap;
use lapce_core::mode::{Mode, VisualMode};
use lsp_types::{DiagnosticSeverity, ProgressToken};

use crate::{
    app::clickable_icon,
    command::LapceWorkbenchCommand,
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    editor::EditorData,
    listener::Listener,
    palette::kind::PaletteKind,
    panel::{kind::PanelKind, position::PanelContainerPosition},
    source_control::SourceControlData,
    window_tab::{WindowTabData, WorkProgress},
};

pub fn status(
    window_tab_data: Rc<WindowTabData>,
    source_control: SourceControlData,
    workbench_command: Listener<LapceWorkbenchCommand>,
    status_height: RwSignal<f64>,
    _config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let config = window_tab_data.common.config;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let editor = window_tab_data.main_split.active_editor;
    let panel = window_tab_data.panel.clone();
    let palette = window_tab_data.palette.clone();
    let ownstack_palette = window_tab_data.ownstack_palette.clone();
    let ownstack_audit = window_tab_data.ownstack_audit.clone();
    let ownstack_status = window_tab_data.ownstack_status.clone();
    let diagnostic_count = create_memo(move |_| {
        let mut errors = 0;
        let mut warnings = 0;
        for (_, diagnostics) in diagnostics.get().iter() {
            for diagnostic in diagnostics.diagnostics.get().iter() {
                if let Some(severity) = diagnostic.severity {
                    match severity {
                        DiagnosticSeverity::ERROR => errors += 1,
                        DiagnosticSeverity::WARNING => warnings += 1,
                        _ => (),
                    }
                }
            }
        }
        (errors, warnings)
    });
    let branch = source_control.branch;
    let file_diffs = source_control.file_diffs;
    let branch = move || {
        format!(
            "{}{}",
            branch.get(),
            if file_diffs.with(|diffs| diffs.is_empty()) {
                ""
            } else {
                "*"
            }
        )
    };

    let progresses = window_tab_data.progresses;
    let window_tab_data_for_mode = window_tab_data.clone();
    let mode = create_memo(move |_| window_tab_data_for_mode.mode());
    let pointer_down = floem::reactive::create_rw_signal(false);

    stack((
        stack((
            label(move || match mode.get() {
                Mode::Normal => "Normal".to_string(),
                Mode::Insert => "Insert".to_string(),
                Mode::Visual(mode) => match mode {
                    VisualMode::Normal => "Visual".to_string(),
                    VisualMode::Linewise => "Visual Line".to_string(),
                    VisualMode::Blockwise => "Visual Block".to_string(),
                },
                Mode::Terminal => "Terminal".to_string(),
            })
            .style(move |s| {
                let config = config.get();
                let display = if config.core.modal {
                    Display::Flex
                } else {
                    Display::None
                };

                let (bg, fg) = match mode.get() {
                    Mode::Normal => (
                        LapceColor::STATUS_MODAL_NORMAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_NORMAL_FOREGROUND,
                    ),
                    Mode::Insert => (
                        LapceColor::STATUS_MODAL_INSERT_BACKGROUND,
                        LapceColor::STATUS_MODAL_INSERT_FOREGROUND,
                    ),
                    Mode::Visual(_) => (
                        LapceColor::STATUS_MODAL_VISUAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_VISUAL_FOREGROUND,
                    ),
                    Mode::Terminal => (
                        LapceColor::STATUS_MODAL_TERMINAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_TERMINAL_FOREGROUND,
                    ),
                };

                let bg = config.color(bg);
                let fg = config.color(fg);

                s.display(display)
                    .padding_horiz(10.0)
                    .color(fg)
                    .background(bg)
                    .height_pct(100.0)
                    .align_items(Some(AlignItems::Center))
                    .selectable(false)
            }),
            stack((
                svg(move || config.get().ui_svg(LapceIcons::SCM)).style(move |s| {
                    let config = config.get();
                    let icon_size = config.ui.icon_size() as f32;
                    s.size(icon_size, icon_size)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                }),
                label(branch).style(move |s| {
                    s.margin_left(10.0)
                        .color(config.get().color(LapceColor::STATUS_FOREGROUND))
                        .selectable(false)
                }),
            ))
            .style(move |s| {
                s.display(if branch().is_empty() {
                    Display::None
                } else {
                    Display::Flex
                })
                .height_pct(100.0)
                .padding_horiz(10.0)
                .align_items(Some(AlignItems::Center))
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.get().color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
            })
            .on_event_cont(floem::event::EventListener::PointerDown, move |_| {
                pointer_down.set(true);
            })
            .on_event(
                floem::event::EventListener::PointerUp,
                move |_| {
                    if pointer_down.get() {
                        workbench_command
                            .send(LapceWorkbenchCommand::PaletteSCMReferences);
                    }
                    pointer_down.set(false);
                    EventPropagation::Continue
                },
            ),
            {
                let panel = panel.clone();
                stack((
                    svg(move || config.get().ui_svg(LapceIcons::ERROR)).style(
                        move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size)
                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        },
                    ),
                    label(move || diagnostic_count.get().0.to_string()).style(
                        move |s| {
                            s.margin_left(5.0)
                                .color(
                                    config
                                        .get()
                                        .color(LapceColor::STATUS_FOREGROUND),
                                )
                                .selectable(false)
                        },
                    ),
                    svg(move || config.get().ui_svg(LapceIcons::WARNING)).style(
                        move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size)
                                .margin_left(5.0)
                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        },
                    ),
                    label(move || diagnostic_count.get().1.to_string()).style(
                        move |s| {
                            s.margin_left(5.0)
                                .color(
                                    config
                                        .get()
                                        .color(LapceColor::STATUS_FOREGROUND),
                                )
                                .selectable(false)
                        },
                    ),
                ))
                .on_click_stop(move |_| {
                    panel.show_panel(&PanelKind::Problem);
                })
                .style(move |s| {
                    s.height_pct(100.0)
                        .padding_horiz(10.0)
                        .items_center()
                        .hover(|s| {
                            s.cursor(CursorStyle::Pointer).background(
                                config
                                    .get()
                                    .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
            },
            progress_view(config, progresses),
            {
                let panel = panel.clone();
                let ownstack_style = ownstack_status.clone();
                stack((
                    {
                        let window_tab_data = window_tab_data.clone();
                        clickable_icon(
                            || LapceIcons::DEBUG_CONSOLE,
                            move || {
                                window_tab_data.take_ui_snapshot();
                            },
                            || false,
                            || false,
                            || "Take UI Snapshot (Vision Bridge)",
                            config,
                        )
                        .style(|s| s.margin_right(10.0))
                    },
                    // Vertical separator between Lapce and OwnStack zones
                    empty().style(move |s| {
                        s.width(1.0)
                            .height(16.0)
                            .margin_horiz(6.0)
                            .background(
                                config
                                    .get()
                                    .color(LapceColor::LAPCE_BORDER)
                                    .multiply_alpha(0.6),
                            )
                    }),
                    label(|| "AI Cmd".to_string())
                        .debug_name("AI Cmd Button")
                        .on_click_stop({
                            let ownstack_palette = ownstack_palette.clone();
                            move |_| {
                                if ownstack_palette.active.get_untracked() {
                                    ownstack_palette.hide();
                                } else {
                                    ownstack_palette.show();
                                }
                            }
                        })
                        .style(move |s| {
                            let config = config.get();
                            s.padding_horiz(10.0)
                                .padding_vert(3.0)
                                .margin_right(6.0)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(
                                    config.color(LapceColor::LAPCE_BORDER),
                                )
                                .background(
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                        .multiply_alpha(0.3),
                                )
                                .color(
                                    config.color(LapceColor::STATUS_FOREGROUND),
                                )
                                .font_size(12.0)
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| {
                                    s.background(
                                        config
                                            .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                            .multiply_alpha(0.7),
                                    )
                                    .border_color(
                                        config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE),
                                    )
                                })
                                .active(|s| {
                                    s.background(
                                        config
                                            .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                                            .multiply_alpha(0.3),
                                    )
                                })
                        }),
                    label(|| "Audit".to_string())
                        .debug_name("Audit Button")
                        .on_click_stop({
                            let ownstack_audit = ownstack_audit.clone();
                            move |_| {
                                if !ownstack_audit.visible.get_untracked() {
                                    ownstack_audit.reload_from_disk();
                                }
                                ownstack_audit.toggle();
                            }
                        })
                        .style(move |s| {
                            let config = config.get();
                            s.padding_horiz(10.0)
                                .padding_vert(3.0)
                                .margin_right(6.0)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(
                                    config.color(LapceColor::LAPCE_BORDER),
                                )
                                .background(
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                        .multiply_alpha(0.3),
                                )
                                .color(
                                    config.color(LapceColor::STATUS_FOREGROUND),
                                )
                                .font_size(12.0)
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| {
                                    s.background(
                                        config
                                            .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                            .multiply_alpha(0.7),
                                    )
                                    .border_color(
                                        config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE),
                                    )
                                })
                                .active(|s| {
                                    s.background(
                                        config
                                            .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                                            .multiply_alpha(0.3),
                                    )
                                })
                        }),
                    label(|| "Settings".to_string())
                        .debug_name("Settings Button")
                        .on_click_stop(move |_| {
                            workbench_command
                                .send(LapceWorkbenchCommand::OpenSettings);
                        })
                        .style(move |s| {
                            let config = config.get();
                            s.padding_horiz(10.0)
                                .padding_vert(3.0)
                                .margin_right(6.0)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(
                                    config.color(LapceColor::LAPCE_BORDER),
                                )
                                .background(
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                        .multiply_alpha(0.3),
                                )
                                .color(
                                    config.color(LapceColor::STATUS_FOREGROUND),
                                )
                                .font_size(12.0)
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| {
                                    s.background(
                                        config
                                            .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                            .multiply_alpha(0.7),
                                    )
                                    .border_color(
                                        config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE),
                                    )
                                })
                                .active(|s| {
                                    s.background(
                                        config
                                            .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                                            .multiply_alpha(0.3),
                                    )
                                })
                        }),
                    {
                        let ownstack_mode_label = ownstack_style.clone();
                        let ownstack_mode_color = ownstack_style.clone();
                        label(move || ownstack_mode_label.mode_label().to_uppercase())
                            .style(move |s| {
                                let config = config.get();
                                let badge_bg = match ownstack_mode_color.mode.get() {
                                    crate::ownstack_chat::AgentMode::Ask => {
                                        config.color(
                                            LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE,
                                        )
                                    }
                                    crate::ownstack_chat::AgentMode::Auto => {
                                        config.color(LapceColor::SOURCE_CONTROL_ADDED)
                                    }
                                    crate::ownstack_chat::AgentMode::Plan => {
                                        config.color(LapceColor::LAPCE_WARN)
                                    }
                                    crate::ownstack_chat::AgentMode::Project => {
                                        config.color(LapceColor::SOURCE_CONTROL_ADDED)
                                    }
                                };

                                s.padding_horiz(8.0)
                                    .padding_vert(3.0)
                                    .margin_right(8.0)
                                    .border_radius(999.0)
                                    .background(badge_bg.multiply_alpha(0.9))
                                    .color(config.color(LapceColor::STATUS_BACKGROUND))
                                    .font_size(11.0)
                                    .font_weight(Weight::SEMIBOLD)
                                    .selectable(false)
                            })
                    },
                    label(move || ownstack_status.detail_label()).style(move |s| {
                        s.margin_left(4.0)
                            .color(config.get().color(LapceColor::STATUS_FOREGROUND))
                            .selectable(false)
                    }),
                    {
                        let budget_label = ownstack_style.clone();
                        let budget_level = ownstack_style.clone();
                        label(move || budget_label.combined_budget_label()).style(
                            move |s| {
                                let config = config.get();
                                let (fg, bg, border) =
                                    match budget_level.combined_budget_level() {
                                        crate::ownstack_status::BudgetLevel::Healthy => (
                                            config.color(
                                                LapceColor::SOURCE_CONTROL_ADDED,
                                            ),
                                            config
                                                .color(LapceColor::SOURCE_CONTROL_ADDED)
                                                .multiply_alpha(0.18),
                                            config.color(
                                                LapceColor::SOURCE_CONTROL_ADDED,
                                            ),
                                        ),
                                        crate::ownstack_status::BudgetLevel::Warning => (
                                            config.color(LapceColor::LAPCE_WARN),
                                            config
                                                .color(LapceColor::LAPCE_WARN)
                                                .multiply_alpha(0.18),
                                            config.color(LapceColor::LAPCE_WARN),
                                        ),
                                        crate::ownstack_status::BudgetLevel::Critical => (
                                            config.color(LapceColor::LAPCE_ERROR),
                                            config
                                                .color(LapceColor::LAPCE_ERROR)
                                                .multiply_alpha(0.18),
                                            config.color(LapceColor::LAPCE_ERROR),
                                        ),
                                        crate::ownstack_status::BudgetLevel::Unknown => (
                                            config
                                                .color(LapceColor::STATUS_FOREGROUND),
                                            config
                                                .color(LapceColor::STATUS_BACKGROUND)
                                                .multiply_alpha(0.7),
                                            config.color(LapceColor::LAPCE_BORDER),
                                        ),
                                    };

                                s.padding_horiz(7.0)
                                    .padding_vert(2.0)
                                    .margin_left(6.0)
                                    .border(1.0)
                                    .border_radius(999.0)
                                    .border_color(border)
                                    .background(bg)
                                    .color(fg)
                                    .font_size(11.0)
                                    .selectable(false)
                            },
                        )
                    },
                ))
                    .on_click_stop(move |_| {
                        panel.show_panel(&PanelKind::OwnStackChat);
                    })
                    .style(move |s| {
                        let config = config.get();
                        let (text_color, border_color, background_color) =
                            match ownstack_style.run_state() {
                                crate::ownstack_status::OwnStackRunState::Running => (
                                    config.color(LapceColor::STATUS_FOREGROUND),
                                    config.color(
                                        LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE,
                                    ),
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                        .multiply_alpha(0.9),
                                ),
                                crate::ownstack_status::OwnStackRunState::Disconnected => (
                                    config.color(LapceColor::LAPCE_ERROR),
                                    config.color(LapceColor::LAPCE_ERROR),
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                        .multiply_alpha(0.7),
                                ),
                                crate::ownstack_status::OwnStackRunState::Idle => (
                                    config.color(LapceColor::STATUS_FOREGROUND),
                                    config.color(LapceColor::LAPCE_BORDER),
                                    config
                                        .color(LapceColor::STATUS_BACKGROUND)
                                        .multiply_alpha(0.65),
                                ),
                            };

                        s.height_pct(100.0)
                            .padding_horiz(10.0)
                            .padding_vert(2.0)
                            .margin_horiz(6.0)
                            .border(1.0)
                            .border_radius(6.0)
                            .border_color(border_color)
                            .background(background_color)
                            .items_center()
                            .color(text_color)
                            .hover(|s| {
                                s.cursor(CursorStyle::Pointer).background(
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                        .multiply_alpha(0.95),
                                )
                            })
                            .selectable(false)
                    })
            },
        ))
        .style(|s| {
            s.height_pct(100.0)
                .min_width(0.0)
                .flex_basis(0.0)
                .flex_grow(1.0)
                .items_center()
        }),
        stack((
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel
                            .is_container_shown(&PanelContainerPosition::Left, true)
                        {
                            LapceIcons::SIDEBAR_LEFT
                        } else {
                            LapceIcons::SIDEBAR_LEFT_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel.toggle_container_visual(&PanelContainerPosition::Left)
                    },
                    || false,
                    || false,
                    || "Toggle Left Panel",
                    config,
                )
            },
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel.is_container_shown(
                            &PanelContainerPosition::Bottom,
                            true,
                        ) {
                            LapceIcons::LAYOUT_PANEL
                        } else {
                            LapceIcons::LAYOUT_PANEL_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel
                            .toggle_container_visual(&PanelContainerPosition::Bottom)
                    },
                    || false,
                    || false,
                    || "Toggle Bottom Panel",
                    config,
                )
            },
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel
                            .is_container_shown(&PanelContainerPosition::Right, true)
                        {
                            LapceIcons::SIDEBAR_RIGHT
                        } else {
                            LapceIcons::SIDEBAR_RIGHT_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel.toggle_container_visual(&PanelContainerPosition::Right)
                    },
                    || false,
                    || false,
                    || "Toggle Right Panel",
                    config,
                )
            },
        ))
        .style(move |s| {
            s.height_pct(100.0)
                .items_center()
                .color(config.get().color(LapceColor::STATUS_FOREGROUND))
        }),
        stack({
            let palette_clone = palette.clone();
            let cursor_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let mut status = String::new();
                    let cursor = editor.cursor().get();
                    if let Some((line, column, character)) = editor
                        .doc_signal()
                        .get()
                        .buffer
                        .with(|buffer| cursor.get_line_col_char(buffer))
                    {
                        status = format!(
                            "Ln {}, Col {}, Char {}",
                            line + 1,
                            column + 1,
                            character,
                        );
                    }
                    if let Some(selection) = cursor.get_selection() {
                        let selection_range = selection.0.abs_diff(selection.1);

                        if selection.0 != selection.1 {
                            status =
                                format!("{status} ({selection_range} selected)");
                        }
                    }
                    let selection_count = cursor.get_selection_count();
                    if selection_count > 1 {
                        status = format!("{status} {selection_count} selections");
                    }
                    return status;
                }
                String::new()
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::Line);
            });
            let palette_clone = palette.clone();
            let line_ending_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let doc = editor.doc_signal().get();
                    doc.buffer.with(|b| b.line_ending()).as_str()
                } else {
                    ""
                }
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::LineEnding);
            });
            let palette_clone = palette.clone();
            let language_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let doc = editor.doc_signal().get();
                    doc.syntax().with(|s| s.language.name())
                } else {
                    "unknown"
                }
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::Language);
            });
            (cursor_info, line_ending_info, language_info)
        })
        .style(|s| {
            s.height_pct(100.0)
                .flex_basis(0.0)
                .flex_grow(1.0)
                .justify_end()
        }),
    ))
    .on_resize(move |rect| {
        let height = rect.height();
        if height != status_height.get_untracked() {
            status_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.border_top(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::STATUS_BACKGROUND))
            .flex_basis(config.ui.status_height() as f32)
            .flex_grow(0.0)
            .flex_shrink(0.0)
            .items_center()
    })
    .debug_name("Status/Bottom Bar")
}

fn progress_view(
    config: ReadSignal<Arc<LapceConfig>>,
    progresses: RwSignal<IndexMap<ProgressToken, WorkProgress>>,
) -> impl View {
    let id = AtomicU64::new(0);
    dyn_stack(
        move || progresses.get(),
        move |_| id.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        move |(_, p)| {
            let progress = match p.message {
                Some(message) if !message.is_empty() => {
                    format!("{}: {}", p.title, message)
                }
                _ => p.title,
            };
            label(move || progress.clone()).style(move |s| {
                s.height_pct(100.0)
                    .min_width(0.0)
                    .margin_left(10.0)
                    .text_ellipsis()
                    .selectable(false)
                    .items_center()
                    .color(config.get().color(LapceColor::STATUS_FOREGROUND))
            })
        },
    )
    .style(move |s| s.flex_row().height_pct(100.0).min_width(0.0))
}

fn status_text<S: std::fmt::Display + 'static>(
    config: ReadSignal<Arc<LapceConfig>>,
    editor: Memo<Option<EditorData>>,
    text: impl Fn() -> S + 'static,
) -> impl View {
    label(text).style(move |s| {
        let config = config.get();
        let display = if editor
            .get()
            .map(|editor| {
                editor.doc_signal().get().content.with(|c| {
                    use crate::doc::DocContent;
                    matches!(c, DocContent::File { .. } | DocContent::Scratch { .. })
                })
            })
            .unwrap_or(false)
        {
            Display::Flex
        } else {
            Display::None
        };

        s.display(display)
            .height_full()
            .padding_horiz(10.0)
            .items_center()
            .color(config.color(LapceColor::STATUS_FOREGROUND))
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
            })
            .selectable(false)
    })
}
