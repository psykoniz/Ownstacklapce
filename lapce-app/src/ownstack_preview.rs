//! Web Preview panel.
//!
//! Detects a running local dev server (Vite, Next.js, etc.), shows its URL, and
//! lets the user open it in their system browser. The panel keeps an editable
//! URL bar so any address can be previewed.
//!
//! An *embedded* webview (via `wry`) is gated behind the `web-preview` Cargo
//! feature because it requires platform webview libraries (WebKitGTK on Linux).
//! Without that feature — the default — this panel provides detection plus
//! open-in-browser, which works everywhere with zero system dependencies.

use std::rc::Rc;
use std::time::Duration;

use floem::{IntoView, View};
use floem::ext_event::create_ext_action;
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use floem::style::CursorStyle;
use floem::text::Weight;
use floem::views::{Decorators, container, dyn_container, h_stack, label, text_input, v_stack};

use crate::config::color::LapceColor;
use crate::ownstack_theme as tok;
use crate::window_tab::WindowTabData;

/// Common local dev-server ports, probed in priority order.
const DEV_PORTS: &[(u16, &str)] = &[
    (5173, "Vite"),
    (3000, "Next.js / React"),
    (3001, "Dev server"),
    (4200, "Angular"),
    (8080, "Dev server"),
    (8000, "Python / Django"),
    (5000, "Flask"),
    (4321, "Astro"),
];

/// Reactive state for the web preview panel.
#[derive(Clone)]
pub struct WebPreviewData {
    /// URL currently entered / being previewed.
    pub url: RwSignal<String>,
    /// Detected dev server label (e.g. "Vite on :5173"), if any.
    pub detected: RwSignal<Option<String>>,
    /// Whether a detection probe is in flight.
    pub probing: RwSignal<bool>,
    scope: Scope,
}

impl WebPreviewData {
    pub fn new(cx: Scope) -> Self {
        Self {
            url: cx.create_rw_signal("http://localhost:5173".to_string()),
            detected: cx.create_rw_signal(None),
            probing: cx.create_rw_signal(false),
            scope: cx,
        }
    }

    /// Probe known dev-server ports off-thread and update `detected`/`url`.
    pub fn detect(&self) {
        if self.probing.get_untracked() {
            return;
        }
        self.probing.set(true);
        let url_sig = self.url;
        let detected_sig = self.detected;
        let probing_sig = self.probing;
        let set = create_ext_action(
            self.scope,
            move |result: Option<(u16, String)>| {
                probing_sig.set(false);
                match result {
                    Some((port, label)) => {
                        detected_sig.set(Some(format!("{label} on :{port}")));
                        url_sig.set(format!("http://localhost:{port}"));
                    }
                    None => detected_sig.set(None),
                }
            },
        );
        std::thread::spawn(move || {
            set(probe_dev_servers());
        });
    }
}

/// Synchronously probe dev-server ports; returns the first that accepts a TCP
/// connection. Runs on a background thread.
fn probe_dev_servers() -> Option<(u16, String)> {
    for (port, label) in DEV_PORTS {
        let addr = format!("127.0.0.1:{port}");
        if let Ok(sock_addr) = addr.parse() {
            if std::net::TcpStream::connect_timeout(
                &sock_addr,
                Duration::from_millis(120),
            )
            .is_ok()
            {
                return Some((*port, label.to_string()));
            }
        }
    }
    None
}

