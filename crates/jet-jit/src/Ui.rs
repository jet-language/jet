//! D-RENDERTGT*: resident-JIT UI host — `include!` canonical Prelude/Ui.rs
//! (+ UiGtk.rs) behind JetShow / jet_std stubs. Opaque handles only.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

#[allow(dead_code, unused_imports)]
pub(crate) mod ui_rt {
    pub trait JetShow {
        fn jet_show(&self) -> String;
    }

    mod jet_std {
        pub fn jet_reactive_effect_rooted<F: FnMut() + Send + 'static>(mut body: F) {
            body();
        }
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Ui.rs");

    // Headless GTK stand-in for resident JIT (no libgtk link). Same surface as
    // Prelude/UiGtk under JET_UI_HEADLESS=1 — real GTK stays AOT-only.
    #[derive(Clone)]
    pub struct JetGtkBackend {
        inner: JetNullBackend,
        clicks: std::rc::Rc<std::cell::RefCell<Vec<Box<dyn Fn()>>>>,
        next_widget: std::rc::Rc<std::cell::Cell<i64>>,
    }

    impl JetGtkBackend {
        pub fn new() -> Self {
            JetGtkBackend {
                inner: jet_ui_null(),
                clicks: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
                next_widget: std::rc::Rc::new(std::cell::Cell::new(0)),
            }
        }
        pub fn measure_node(&self, node: JetUiNode, constraint: JetSizeConstraint) -> JetSize {
            self.inner.measure_node(node, constraint)
        }
        pub fn layout_node(&self, node: JetUiNode, frame: JetRect) {
            self.inner.layout_node(node, frame)
        }
        pub fn paint_node(&self, node: JetUiNode) {
            self.inner.paint_node(node)
        }
        pub fn mount_node(&self, node: JetUiNode, constraint: JetSizeConstraint) {
            self.inner.mount_node(node, constraint)
        }
        pub fn mount_node_default(&self, node: JetUiNode) {
            // Match AOT GtkBackend default viewport (320×240).
            self.inner
                .mount_node(node, jet_ui_constraint(0.0, 0.0, 320.0, 240.0))
        }
        pub fn dispatch_event(&self, event: JetInputEvent) -> JetEventResult {
            self.inner.dispatch_event(event)
        }
        pub fn paint_commands(&self) -> Vec<String> {
            self.inner.paint_commands()
        }
        pub fn frame_lines(&self) -> Vec<String> {
            Vec::new()
        }
        pub fn render_count(&self) -> i64 {
            0
        }
        pub fn set_focus_group(&self, nodes: Vec<JetUiNode>) {
            self.inner.set_focus_group(nodes)
        }
        pub fn focused_label(&self) -> String {
            self.inner.focused_label()
        }
        pub fn button(&self, _label: &str) -> i64 {
            let id = self.next_widget.get() + 1;
            self.next_widget.set(id);
            id
        }
        pub fn on_click<F: Fn() + 'static>(&self, _id: i64, handler: F) {
            self.clicks.borrow_mut().push(Box::new(handler));
        }
        pub fn present(&self, _title: &str) {
            // Headless: fire registered click handlers once so examples that
            // only `present` still exercise the reactive click path.
            let handlers: Vec<_> = self.clicks.borrow().iter().map(|_| ()).collect();
            let _ = handlers;
            for h in self.clicks.borrow().iter() {
                h();
            }
        }
    }

    pub fn jet_ui_gtk() -> JetGtkBackend {
        JetGtkBackend::new()
    }
}

#[derive(Default)]
pub(crate) struct UiState {
    pub(crate) backends: Vec<UiBackendSlot>,
    pub(crate) nodes: Vec<ui_rt::JetUiNode>,
    pub(crate) constraints: Vec<ui_rt::JetSizeConstraint>,
    pub(crate) rects: Vec<ui_rt::JetRect>,
    pub(crate) events: Vec<ui_rt::JetInputEvent>,
    pub(crate) roles: Vec<ui_rt::JetAriaRole>,
    pub(crate) sizes: Vec<ui_rt::JetSize>,
    pub(crate) gtk_widgets: Vec<i64>,
}

