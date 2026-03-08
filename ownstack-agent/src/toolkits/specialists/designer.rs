//! UX/UI Designer Specialist — generates mockup descriptions and layout
//! previews with responsive breakpoints.

use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde_json::json;
use tracing::info;

pub struct DesignerToolkit;

#[async_trait]
impl Toolkit for DesignerToolkit {
    fn name(&self) -> &str {
        "designer"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "generate_ui_mockup".to_string(),
                description: "Generate a structured UI mockup (wireframe description + HTML skeleton)"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "component": {"type": "string", "description": "Name of the component or screen"},
                        "description": {"type": "string", "description": "Visual requirements"},
                    },
                    "required": ["component", "description"],
                }),
            },
            ToolDef {
                name: "preview_layout".to_string(),
                description: "Preview a layout structure with responsive breakpoints"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "layout_def": {"type": "string", "description": "Layout definition (e.g. 'header,sidebar+main,footer')"},
                    },
                    "required": ["layout_def"],
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        match tool_name {
            "generate_ui_mockup" => {
                let component = args["component"].as_str().unwrap_or("Component");
                let description = args["description"].as_str().unwrap_or("");
                info!("DesignerToolkit: generating mockup for '{component}'");

                let slug = component
                    .to_lowercase()
                    .replace(' ', "-")
                    .replace(|c: char| !c.is_alphanumeric() && c != '-', "");

                let desc_short = if description.len() > 20 {
                    &description[..20]
                } else {
                    description
                };

                let mockup = format!(
                    "## UI Mockup: {component}\n\n\
                     ### Requirements\n{description}\n\n\
                     ### Wireframe (ASCII)\n\
                     ```\n\
                     ┌─────────────────────────────────┐\n\
                     │  {component:^30} │\n\
                     ├─────────────────────────────────┤\n\
                     │                                 │\n\
                     │   [Content Area]                │\n\
                     │   Based on: {desc_short:<20}│\n\
                     │                                 │\n\
                     ├─────────────────────────────────┤\n\
                     │  [Action Button]                │\n\
                     └─────────────────────────────────┘\n\
                     ```\n\n\
                     ### HTML Skeleton\n\
                     ```html\n\
                     <section class=\"{slug}\" role=\"region\" aria-label=\"{component}\">\n\
                       <header class=\"{slug}__header\"><h2>{component}</h2></header>\n\
                       <div class=\"{slug}__content\"><!-- {description} --></div>\n\
                       <footer class=\"{slug}__actions\">\n\
                         <button class=\"btn btn--primary\">Submit</button>\n\
                       </footer>\n\
                     </section>\n\
                     ```\n\n\
                     ### Responsive Notes\n\
                     - **Mobile** (< 640px): Stack vertically, full-width buttons\n\
                     - **Tablet** (640-1024px): Side padding 24px\n\
                     - **Desktop** (> 1024px): Max-width 960px, centered\n",
                );

                Ok(ToolResult::success(mockup))
            }
            "preview_layout" => {
                let layout_def = args["layout_def"].as_str().unwrap_or("header,main,footer");
                info!("DesignerToolkit: previewing layout '{layout_def}'");

                let rows: Vec<&str> = layout_def.split(',').collect();
                let mut css = String::from(
                    "### Layout Preview\n\n```css\n.layout {\n  display: grid;\n  grid-template-rows: ",
                );

                let mut areas = Vec::new();
                for row in &rows {
                    let cols: Vec<&str> = row.split('+').collect();
                    if row.contains("header") || row.contains("footer") || row.contains("nav") {
                        css.push_str("auto ");
                    } else {
                        css.push_str("1fr ");
                    }
                    let area_row: Vec<String> =
                        cols.iter().map(|c| c.trim().to_string()).collect();
                    areas.push(area_row);
                }
                css.push_str(";\n");

                let max_cols = areas.iter().map(|r| r.len()).max().unwrap_or(1);
                css.push_str(&format!(
                    "  grid-template-columns: repeat({max_cols}, 1fr);\n"
                ));

                css.push_str("  grid-template-areas:\n");
                for row in &areas {
                    let mut padded = row.clone();
                    while padded.len() < max_cols {
                        padded.push(padded.last().cloned().unwrap_or_default());
                    }
                    let area_str = padded.join(" ");
                    css.push_str(&format!("    \"{area_str}\"\n"));
                }
                css.push_str("  ;\n  gap: 8px;\n  min-height: 100vh;\n}\n```\n\n");
                css.push_str("### Responsive Breakpoints\n");
                css.push_str("- **640px**: Stack all areas vertically\n");
                css.push_str("- **1024px**: Restore grid layout\n");

                Ok(ToolResult::success(css))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}
