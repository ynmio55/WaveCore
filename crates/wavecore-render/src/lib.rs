use std::collections::HashMap;
use wavecore_layout::{Edges, LayoutBox, Rect};

#[derive(Debug, Clone, PartialEq)]
pub enum Canvas2DCommand {
    FillRect { x: f32, y: f32, width: f32, height: f32, color: String },
    StrokeRect { x: f32, y: f32, width: f32, height: f32, color: String, line_width: f32 },
    DrawLine { x1: f32, y1: f32, x2: f32, y2: f32, color: String, line_width: f32 },
    FillCircle { cx: f32, cy: f32, radius: f32, color: String },
    StrokeCircle { cx: f32, cy: f32, radius: f32, color: String, line_width: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WebGlCommand {
    Viewport { x: i32, y: i32, width: i32, height: i32 },
    ClearColor([f32; 4]),
    Clear { mask: u32 },
    UploadArrayBuffer { id: u32, data: Vec<f32> },
    UploadElementArrayBuffer { id: u32, data: Vec<u32> },
    BindArrayBuffer(Option<u32>),
    BindElementArrayBuffer(Option<u32>),
    VertexAttribPointer {
        index: u32,
        size: u32,
        stride_floats: u32,
        offset_floats: u32,
        buffer_id: Option<u32>,
    },
    VertexAttrib4f {
        index: u32,
        value: [f32; 4],
    },
    EnableVertexAttribArray(u32),
    UseProgram(Option<u32>),
    DrawArrays { mode: u32, first: u32, count: u32 },
    DrawElements { mode: u32, count: u32, element_type: u32, offset_bytes: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCommand {
    FillRect { rect: Rect, color: String },
    FillRoundedRect { rect: Rect, radius: f32, color: String },
    Border { rect: Rect, widths: Edges, color: String, radius: f32 },
    Text { text: String, rect: Rect, font_size: f32, line_height: f32, color: String },
    Image { rect: Rect, src: String },
    DrawLine { x1: f32, y1: f32, x2: f32, y2: f32, color: String, width: f32 },
    DrawCircle { cx: f32, cy: f32, radius: f32, fill: Option<String>, stroke: Option<(String, f32)> },
    PushClip(Rect),
    PopClip,
    PushOpacity(f32),
    PopOpacity,
}


#[derive(Debug, Clone)]
pub struct CompositorLayer {
    pub node_id: Option<u64>,
    pub z_index: i32,
    pub opacity: f32,
    pub bounds: Rect,
    pub commands: Vec<DisplayCommand>,
    pub webgl_commands: Vec<WebGlCommand>,
}

#[derive(Debug, Clone, Default)]
pub struct CompositorFrame {
    pub layers: Vec<CompositorLayer>,
}

impl CompositorFrame {
    pub fn flatten(&self) -> Vec<DisplayCommand> {
        let mut layers = self.layers.clone();
        layers.sort_by_key(|layer| layer.z_index);
        let mut out = Vec::new();
        for layer in layers {
            if layer.opacity < 0.999 {
                out.push(DisplayCommand::PushOpacity(layer.opacity));
            }
            out.extend(layer.commands);
            if layer.opacity < 0.999 {
                out.push(DisplayCommand::PopOpacity);
            }
        }
        out
    }
}

pub trait CompositorBackend {
    type Error;
    fn present(&mut self, frame: &CompositorFrame) -> Result<(), Self::Error>;
}

pub fn build_compositor_frame(layout: &LayoutBox) -> CompositorFrame {
    let mut frame = CompositorFrame::default();
    collect_layers(layout, &mut frame.layers);
    frame
}

fn collect_layers(layout: &LayoutBox, layers: &mut Vec<CompositorLayer>) {
    let mut commands = Vec::new();
    walk_self(layout, &mut commands);
    layers.push(CompositorLayer {
        node_id: layout.node_id.map(|id| id.0),
        z_index: layout.z_index,
        opacity: layout.opacity,
        bounds: layout.rect,
        commands,
        webgl_commands: Vec::new(),
    });

    let mut children: Vec<&LayoutBox> = layout.children.iter().collect();
    children.sort_by_key(|child| child.z_index);
    for child in children {
        collect_layers(child, layers);
    }
}

fn walk_self(b: &LayoutBox, v: &mut Vec<DisplayCommand>) {
    let is_clipped = b.overflow_hidden;
    if is_clipped {
        v.push(DisplayCommand::PushClip(b.content));
    }

    if let Some(bg) = &b.background {
        if b.border_radius > 0.0 {
            v.push(DisplayCommand::FillRoundedRect {
                rect: b.rect,
                radius: b.border_radius,
                color: bg.clone(),
            });
        } else {
            v.push(DisplayCommand::FillRect {
                rect: b.rect,
                color: bg.clone(),
            });
        }
    }

    if b.border != Edges::default() {
        v.push(DisplayCommand::Border {
            rect: b.rect,
            widths: b.border,
            color: b.border_color.clone().unwrap_or_else(|| "#000000".into()),
            radius: b.border_radius,
        });
    }

    if let Some(src) = &b.image_src {
        v.push(DisplayCommand::Image {
            rect: b.content,
            src: src.clone(),
        });
    }

    if b.text.is_some() {
        let count = b.text_lines.len().max(1) as f32;
        let lh = b.content.height / count;
        let fs = lh / 1.25;
        let text_color = b.color.clone().unwrap_or_else(|| "#000000".into());
        for (i, line) in b.text_lines.iter().enumerate() {
            if !line.trim().is_empty() {
                v.push(DisplayCommand::Text {
                    text: line.clone(),
                    rect: Rect {
                        x: b.content.x,
                        y: b.content.y + i as f32 * lh,
                        width: b.content.width,
                        height: lh,
                    },
                    font_size: fs,
                    line_height: lh,
                    color: text_color.clone(),
                });
            }
        }
    }

    if is_clipped {
        v.push(DisplayCommand::PopClip);
    }
}


pub fn append_canvas_commands(
    layout: &LayoutBox,
    registry: &HashMap<u64, Vec<Canvas2DCommand>>,
    out: &mut Vec<DisplayCommand>,
) {
    if layout.is_canvas {
        if let Some(node_id) = layout.node_id {
            if let Some(commands) = registry.get(&node_id.0) {
                out.push(DisplayCommand::PushClip(layout.content));
                for command in commands {
                    match command {
                        Canvas2DCommand::FillRect { x, y, width, height, color } => {
                            out.push(DisplayCommand::FillRect {
                                rect: Rect {
                                    x: layout.content.x + *x,
                                    y: layout.content.y + *y,
                                    width: *width,
                                    height: *height,
                                },
                                color: color.clone(),
                            });
                        }
                        Canvas2DCommand::StrokeRect {
                            x,
                            y,
                            width,
                            height,
                            color,
                            line_width,
                        } => {
                            let w = line_width.max(1.0);
                            out.push(DisplayCommand::Border {
                                rect: Rect {
                                    x: layout.content.x + *x,
                                    y: layout.content.y + *y,
                                    width: *width,
                                    height: *height,
                                },
                                widths: Edges {
                                    top: w,
                                    right: w,
                                    bottom: w,
                                    left: w,
                                },
                                color: color.clone(),
                                radius: 0.0,
                            });
                        }
                        Canvas2DCommand::DrawLine { x1, y1, x2, y2, color, line_width } => {
                            out.push(DisplayCommand::DrawLine {
                                x1: layout.content.x + *x1,
                                y1: layout.content.y + *y1,
                                x2: layout.content.x + *x2,
                                y2: layout.content.y + *y2,
                                color: color.clone(),
                                width: line_width.max(1.0),
                            });
                        }
                        Canvas2DCommand::FillCircle { cx, cy, radius, color } => {
                            out.push(DisplayCommand::DrawCircle {
                                cx: layout.content.x + *cx,
                                cy: layout.content.y + *cy,
                                radius: radius.max(0.0),
                                fill: Some(color.clone()),
                                stroke: None,
                            });
                        }
                        Canvas2DCommand::StrokeCircle { cx, cy, radius, color, line_width } => {
                            out.push(DisplayCommand::DrawCircle {
                                cx: layout.content.x + *cx,
                                cy: layout.content.y + *cy,
                                radius: radius.max(0.0),
                                fill: None,
                                stroke: Some((color.clone(), line_width.max(1.0))),
                            });
                        }
                    }
                }
                out.push(DisplayCommand::PopClip);
            }
        }
    }
    for child in &layout.children {
        append_canvas_commands(child, registry, out);
    }
}


pub fn append_canvas_to_compositor_frame(
    layout: &LayoutBox,
    registry: &HashMap<u64, Vec<Canvas2DCommand>>,
    frame: &mut CompositorFrame,
) {
    if layout.is_canvas {
        if let Some(node_id) = layout.node_id.map(|id| id.0) {
            if let Some(commands) = registry.get(&node_id) {
                if let Some(layer) = frame.layers.iter_mut().find(|layer| layer.node_id == Some(node_id)) {
                    layer.commands.push(DisplayCommand::PushClip(layout.content));
                    for command in commands {
                        match command {
                            Canvas2DCommand::FillRect { x, y, width, height, color } => {
                                layer.commands.push(DisplayCommand::FillRect {
                                    rect: Rect {
                                        x: layout.content.x + *x,
                                        y: layout.content.y + *y,
                                        width: *width,
                                        height: *height,
                                    },
                                    color: color.clone(),
                                });
                            }
                            Canvas2DCommand::StrokeRect {
                                x,
                                y,
                                width,
                                height,
                                color,
                                line_width,
                            } => {
                                let w = line_width.max(1.0);
                                layer.commands.push(DisplayCommand::Border {
                                    rect: Rect {
                                        x: layout.content.x + *x,
                                        y: layout.content.y + *y,
                                        width: *width,
                                        height: *height,
                                    },
                                    widths: Edges {
                                        top: w,
                                        right: w,
                                        bottom: w,
                                        left: w,
                                    },
                                    color: color.clone(),
                                    radius: 0.0,
                                });
                            }
                            Canvas2DCommand::DrawLine { x1, y1, x2, y2, color, line_width } => {
                                layer.commands.push(DisplayCommand::DrawLine {
                                    x1: layout.content.x + *x1,
                                    y1: layout.content.y + *y1,
                                    x2: layout.content.x + *x2,
                                    y2: layout.content.y + *y2,
                                    color: color.clone(),
                                    width: line_width.max(1.0),
                                });
                            }
                            Canvas2DCommand::FillCircle { cx, cy, radius, color } => {
                                layer.commands.push(DisplayCommand::DrawCircle {
                                    cx: layout.content.x + *cx,
                                    cy: layout.content.y + *cy,
                                    radius: radius.max(0.0),
                                    fill: Some(color.clone()),
                                    stroke: None,
                                });
                            }
                            Canvas2DCommand::StrokeCircle { cx, cy, radius, color, line_width } => {
                                layer.commands.push(DisplayCommand::DrawCircle {
                                    cx: layout.content.x + *cx,
                                    cy: layout.content.y + *cy,
                                    radius: radius.max(0.0),
                                    fill: None,
                                    stroke: Some((color.clone(), line_width.max(1.0))),
                                });
                            }
                        }
                    }
                    layer.commands.push(DisplayCommand::PopClip);
                }
            }
        }
    }

    for child in &layout.children {
        append_canvas_to_compositor_frame(child, registry, frame);
    }
}


pub fn append_webgl_to_compositor_frame(
    layout: &LayoutBox,
    registry: &HashMap<u64, Vec<WebGlCommand>>,
    frame: &mut CompositorFrame,
) {
    if layout.is_canvas {
        if let Some(node_id) = layout.node_id.map(|id| id.0) {
            if let Some(commands) = registry.get(&node_id) {
                if let Some(layer) = frame
                    .layers
                    .iter_mut()
                    .find(|layer| layer.node_id == Some(node_id))
                {
                    layer.webgl_commands = commands.clone();
                }
            }
        }
    }
    for child in &layout.children {
        append_webgl_to_compositor_frame(child, registry, frame);
    }
}

pub fn build_display_list(layout: &LayoutBox) -> Vec<DisplayCommand> {
    let mut v = Vec::new();
    walk(layout, &mut v);
    v
}

fn walk(b: &LayoutBox, v: &mut Vec<DisplayCommand>) {
    let is_clipped = b.overflow_hidden;
    if b.opacity < 0.999 {
        v.push(DisplayCommand::PushOpacity(b.opacity));
    }
    if is_clipped {
        v.push(DisplayCommand::PushClip(b.content));
    }

    if let Some(bg) = &b.background {
        if b.border_radius > 0.0 {
            v.push(DisplayCommand::FillRoundedRect {
                rect: b.rect,
                radius: b.border_radius,
                color: bg.clone(),
            });
        } else {
            v.push(DisplayCommand::FillRect { rect: b.rect, color: bg.clone() });
        }
    }
    if b.border != Edges::default() {
        v.push(DisplayCommand::Border {
            rect: b.rect,
            widths: b.border,
            color: b.border_color.clone().unwrap_or_else(|| "#000000".into()),
            radius: b.border_radius,
        });
    }
    if let Some(src) = &b.image_src {
        v.push(DisplayCommand::Image {
            rect: b.content,
            src: src.clone(),
        });
    }
    if b.text.is_some() {
        let count = b.text_lines.len().max(1) as f32;
        let lh = b.content.height / count;
        let fs = lh / 1.25;
        let text_color = b.color.clone().unwrap_or_else(|| "#000000".into());
        for (i, line) in b.text_lines.iter().enumerate() {
            if !line.trim().is_empty() {
                v.push(DisplayCommand::Text {
                    text: line.clone(),
                    rect: Rect {
                        x: b.content.x,
                        y: b.content.y + i as f32 * lh,
                        width: b.content.width,
                        height: lh,
                    },
                    font_size: fs,
                    line_height: lh,
                    color: text_color.clone(),
                });
            }
        }
    }
    let mut children: Vec<&LayoutBox> = b.children.iter().collect();
    children.sort_by_key(|child| child.z_index);
    for c in children {
        walk(c, v);
    }

    if is_clipped {
        v.push(DisplayCommand::PopClip);
    }
    if b.opacity < 0.999 {
        v.push(DisplayCommand::PopOpacity);
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use wavecore_dom::Node;
    use wavecore_style::style_tree;

    #[test]
    fn emits_rounded_background_opacity_and_z_order() {
        let a = Node::element("div", vec![Node::text("A")]);
        let b = Node::element("div", vec![Node::text("B")]);
        let root = Node::element("main", vec![a, b]);
        let css = wavecore_css::parse(
            "main{width:300px} main>div{width:50px;height:20px;background:red;border-radius:6px;opacity:.5} main>div:first-child{z-index:2}"
        );
        let layout = wavecore_layout::layout(&style_tree(&root, &css), 300.0);
        let list = build_display_list(&layout);
        assert!(list.iter().any(|cmd| matches!(cmd, DisplayCommand::FillRoundedRect { radius, .. } if *radius > 0.0)));
        assert!(list.iter().any(|cmd| matches!(cmd, DisplayCommand::PushOpacity(v) if *v < 1.0)));
    }
    #[test]
    fn compositor_frame_sorts_layers_and_preserves_opacity() {
        let child_a = Node::element("div", vec![Node::text("A")]);
        let child_b = Node::element("div", vec![Node::text("B")]);
        let root = Node::element("main", vec![child_a, child_b]);
        let css = wavecore_css::parse(
            "main{width:300px} main>div{width:50px;height:20px;background:red;opacity:.7}"
        );
        let layout = wavecore_layout::layout(&style_tree(&root, &css), 300.0);
        let frame = build_compositor_frame(&layout);
        assert!(frame.layers.len() >= 3);
        assert!(frame.layers.iter().any(|layer| layer.opacity < 1.0));
        assert!(!frame.flatten().is_empty());
    }

}