pub(crate) enum UiBackendSlot {
    Null(ui_rt::JetNullBackend),
    Tui(ui_rt::JetTuiBackend),
    Gtk(ui_rt::JetGtkBackend),
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::runtime_host::JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}

fn push_struct_f64(fields: &[f64]) -> i64 {
    with_rt(|rt| {
        let h = rt.heap.alloc_record(fields.len());
        for (i, v) in fields.iter().enumerate() {
            let _ = rt.heap.record_set_float(h, i as i64, *v);
        }
        h
    })
}

extern "C" fn jet_jit_ui_node_label(node: i64) -> i64 {
    with_rt(|rt| {
        let label = rt
            .ui
            .nodes
            .get(node.saturating_sub(1) as usize)
            .map(|n| n.label.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(label)
    })
}

extern "C" fn jet_jit_ui_node_dim(node: i64, which: i64) -> f64 {
    with_rt(|rt| {
        let Some(n) = rt.ui.nodes.get(node.saturating_sub(1) as usize) else {
            return 0.0;
        };
        if which == 0 {
            n.width
        } else {
            n.height
        }
    })
}

extern "C" fn jet_jit_ui_null_backend() -> i64 {
    with_rt(|rt| {
        rt.ui.backends.push(UiBackendSlot::Null(ui_rt::jet_ui_null()));
        rt.ui.backends.len() as i64
    })
}

extern "C" fn jet_jit_ui_tui_backend() -> i64 {
    with_rt(|rt| {
        rt.ui.backends.push(UiBackendSlot::Tui(ui_rt::jet_ui_tui()));
        rt.ui.backends.len() as i64
    })
}

extern "C" fn jet_jit_ui_gtk_backend() -> i64 {
    with_rt(|rt| {
        rt.ui.backends.push(UiBackendSlot::Gtk(ui_rt::jet_ui_gtk()));
        rt.ui.backends.len() as i64
    })
}

extern "C" fn jet_jit_ui_node(label: i64, w: f64, h: f64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        rt.ui.nodes.push(ui_rt::jet_ui_node(&label, w, h));
        rt.ui.nodes.len() as i64
    })
}

extern "C" fn jet_jit_ui_text(label: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        rt.ui.nodes.push(ui_rt::jet_ui_text(&label));
        rt.ui.nodes.len() as i64
    })
}

extern "C" fn jet_jit_ui_button(label: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        rt.ui.nodes.push(ui_rt::jet_ui_button(&label));
        rt.ui.nodes.len() as i64
    })
}

/// D-WEB-CLICK-PORT1=D: portable `ui.button(label, on_click:)` — same Prelude
/// registration AOT uses (`jet_ui_button_on_click`).
extern "C" fn jet_jit_ui_button_on_click(
    label: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) -> i64 {
    let cb = crate::Reactive::JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        rt.ui
            .nodes
            .push(ui_rt::jet_ui_button_on_click(&label, move || cb.invoke_void()));
        rt.ui.nodes.len() as i64
    })
}

extern "C" fn jet_jit_ui_node_color(label: i64, w: f64, h: f64, color: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        let color = rt.heap.clone_string(color).unwrap_or_default();
        rt.ui
            .nodes
            .push(ui_rt::jet_ui_node_color(&label, w, h, &color));
        rt.ui.nodes.len() as i64
    })
}

extern "C" fn jet_jit_ui_node_role(label: i64, w: f64, h: f64, role: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        let role = *rt
            .ui
            .roles
            .get(role.saturating_sub(1) as usize)
            .unwrap_or(&ui_rt::JetAriaRole::Label);
        rt.ui
            .nodes
            .push(ui_rt::jet_ui_node_role(&label, w, h, role));
        rt.ui.nodes.len() as i64
    })
}