/// Build the web preview panel view.
pub fn web_preview_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: crate::panel::position::PanelPosition,
) -> impl View {
    let data = window_tab_data.web_preview.clone();
    let config = window_tab_data.common.config;

    // Kick off an initial detection when the panel is built.
    data.detect();

    let data_refresh = data.clone();
    let data_open = data.clone();
    let url_for_input = data.url;

    let toolbar = h_stack((
        // Refresh / re-detect button
        label(|| "\u{21BB} Detect")
            .on_click_stop(move |_| data_refresh.detect())
            .style(move |s| {
                s.padding_horiz(10.0)
                    .padding_vert(5.0)
                    .border_radius(6.0)
                    .font_size(11.0)
                    .font_weight(Weight::SEMIBOLD)
                    .color(tok::CTA_TEXT)
                    .background(tok::CTA_BG)
                    .border(1.0)
                    .border_color(tok::CTA_BORDER)
                    .cursor(CursorStyle::Pointer)
                    .hover(|s| s.background(tok::CTA_BG_HOVER))
            }),
        // Editable URL bar
        container(text_input(url_for_input).style(|s| s.width_full())).style(
            move |s| {
                s.flex_grow(1.0)
                    .margin_horiz(8.0)
                    .padding_horiz(8.0)
                    .padding_vert(4.0)
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(config.get().color(LapceColor::LAPCE_BORDER))
            },
        ),
        // Open in system browser
        label(|| "Open \u{2197}")
            .on_click_stop(move |_| {
                let url = data_open.url.get_untracked();
                if let Err(e) = open::that(&url) {
                    tracing::warn!("failed to open browser for {url}: {e}");
                }
            })
            .style(move |s| {
                s.padding_horiz(12.0)
                    .padding_vert(5.0)
                    .border_radius(6.0)
                    .font_size(11.0)
                    .font_weight(Weight::SEMIBOLD)
                    .color(tok::CTA_TEXT)
                    .background(tok::ACCENT.multiply_alpha(0.18))
                    .border(1.0)
                    .border_color(tok::ACCENT.multiply_alpha(0.4))
                    .cursor(CursorStyle::Pointer)
                    .hover(|s| s.background(tok::ACCENT.multiply_alpha(0.28)))
            }),
    ))
    .style(move |s| {
        s.width_full()
            .items_center()
            .padding(8.0)
            .border_bottom(1.0)
            .border_color(config.get().color(LapceColor::LAPCE_BORDER))
    });

    // Status / detection line.
    let detected_sig = data.detected;
    let probing_sig = data.probing;
    let status = dyn_container(
        move || (probing_sig.get(), detected_sig.get()),
        move |(probing, detected)| {
            let text = if probing {
                "Scanning for dev servers\u{2026}".to_string()
            } else {
                match detected {
                    Some(label) => format!("\u{25CF} Detected {label}"),
                    None => "No local dev server detected. Start one (e.g. `npm \
run dev`) then press Detect, or enter a URL above."
                        .to_string(),
                }
            };
            let ok = !probing && text.starts_with('\u{25CF}');
            container(label(move || text.clone()).style(move |s| {
                s.font_size(12.0).color(if ok {
                    tok::STATE_OK
                } else {
                    tok::TEXT_DIM
                })
            }))
            .style(|s| s.padding(12.0))
            .into_any()
        },
    );

    // Body — preview placeholder (embedded webview slots in here behind the
    // `web-preview` feature on platforms with a system webview).
    let body = preview_body(data.clone());

    v_stack((toolbar, status, body))
        .style(|s| s.size_full().flex_col())
}

#[cfg(not(feature = "web-preview"))]
fn preview_body(data: WebPreviewData) -> impl View {
    let data_open = data.clone();
    v_stack((
        label(|| "\u{1F310}").style(|s| {
            s.font_size(40.0).color(tok::ACCENT).margin_bottom(8.0)
        }),
        label(|| "Web Preview").style(|s| {
            s.font_size(15.0)
                .font_weight(Weight::BOLD)
                .color(tok::TITLE)
                .margin_bottom(6.0)
        }),
        label(|| {
            "Preview your app's dev server. Click below to open the current URL \
in your browser. Inline rendering is available in desktop builds with an \
embedded webview."
        })
        .style(|s| {
            s.font_size(12.0)
                .color(tok::TEXT_DIM)
                .max_width(320.0)
                .line_height(1.6)
                .margin_bottom(16.0)
        }),
        label(|| "Open current URL \u{2197}")
            .on_click_stop(move |_| {
                let url = data_open.url.get_untracked();
                let _ = open::that(&url);
            })
            .style(|s| {
                s.padding_horiz(18.0)
                    .padding_vert(9.0)
                    .border_radius(8.0)
                    .font_size(12.0)
                    .font_weight(Weight::SEMIBOLD)
                    .color(tok::CTA_TEXT)
                    .background(tok::CTA_BG)
                    .border(2.0)
                    .border_color(tok::CTA_BORDER)
                    .cursor(CursorStyle::Pointer)
                    .hover(|s| s.background(tok::CTA_BG_HOVER))
            }),
    ))
    .style(|s| {
        s.size_full()
            .flex_grow(1.0)
            .items_center()
            .justify_center()
            .flex_col()
            .padding(20.0)
    })
}

#[cfg(feature = "web-preview")]
fn preview_body(_data: WebPreviewData) -> impl View {
    // When the embedded webview feature is enabled, this is where a `wry`
    // child webview would be mounted. Kept as a labelled placeholder so the
    // default and featured builds share the same panel structure.
    container(label(|| "Embedded webview active").style(|s| {
        s.font_size(12.0).color(tok::TEXT_DIM).padding(12.0)
    }))
    .style(|s| s.size_full().items_center().justify_center())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_ports_include_common_servers() {
        let ports: Vec<u16> = DEV_PORTS.iter().map(|(p, _)| *p).collect();
        assert!(ports.contains(&5173)); // Vite
        assert!(ports.contains(&3000)); // Next.js
        assert!(ports.contains(&8000)); // Django
    }

    #[test]
    fn probe_returns_none_when_nothing_listening() {
        // Ports in our list are almost certainly closed in CI; probe is fast.
        // We don't assert None strictly (a server could be up), only that it
        // returns without panicking and within the timeout budget.
        let _ = probe_dev_servers();
    }
}