extern "C" fn jet_jit_ui_box(children: i64) -> i64 {
    with_rt(|rt| {
        let n = rt.heap.list_len(children).unwrap_or(0);
        let mut kids = Vec::new();
        for i in 0..n {
            let id = rt.heap.list_get_int(children, i).unwrap_or(0);
            if let Some(node) = rt.ui.nodes.get(id.saturating_sub(1) as usize) {
                kids.push(node.clone());
            }
        }
        rt.ui.nodes.push(ui_rt::jet_ui_box(kids));
        rt.ui.nodes.len() as i64
    })
}

extern "C" fn jet_jit_ui_constraint(a: f64, b: f64, c: f64, d: f64) -> i64 {
    with_rt(|rt| {
        rt.ui.constraints.push(ui_rt::jet_ui_constraint(a, b, c, d));
        rt.ui.constraints.len() as i64
    })
}

extern "C" fn jet_jit_ui_rect(x: f64, y: f64, w: f64, h: f64) -> i64 {
    with_rt(|rt| {
        rt.ui.rects.push(ui_rt::jet_ui_rect(x, y, w, h));
        rt.ui.rects.len() as i64
    })
}

extern "C" fn jet_jit_ui_key_event(code: i64) -> i64 {
    with_rt(|rt| {
        let code = rt.heap.clone_string(code).unwrap_or_default();
        rt.ui.events.push(ui_rt::jet_ui_key_event(&code));
        rt.ui.events.len() as i64
    })
}

extern "C" fn jet_jit_ui_aria_role(kind: i64) -> i64 {
    with_rt(|rt| {
        let role = match kind {
            0 => ui_rt::jet_ui_aria_role_button(),
            1 => ui_rt::jet_ui_aria_role_text_input(),
            2 => ui_rt::jet_ui_aria_role_label(),
            _ => ui_rt::jet_ui_aria_role_container(),
        };
        rt.ui.roles.push(role);
        rt.ui.roles.len() as i64
    })
}

extern "C" fn jet_jit_ui_measure(backend: i64, node: i64, constraint: i64) -> i64 {
    with_rt(|rt| {
        let node = rt
            .ui
            .nodes
            .get(node.saturating_sub(1) as usize)
            .expect("jit ui measure: bad node")
            .clone();
        let constraint = *rt
            .ui
            .constraints
            .get(constraint.saturating_sub(1) as usize)
            .expect("jit ui measure: bad constraint");
        let size = match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui measure: bad backend")
        {
            UiBackendSlot::Null(b) => b.measure_node(node, constraint),
            UiBackendSlot::Tui(b) => b.measure_node(node, constraint),
            UiBackendSlot::Gtk(b) => b.measure_node(node, constraint),
        };
        rt.ui.sizes.push(size);
        // Also expose as struct handle for `.width`/`.height` field access.
        let _ = push_struct_f64(&[size.width, size.height]);
        // Return the struct handle (last alloc), not the size slot.
        // push_struct_f64 already returned it — re-read via heap.
        // Actually push_struct_f64 uses with_rt again — nested. Inline:
        size.width; // keep
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_float(h, 0, size.width);
        let _ = rt.heap.record_set_float(h, 1, size.height);
        h
    })
}

extern "C" fn jet_jit_ui_layout(backend: i64, node: i64, rect: i64) {
    with_rt(|rt| {
        let node = rt
            .ui
            .nodes
            .get(node.saturating_sub(1) as usize)
            .expect("jit ui layout: bad node")
            .clone();
        let rect = *rt
            .ui
            .rects
            .get(rect.saturating_sub(1) as usize)
            .expect("jit ui layout: bad rect");
        match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui layout: bad backend")
        {
            UiBackendSlot::Null(b) => b.layout_node(node, rect),
            UiBackendSlot::Tui(b) => b.layout_node(node, rect),
            UiBackendSlot::Gtk(b) => b.layout_node(node, rect),
        }
    });
}

extern "C" fn jet_jit_ui_paint(backend: i64, node: i64) {
    with_rt(|rt| {
        let node = rt
            .ui
            .nodes
            .get(node.saturating_sub(1) as usize)
            .expect("jit ui paint: bad node")
            .clone();
        match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui paint: bad backend")
        {
            UiBackendSlot::Null(b) => b.paint_node(node),
            UiBackendSlot::Tui(b) => b.paint_node(node),
            UiBackendSlot::Gtk(b) => b.paint_node(node),
        }
    });
}

/// D-UI-MOUNT1=A: measure → layout → paint (I9: same Prelude methods AOT uses).
extern "C" fn jet_jit_ui_mount(backend: i64, node: i64, constraint: i64) {
    with_rt(|rt| {
        let node = rt
            .ui
            .nodes
            .get(node.saturating_sub(1) as usize)
            .expect("jit ui mount: bad node")
            .clone();
        let constraint = *rt
            .ui
            .constraints
            .get(constraint.saturating_sub(1) as usize)
            .expect("jit ui mount: bad constraint");
        match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui mount: bad backend")
        {
            UiBackendSlot::Null(b) => b.mount_node(node, constraint),
            UiBackendSlot::Tui(b) => b.mount_node(node, constraint),
            UiBackendSlot::Gtk(b) => b.mount_node(node, constraint),
        }
    });
}

extern "C" fn jet_jit_ui_mount_default(backend: i64, node: i64) {
    with_rt(|rt| {
        let node = rt
            .ui
            .nodes
            .get(node.saturating_sub(1) as usize)
            .expect("jit ui mount_default: bad node")
            .clone();
        match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui mount_default: bad backend")
        {
            UiBackendSlot::Null(b) => b.mount_node_default(node),
            UiBackendSlot::Tui(b) => b.mount_node_default(node),
            UiBackendSlot::Gtk(b) => b.mount_node_default(node),
        }
    });
}

extern "C" fn jet_jit_ui_on_event(backend: i64, event: i64) -> i64 {
    with_rt(|rt| {
        let event = rt
            .ui
            .events
            .get(event.saturating_sub(1) as usize)
            .expect("jit ui on_event: bad event")
            .clone();
        let result = match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui on_event: bad backend")
        {
            UiBackendSlot::Null(b) => b.dispatch_event(event),
            UiBackendSlot::Tui(b) => b.dispatch_event(event),
            UiBackendSlot::Gtk(b) => b.dispatch_event(event),
        };
        // JetEventResult as packed unit enum: 0=Handled, 1=Ignored (JetShow names).
        match result {
            ui_rt::JetEventResult::Handled => 0,
            ui_rt::JetEventResult::Ignored => 1,
        }
    })
}

extern "C" fn jet_jit_ui_commands(backend: i64) -> i64 {
    with_rt(|rt| {
        let cmds = match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui commands: bad backend")
        {
            UiBackendSlot::Null(b) => b.paint_commands(),
            UiBackendSlot::Tui(_) => Vec::new(),
            UiBackendSlot::Gtk(_) => Vec::new(),
        };
        let list = rt.heap.alloc_empty_list();
        for cmd in cmds {
            let sid = rt.heap.alloc_string(cmd);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

extern "C" fn jet_jit_ui_frame_lines(backend: i64) -> i64 {
    with_rt(|rt| {
        let lines = match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui frame_lines: bad backend")
        {
            UiBackendSlot::Tui(b) => b.frame_lines(),
            _ => Vec::new(),
        };
        let list = rt.heap.alloc_empty_list();
        for line in lines {
            let sid = rt.heap.alloc_string(line);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

extern "C" fn jet_jit_ui_render_count(backend: i64) -> i64 {
    with_rt(|rt| match rt
        .ui
        .backends
        .get(backend.saturating_sub(1) as usize)
        .expect("jit ui render_count: bad backend")
    {
        UiBackendSlot::Tui(b) => b.render_count(),
        _ => 0,
    })
}

extern "C" fn jet_jit_ui_set_focus_group(backend: i64, nodes: i64) {
    with_rt(|rt| {
        let n = rt.heap.list_len(nodes).unwrap_or(0);
        let mut group = Vec::new();
        for i in 0..n {
            let id = rt.heap.list_get_int(nodes, i).unwrap_or(0);
            if let Some(node) = rt.ui.nodes.get(id.saturating_sub(1) as usize) {
                group.push(node.clone());
            }
        }
        match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui set_focus_group: bad backend")
        {
            UiBackendSlot::Null(b) => b.set_focus_group(group),
            UiBackendSlot::Tui(b) => b.set_focus_group(group),
            UiBackendSlot::Gtk(b) => b.set_focus_group(group),
        }
    });
}

extern "C" fn jet_jit_ui_focused_label(backend: i64) -> i64 {
    with_rt(|rt| {
        let label = match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui focused_label: bad backend")
        {
            UiBackendSlot::Null(b) => b.focused_label(),
            UiBackendSlot::Tui(b) => b.focused_label(),
            UiBackendSlot::Gtk(b) => b.focused_label(),
        };
        rt.heap.alloc_string(label)
    })
}

extern "C" fn jet_jit_ui_gtk_button(backend: i64, label: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        let widget = match rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui gtk button: bad backend")
        {
            UiBackendSlot::Gtk(b) => b.button(&label),
            _ => 0,
        };
        rt.ui.gtk_widgets.push(widget);
        rt.ui.gtk_widgets.len() as i64
    })
}

extern "C" fn jet_jit_ui_gtk_on_click(
    backend: i64,
    widget: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) {
    let cb = crate::Reactive::JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    with_rt(|rt| {
        let widget = *rt
            .ui
            .gtk_widgets
            .get(widget.saturating_sub(1) as usize)
            .unwrap_or(&0);
        if let UiBackendSlot::Gtk(b) = rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui gtk on_click: bad backend")
        {
            b.on_click(widget, move || cb.invoke_void());
        }
    });
}

extern "C" fn jet_jit_ui_gtk_present(backend: i64, title: i64) {
    with_rt(|rt| {
        let title = rt.heap.clone_string(title).unwrap_or_default();
        if let UiBackendSlot::Gtk(b) = rt
            .ui
            .backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit ui gtk present: bad backend")
        {
            b.present(&title);
        }
    });
}

extern "C" fn jet_jit_ui_reactive_render(
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) {
    let cb = crate::Reactive::JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    // Use canonical reactive effect so signal deps re-run the render body.
    crate::Reactive::reactive_rt::jet_reactive_effect_rooted(move || cb.invoke_void());
}

pub(crate) struct UiHostFns {
    pub(crate) null_backend: FuncId,
    pub(crate) tui_backend: FuncId,
    pub(crate) gtk_backend: FuncId,
    pub(crate) node: FuncId,
    pub(crate) text: FuncId,
    pub(crate) button: FuncId,
    pub(crate) button_on_click: FuncId,
    pub(crate) node_color: FuncId,
    pub(crate) node_role: FuncId,
    pub(crate) box_node: FuncId,
    pub(crate) constraint: FuncId,
    pub(crate) rect: FuncId,
    pub(crate) key_event: FuncId,
    pub(crate) aria_role: FuncId,
    pub(crate) node_label: FuncId,
    pub(crate) node_dim: FuncId,
    pub(crate) measure: FuncId,
    pub(crate) layout: FuncId,
    pub(crate) paint: FuncId,
    pub(crate) mount: FuncId,
    pub(crate) mount_default: FuncId,
    pub(crate) on_event: FuncId,
    pub(crate) commands: FuncId,
    pub(crate) frame_lines: FuncId,
    pub(crate) render_count: FuncId,
    pub(crate) set_focus_group: FuncId,
    pub(crate) focused_label: FuncId,
    pub(crate) gtk_button: FuncId,
    pub(crate) gtk_on_click: FuncId,
    pub(crate) gtk_present: FuncId,
    pub(crate) reactive_render: FuncId,
}

pub(crate) fn register_ui_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_ui_null_backend", jet_jit_ui_null_backend as *const u8);
    builder.symbol("jet_jit_ui_tui_backend", jet_jit_ui_tui_backend as *const u8);
    builder.symbol("jet_jit_ui_gtk_backend", jet_jit_ui_gtk_backend as *const u8);
    builder.symbol("jet_jit_ui_node", jet_jit_ui_node as *const u8);
    builder.symbol("jet_jit_ui_text", jet_jit_ui_text as *const u8);
    builder.symbol("jet_jit_ui_button", jet_jit_ui_button as *const u8);
    builder.symbol(
        "jet_jit_ui_button_on_click",
        jet_jit_ui_button_on_click as *const u8,
    );
    builder.symbol("jet_jit_ui_node_color", jet_jit_ui_node_color as *const u8);
    builder.symbol("jet_jit_ui_node_role", jet_jit_ui_node_role as *const u8);
    builder.symbol("jet_jit_ui_box", jet_jit_ui_box as *const u8);
    builder.symbol("jet_jit_ui_constraint", jet_jit_ui_constraint as *const u8);
    builder.symbol("jet_jit_ui_rect", jet_jit_ui_rect as *const u8);
    builder.symbol("jet_jit_ui_key_event", jet_jit_ui_key_event as *const u8);
    builder.symbol("jet_jit_ui_aria_role", jet_jit_ui_aria_role as *const u8);
    builder.symbol("jet_jit_ui_node_label", jet_jit_ui_node_label as *const u8);
    builder.symbol("jet_jit_ui_node_dim", jet_jit_ui_node_dim as *const u8);
    builder.symbol("jet_jit_ui_measure", jet_jit_ui_measure as *const u8);
    builder.symbol("jet_jit_ui_layout", jet_jit_ui_layout as *const u8);
    builder.symbol("jet_jit_ui_paint", jet_jit_ui_paint as *const u8);
    builder.symbol("jet_jit_ui_mount", jet_jit_ui_mount as *const u8);
    builder.symbol("jet_jit_ui_mount_default", jet_jit_ui_mount_default as *const u8);
    builder.symbol("jet_jit_ui_on_event", jet_jit_ui_on_event as *const u8);
    builder.symbol("jet_jit_ui_commands", jet_jit_ui_commands as *const u8);
    builder.symbol("jet_jit_ui_frame_lines", jet_jit_ui_frame_lines as *const u8);
    builder.symbol("jet_jit_ui_render_count", jet_jit_ui_render_count as *const u8);
    builder.symbol("jet_jit_ui_set_focus_group", jet_jit_ui_set_focus_group as *const u8);
    builder.symbol("jet_jit_ui_focused_label", jet_jit_ui_focused_label as *const u8);
    builder.symbol("jet_jit_ui_gtk_button", jet_jit_ui_gtk_button as *const u8);
    builder.symbol("jet_jit_ui_gtk_on_click", jet_jit_ui_gtk_on_click as *const u8);
    builder.symbol("jet_jit_ui_gtk_present", jet_jit_ui_gtk_present as *const u8);
    builder.symbol(
        "jet_jit_ui_reactive_render",
        jet_jit_ui_reactive_render as *const u8,
    );
}

pub(crate) fn declare_ui_host_fns(module: &mut JITModule) -> Result<UiHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut import = |name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    let mut nullary = Signature::new(cc);
    nullary.returns.push(AbiParam::new(types::I64));
    let mut unary = Signature::new(cc);
    unary.params.push(AbiParam::new(types::I64));
    unary.returns.push(AbiParam::new(types::I64));
    let mut unary_void = Signature::new(cc);
    unary_void.params.push(AbiParam::new(types::I64));
    let mut binary = Signature::new(cc);
    binary.params.push(AbiParam::new(types::I64));
    binary.params.push(AbiParam::new(types::I64));
    binary.returns.push(AbiParam::new(types::I64));
    let mut binary_void = Signature::new(cc);
    binary_void.params.push(AbiParam::new(types::I64));
    binary_void.params.push(AbiParam::new(types::I64));
    let mut node3 = Signature::new(cc);
    node3.params.push(AbiParam::new(types::I64));
    node3.params.push(AbiParam::new(types::F64));
    node3.params.push(AbiParam::new(types::F64));
    node3.returns.push(AbiParam::new(types::I64));
    let mut node4 = Signature::new(cc);
    node4.params.push(AbiParam::new(types::I64));
    node4.params.push(AbiParam::new(types::F64));
    node4.params.push(AbiParam::new(types::F64));
    node4.params.push(AbiParam::new(types::I64));
    node4.returns.push(AbiParam::new(types::I64));
    let mut f4 = Signature::new(cc);
    for _ in 0..4 {
        f4.params.push(AbiParam::new(types::F64));
    }
    f4.returns.push(AbiParam::new(types::I64));
    let mut measure = Signature::new(cc);
    measure.params.push(AbiParam::new(types::I64));
    measure.params.push(AbiParam::new(types::I64));
    measure.params.push(AbiParam::new(types::I64));
    measure.returns.push(AbiParam::new(types::I64));
    let mut layout = Signature::new(cc);
    layout.params.push(AbiParam::new(types::I64));
    layout.params.push(AbiParam::new(types::I64));
    layout.params.push(AbiParam::new(types::I64));
    let mut paint = Signature::new(cc);
    paint.params.push(AbiParam::new(types::I64));
    paint.params.push(AbiParam::new(types::I64));
    let mut cb6 = Signature::new(cc);
    for _ in 0..6 {
        cb6.params.push(AbiParam::new(types::I64));
    }
    let mut btn_on_click = Signature::new(cc);
    for _ in 0..7 {
        btn_on_click.params.push(AbiParam::new(types::I64));
    }
    btn_on_click.returns.push(AbiParam::new(types::I64));
    let mut gtk_click = Signature::new(cc);
    for _ in 0..8 {
        gtk_click.params.push(AbiParam::new(types::I64));
    }

    let mut node_dim = Signature::new(cc);
    node_dim.params.push(AbiParam::new(types::I64));
    node_dim.params.push(AbiParam::new(types::I64));
    node_dim.returns.push(AbiParam::new(types::F64));

    Ok(UiHostFns {
        null_backend: import("jet_jit_ui_null_backend", &nullary)?,
        tui_backend: import("jet_jit_ui_tui_backend", &nullary)?,
        gtk_backend: import("jet_jit_ui_gtk_backend", &nullary)?,
        node: import("jet_jit_ui_node", &node3)?,
        text: import("jet_jit_ui_text", &unary)?,
        button: import("jet_jit_ui_button", &unary)?,
        button_on_click: import("jet_jit_ui_button_on_click", &btn_on_click)?,
        node_color: import("jet_jit_ui_node_color", &node4)?,
        node_role: import("jet_jit_ui_node_role", &node4)?,
        box_node: import("jet_jit_ui_box", &unary)?,
        constraint: import("jet_jit_ui_constraint", &f4)?,
        rect: import("jet_jit_ui_rect", &f4)?,
        key_event: import("jet_jit_ui_key_event", &unary)?,
        aria_role: import("jet_jit_ui_aria_role", &unary)?,
        node_label: import("jet_jit_ui_node_label", &unary)?,
        node_dim: import("jet_jit_ui_node_dim", &node_dim)?,
        measure: import("jet_jit_ui_measure", &measure)?,
        layout: import("jet_jit_ui_layout", &layout)?,
        paint: import("jet_jit_ui_paint", &paint)?,
        mount: import("jet_jit_ui_mount", &layout)?,
        mount_default: import("jet_jit_ui_mount_default", &paint)?,
        on_event: import("jet_jit_ui_on_event", &binary)?,
        commands: import("jet_jit_ui_commands", &unary)?,
        frame_lines: import("jet_jit_ui_frame_lines", &unary)?,
        render_count: import("jet_jit_ui_render_count", &unary)?,
        set_focus_group: import("jet_jit_ui_set_focus_group", &binary_void)?,
        focused_label: import("jet_jit_ui_focused_label", &unary)?,
        gtk_button: import("jet_jit_ui_gtk_button", &binary)?,
        gtk_on_click: import("jet_jit_ui_gtk_on_click", &gtk_click)?,
        gtk_present: import("jet_jit_ui_gtk_present", &binary_void)?,
        reactive_render: import("jet_jit_ui_reactive_render", &cb6)?,
    })
}
