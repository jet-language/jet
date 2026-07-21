// D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend. A real
// `JetBackend` over libgtk-4, emitted only when a Linux build constructs
// `core.ui.gtk_backend()`. All detail lives inside the module below. (Kept token
// clean above the module: the golden I1 scan matches the module by name and its
// own doc lines are stripped with it — see the comment inside.)
mod jet_gtk {
    // I1 containment: every raw C-ABI call in this module is the vetted
    // platform-FFI boundary, audited here and confined here. `tests/golden.rs`
    // strips this whole `mod jet_gtk { … }` before asserting generated Rust is
    // free of the low-level tier keyword — the same treatment the S58 C-FFI
    // wrapper modules (`user___c_*`), `jet_mem`, and the POSIX term shims get.
    // User Jet code never opts into the low-level tier. The `-lgtk-4 …` link
    // line is named by `use c.gtk4` in the application (the S59 / `pkg-config
    // gtk4` path); the calls here are the internals it links against.
    //
    // Retained-mode widget model: canonical `JetUiNode` trees reconcile by stable
    // path into real GtkBox/GtkLabel/GtkButton/GtkEntry widgets. Repaint updates
    // those widgets in place and removes stale children. The existing handle event
    // methods bind those reconciled widgets rather than creating a second tree.
    //
    // Headless safety: with `JET_UI_HEADLESS=1` or no display, `gtk_init_check`
    // is false, no widgets are created, and every op is a safe no-op — the
    // program still runs (signals update, terminal output prints) and
    // terminates, so tests are deterministic and a display-less CI never hangs.
    use super::{
        jet_ui_advance_focus, jet_ui_measure_tree, jet_ui_paint_tree,
        JetAriaRole, JetBackend, JetEventResult, JetInputEvent, JetPaintCmd, JetRect, JetShow,
        JetSize, JetSizeConstraint, JetUiNode, JetUiNodeKind,
    };
    use std::cell::RefCell;
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_void};
    use std::rc::Rc;

    #[allow(non_camel_case_types)]
    type gboolean = c_int;
    #[allow(non_camel_case_types)]
    type gpointer = *mut c_void;
    type GtkWidget = c_void;
    type GtkCssProvider = c_void;
    type GdkDisplay = c_void;
    type GMainLoop = c_void;
    type GClosure = c_void;

    const GTK_ORIENTATION_VERTICAL: c_int = 1;
    const GTK_STYLE_PROVIDER_PRIORITY_APPLICATION: c_int = 600;

    // The "clicked" GCallback trampoline signature: `void (*)(GtkButton*, gpointer)`.
    type ClickCallback = extern "C" fn(*mut GtkWidget, gpointer);

    extern "C" {
        fn gtk_init_check() -> gboolean;
        fn gtk_window_new() -> *mut GtkWidget;
        fn gtk_window_set_title(window: *mut GtkWidget, title: *const c_char);
        fn gtk_window_set_default_size(window: *mut GtkWidget, width: c_int, height: c_int);
        fn gtk_window_set_child(window: *mut GtkWidget, child: *mut GtkWidget);
        fn gtk_window_present(window: *mut GtkWidget);
        fn gtk_box_new(orientation: c_int, spacing: c_int) -> *mut GtkWidget;
        fn gtk_box_append(box_: *mut GtkWidget, child: *mut GtkWidget);
        fn gtk_box_remove(box_: *mut GtkWidget, child: *mut GtkWidget);
        fn gtk_label_new(text: *const c_char) -> *mut GtkWidget;
        fn gtk_label_set_text(label: *mut GtkWidget, text: *const c_char);
        fn gtk_button_new_with_label(label: *const c_char) -> *mut GtkWidget;
        fn gtk_button_set_label(button: *mut GtkWidget, label: *const c_char);
        fn gtk_entry_new() -> *mut GtkWidget;
        fn gtk_editable_set_text(editable: *mut GtkWidget, text: *const c_char);
        fn gtk_widget_set_size_request(widget: *mut GtkWidget, width: c_int, height: c_int);
        fn gtk_widget_add_css_class(widget: *mut GtkWidget, css_class: *const c_char);
        fn gtk_widget_remove_css_class(widget: *mut GtkWidget, css_class: *const c_char);
        fn gtk_widget_grab_focus(widget: *mut GtkWidget) -> gboolean;
        fn gtk_css_provider_new() -> *mut GtkCssProvider;
        fn gtk_css_provider_load_from_string(provider: *mut GtkCssProvider, string: *const c_char);
        fn gtk_style_context_add_provider_for_display(
            display: *mut GdkDisplay,
            provider: *mut GtkCssProvider,
            priority: c_int,
        );
        fn gdk_display_get_default() -> *mut GdkDisplay;
        fn g_signal_connect_data(
            instance: gpointer,
            detailed_signal: *const c_char,
            c_handler: ClickCallback,
            data: gpointer,
            destroy_data: Option<extern "C" fn(gpointer, *mut GClosure)>,
            connect_flags: c_int,
        ) -> u64;
        fn g_main_loop_new(context: gpointer, is_running: gboolean) -> *mut GMainLoop;
        fn g_main_loop_run(loop_: *mut GMainLoop);
        fn g_main_loop_unref(loop_: *mut GMainLoop);
        fn g_object_unref(object: gpointer);
    }

    /// Trampoline for a GTK "clicked" signal. `data` is a leaked
    /// `*const Rc<dyn Fn()>` (see `on_click`); invoking it runs the Jet handler,
    /// which typically calls `reactive.signal.set(...)` and thereby re-runs the
    /// effect that repaints the label.
    extern "C" fn jet_gtk_click_trampoline(_widget: *mut GtkWidget, data: gpointer) {
        if data.is_null() {
            return;
        }
        // SAFETY: `data` is an `Rc<dyn Fn()>` leaked in `on_click` that outlives
        // the window (owned to program exit). Borrowing it to call is sound.
        unsafe {
            let cb = &*(data as *const Rc<dyn Fn()>);
            cb();
        }
    }

    extern "C" fn jet_gtk_drop_callback(data: gpointer, _closure: *mut GClosure) {
        if data.is_null() {
            return;
        }
        // SAFETY: `on_click` allocated exactly one Box at this pointer; GTK
        // invokes this notifier once when the signal/widget is destroyed.
        unsafe {
            drop(Box::from_raw(data as *mut Rc<dyn Fn()>));
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum GtkAvailability {
        Uninitialized,
        Ready,
        HeadlessOptIn,
        UnsupportedDisplay,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum GtkWidgetKind {
        Box,
        Label,
        Button,
        Entry,
    }

    struct GtkWidgetRecord {
        path: String,
        parent: *mut GtkWidget,
        widget: *mut GtkWidget,
        kind: GtkWidgetKind,
        label: String,
        css_class: Option<String>,
    }

    // Direct widgets remain children of `vbox` for the state's whole lifetime.
    // Reconciled widgets may be removed, so their handles store only a stable
    // path and resolve the current GTK pointer at every use.
    enum GtkWidgetHandleTarget {
        Direct(*mut GtkWidget),
        TreePath(String),
    }

    struct GtkWidgetHandle {
        target: GtkWidgetHandleTarget,
        kind: GtkWidgetKind,
    }

    struct GtkState {
        // Seam parity (display-free): keeps `GtkBackend` a full `JetBackend` so
        // the null/tui measure→layout→paint path is available on it too.
        measured: Option<JetSize>,
        layout_frame: Option<JetRect>,
        commands: Vec<JetPaintCmd>,
        last_event: Option<JetEventResult>,
        focus_nodes: Vec<JetUiNode>,
        focus_paths: Vec<String>,
        focused_index: Option<usize>,
        // Retained native widgets.
        inited: bool,
        display_ok: bool,
        window: *mut GtkWidget,
        vbox: *mut GtkWidget,
        widget_handles: Vec<GtkWidgetHandle>,
        tree_widgets: Vec<GtkWidgetRecord>,
        availability: GtkAvailability,
    }

    impl GtkState {
        /// Initialize GTK once and create the window + vertical container.
        /// No-op (and `display_ok = false`) under `JET_UI_HEADLESS` or with no
        /// display, so every later widget op degrades to nothing.
        ///
        /// SAFETY: `gtk_init_check` guards all subsequent GTK calls on a live
        /// display; the window/box pointers come from GTK constructors.
        fn ensure_init(&mut self) {
            if self.inited {
                return;
            }
            self.inited = true;
            if std::env::var_os("JET_UI_HEADLESS").is_some() {
                self.display_ok = false;
                self.availability = GtkAvailability::HeadlessOptIn;
                return;
            }
            unsafe {
                if gtk_init_check() == 0 {
                    self.display_ok = false;
                    self.availability = GtkAvailability::UnsupportedDisplay;
                    return;
                }
                self.display_ok = true;
                self.availability = GtkAvailability::Ready;
                self.window = gtk_window_new();
                self.vbox = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
                gtk_window_set_child(self.window, self.vbox);
                gtk_window_set_default_size(self.window, 320, 240);
            }
        }

        fn trace(&self, message: &str) {
            if std::env::var_os("JET_UI_GTK_TRACE").is_some() {
                eprintln!("GTK_UI {message}");
            }
        }

        fn widget_kind(node: &JetUiNode) -> GtkWidgetKind {
            match node.kind {
                JetUiNodeKind::Box => GtkWidgetKind::Box,
                JetUiNodeKind::Button => GtkWidgetKind::Button,
                JetUiNodeKind::TextInput => GtkWidgetKind::Entry,
                _ => match node.role {
                    Some(JetAriaRole::Button) => GtkWidgetKind::Button,
                    Some(JetAriaRole::TextInput) => GtkWidgetKind::Entry,
                    Some(JetAriaRole::Container) => GtkWidgetKind::Box,
                    _ => GtkWidgetKind::Label,
                },
            }
        }

        fn remove_tree_widget(&mut self, index: usize) {
            let record = self.tree_widgets.remove(index);
            if self.display_ok && !record.parent.is_null() && !record.widget.is_null() {
                unsafe { gtk_box_remove(record.parent, record.widget) };
            }
            self.trace(&format!("remove {}", record.path));
        }

        fn remove_tree_subtree(&mut self, path: &str) {
            let child_prefix = format!("{path}/");
            for index in (0..self.tree_widgets.len()).rev() {
                let record_path = &self.tree_widgets[index].path;
                if record_path == path || record_path.starts_with(&child_prefix) {
                    self.remove_tree_widget(index);
                }
            }
        }

        fn reconcile_node(
            &mut self,
            node: &JetUiNode,
            parent: *mut GtkWidget,
            path: &str,
            live: &mut Vec<String>,
            focus_nodes: &mut Vec<JetUiNode>,
            focus_paths: &mut Vec<String>,
        ) {
            let kind = Self::widget_kind(node);
            if let Some(index) = self.tree_widgets.iter().position(|record| record.path == path) {
                if self.tree_widgets[index].kind != kind || self.tree_widgets[index].parent != parent {
                    self.remove_tree_subtree(path);
                }
            }
            let index = if let Some(index) = self.tree_widgets.iter().position(|record| record.path == path) {
                index
            } else {
                let text = CString::new(node.label.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
                let widget = if self.display_ok {
                    unsafe {
                        let widget = match kind {
                            GtkWidgetKind::Box => gtk_box_new(GTK_ORIENTATION_VERTICAL, 8),
                            GtkWidgetKind::Label => gtk_label_new(text.as_ptr()),
                            GtkWidgetKind::Button => gtk_button_new_with_label(text.as_ptr()),
                            GtkWidgetKind::Entry => gtk_entry_new(),
                        };
                        gtk_box_append(parent, widget);
                        widget
                    }
                } else {
                    std::ptr::null_mut()
                };
                self.tree_widgets.push(GtkWidgetRecord {
                    path: path.to_string(),
                    parent,
                    widget,
                    kind,
                    label: String::new(),
                    css_class: None,
                });
                self.trace(&format!("create {} {:?}", path, kind));
                self.tree_widgets.len() - 1
            };

            live.push(path.to_string());
            let widget = self.tree_widgets[index].widget;
            if self.display_ok && !widget.is_null() {
                let text = CString::new(node.label.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
                unsafe {
                    match kind {
                        GtkWidgetKind::Label => gtk_label_set_text(widget, text.as_ptr()),
                        GtkWidgetKind::Button => gtk_button_set_label(widget, text.as_ptr()),
                        GtkWidgetKind::Entry => gtk_editable_set_text(widget, text.as_ptr()),
                        GtkWidgetKind::Box => {}
                    }
                    gtk_widget_set_size_request(widget, node.width as c_int, node.height as c_int);
                }
            }
            self.tree_widgets[index].label = node.label.clone();
            let old_class = self.tree_widgets[index].css_class.take();
            let new_class = node.color.as_ref().map(|color| format!("jetfill{}", color.trim_start_matches('#')));
            if self.display_ok && !widget.is_null() && old_class != new_class {
                if let Some(old) = old_class.as_ref().and_then(|class| CString::new(class.as_str()).ok()) {
                    unsafe { gtk_widget_remove_css_class(widget, old.as_ptr()) };
                }
                if let (Some(color), Some(class)) = (node.color.as_ref(), new_class.as_ref()) {
                    let css = format!(".{class} {{ background-color: {color}; }}");
                    if let (Ok(cclass), Ok(ccss)) = (CString::new(class.as_str()), CString::new(css)) {
                        unsafe {
                            let provider = gtk_css_provider_new();
                            gtk_css_provider_load_from_string(provider, ccss.as_ptr());
                            let display = gdk_display_get_default();
                            if !display.is_null() {
                                gtk_style_context_add_provider_for_display(display, provider, GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
                            }
                            gtk_widget_add_css_class(widget, cclass.as_ptr());
                            g_object_unref(provider as gpointer);
                        }
                    }
                }
            }
            self.tree_widgets[index].css_class = new_class;
            self.trace(&format!("update {} {}", path, node.label));

            if node.role.as_ref().is_some_and(JetAriaRole::is_interactive) {
                focus_nodes.push(node.clone());
                focus_paths.push(path.to_string());
            }
            if kind == GtkWidgetKind::Box {
                for (child_index, child) in node.children.iter().enumerate() {
                    self.reconcile_node(
                        child,
                        widget,
                        &format!("{path}/{child_index}"),
                        live,
                        focus_nodes,
                        focus_paths,
                    );
                }
            }
        }

        fn focus_current_widget(&self) {
            let Some(path) = self.focused_index.and_then(|index| self.focus_paths.get(index)) else {
                return;
            };
            let Some(record) = self.tree_widgets.iter().find(|record| &record.path == path) else {
                return;
            };
            if self.display_ok && !record.widget.is_null() {
                unsafe { gtk_widget_grab_focus(record.widget) };
            }
            self.trace(&format!("focus {path}"));
        }

        fn resolve_widget_handle(&self, id: i64) -> Option<(*mut GtkWidget, GtkWidgetKind)> {
            let handle = self.widget_handles.get(id as usize)?;
            let widget = match &handle.target {
                GtkWidgetHandleTarget::Direct(widget) => *widget,
                GtkWidgetHandleTarget::TreePath(path) => self
                    .tree_widgets
                    .iter()
                    .find(|record| record.path == *path && record.kind == handle.kind)
                    .map(|record| record.widget)?,
            };
            (!widget.is_null()).then_some((widget, handle.kind))
        }
    }

    impl Drop for GtkState {
        fn drop(&mut self) {
            if !self.window.is_null() {
                // SAFETY: state owns its initial GTK window reference. GTK
                // tears down children and their signal destroy notifiers.
                unsafe {
                    g_object_unref(self.window as gpointer);
                }
                self.window = std::ptr::null_mut();
            }
            self.trace("cleanup");
        }
    }

    /// The native GTK4 backend. Constructing it is free; the first `label` /
    /// `button` call opens GTK and the window (when a display exists).
    #[derive(Clone)]
    pub struct JetGtkBackend {
        state: Rc<RefCell<GtkState>>,
    }

    impl JetGtkBackend {
        pub fn new() -> Self {
            JetGtkBackend {
                state: Rc::new(RefCell::new(GtkState {
                    measured: None,
                    layout_frame: None,
                    commands: Vec::new(),
                    last_event: None,
                    focus_nodes: Vec::new(),
                    focus_paths: Vec::new(),
                    focused_index: None,
                    inited: false,
                    display_ok: false,
                    window: std::ptr::null_mut(),
                    vbox: std::ptr::null_mut(),
                    widget_handles: Vec::new(),
                    tree_widgets: Vec::new(),
                    availability: GtkAvailability::Uninitialized,
                })),
            }
        }

        // ── Seam parity: display-free measure/layout/paint/on_event ──
        pub fn measure_node(&self, node: JetUiNode, constraint: JetSizeConstraint) -> JetSize {
            let mut state = self.state.borrow_mut();
            JetBackend::measure(&mut *state, &node, constraint)
        }
        pub fn layout_node(&self, node: JetUiNode, frame: JetRect) {
            let mut state = self.state.borrow_mut();
            JetBackend::layout(&mut *state, &node, frame);
        }
        pub fn paint_node(&self, node: JetUiNode) {
            let mut state = self.state.borrow_mut();
            JetBackend::paint(&mut *state, &node);
        }
        pub fn dispatch_event(&self, event: JetInputEvent) -> JetEventResult {
            let mut state = self.state.borrow_mut();
            JetBackend::on_event(&mut *state, event)
        }
        pub fn set_focus_group(&self, nodes: Vec<JetUiNode>) {
            let mut state = self.state.borrow_mut();
            state.focused_index = if nodes.is_empty() { None } else { Some(0) };
            state.focus_paths = nodes
                .iter()
                .filter_map(|node| state.tree_widgets.iter().find(|record| record.label == node.label).map(|record| record.path.clone()))
                .collect();
            state.focus_nodes = nodes;
            state.focus_current_widget();
        }
        pub fn focused_label(&self) -> String {
            let state = self.state.borrow();
            state
                .focused_index
                .and_then(|i| state.focus_nodes.get(i))
                .map(|n| n.label.clone())
                .unwrap_or_default()
        }

        // ── Retained widget API ──

        /// Create a text label, append it to the window, and return its handle.
        pub fn label(&self, text: &str) -> i64 {
            self.add_widget(text, false)
        }

        /// Create a clickable button, append it, and return its handle.
        pub fn button(&self, text: &str) -> i64 {
            self.add_widget(text, true)
        }

        fn add_widget(&self, text: &str, is_button: bool) -> i64 {
            let mut state = self.state.borrow_mut();
            state.ensure_init();
            let id = state.widget_handles.len() as i64;
            let tree_kind = if is_button { GtkWidgetKind::Button } else { GtkWidgetKind::Label };
            if let Some(path) = state
                .tree_widgets
                .iter()
                .find(|record| record.kind == tree_kind && record.label == text)
                .map(|record| record.path.clone())
            {
                state.widget_handles.push(GtkWidgetHandle {
                    target: GtkWidgetHandleTarget::TreePath(path),
                    kind: tree_kind,
                });
                state.trace(&format!("bind {text}"));
                return id;
            }
            let widget = if state.display_ok {
                let ctext = CString::new(text).unwrap_or_else(|_| CString::new("").unwrap());
                // SAFETY: display is live (ensure_init); pointers are GTK handles.
                unsafe {
                    let widget = if is_button {
                        gtk_button_new_with_label(ctext.as_ptr())
                    } else {
                        gtk_label_new(ctext.as_ptr())
                    };
                    gtk_box_append(state.vbox, widget);
                    widget
                }
            } else {
                std::ptr::null_mut()
            };
            state.widget_handles.push(GtkWidgetHandle {
                target: GtkWidgetHandleTarget::Direct(widget),
                kind: tree_kind,
            });
            id
        }

        /// Update a widget's text in place (the reactive counter's live update).
        pub fn set_text(&self, id: i64, text: &str) {
            let state = self.state.borrow();
            let Some((widget, kind)) = state.resolve_widget_handle(id) else {
                state.trace(&format!("handle-miss {id}"));
                return;
            };
            let ctext = CString::new(text).unwrap_or_else(|_| CString::new("").unwrap());
            // SAFETY: display is live and `widget` is a GTK label/button handle.
            unsafe {
                if kind == GtkWidgetKind::Button {
                    gtk_button_set_label(widget, ctext.as_ptr());
                } else {
                    gtk_label_set_text(widget, ctext.as_ptr());
                }
            }
            state.trace(&format!("handle-set-text {id} {text}"));
        }

        /// Apply a Px minimum size (D-STYLEUNIT1's `Px` reaching native layout).
        pub fn set_size(&self, id: i64, width: i64, height: i64) {
            let state = self.state.borrow();
            let Some((widget, _)) = state.resolve_widget_handle(id) else {
                state.trace(&format!("handle-miss {id}"));
                return;
            };
            // SAFETY: display is live and `widget` is a GTK widget handle.
            unsafe {
                gtk_widget_set_size_request(widget, width as c_int, height as c_int);
            }
        }

        /// Apply a `#RRGGBB` fill via a scoped CSS provider (D-STYLESHAPE1 Color
        /// reaching the native paint pipeline).
        pub fn set_color(&self, id: i64, color: &str) {
            let state = self.state.borrow();
            let Some((widget, _)) = state.resolve_widget_handle(id) else {
                state.trace(&format!("handle-miss {id}"));
                return;
            };
            let class_name = format!("jetfill{}", color.trim_start_matches('#'));
            let css = format!(".{class_name} {{ background-color: {color}; }}");
            let (Ok(cclass), Ok(ccss)) = (CString::new(class_name), CString::new(css)) else {
                return;
            };
            // SAFETY: display is live; provider/display are GTK handles.
            unsafe {
                let provider = gtk_css_provider_new();
                gtk_css_provider_load_from_string(provider, ccss.as_ptr());
                let display = gdk_display_get_default();
                if !display.is_null() {
                    gtk_style_context_add_provider_for_display(
                        display,
                        provider,
                        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION,
                    );
                }
                gtk_widget_add_css_class(widget, cclass.as_ptr());
                g_object_unref(provider as gpointer);
            }
        }

        /// Wire a button's "clicked" signal to a Jet handler.
        pub fn on_click<F: Fn() + 'static>(&self, id: i64, handler: F) {
            let state = self.state.borrow();
            let Some((widget, GtkWidgetKind::Button)) = state.resolve_widget_handle(id) else {
                state.trace(&format!("handle-miss {id}"));
                return;
            };
            let boxed: *mut Rc<dyn Fn()> = Box::into_raw(Box::new(Rc::new(handler) as Rc<dyn Fn()>));
            let signal = CString::new("clicked").unwrap();
            // SAFETY: display is live; `widget` is a GTK button; `boxed` is a
            // owned callback; GTK invokes its destroy notifier at widget teardown.
            unsafe {
                g_signal_connect_data(
                    widget as gpointer,
                    signal.as_ptr(),
                    jet_gtk_click_trampoline,
                    boxed as gpointer,
                    Some(jet_gtk_drop_callback),
                    0,
                );
            }
        }

        /// Present the window and run the GLib main loop until it closes. No-op
        /// without a display (`JET_UI_HEADLESS` / headless CI), so the program
        /// terminates instead of blocking.
        pub fn present(&self, title: &str) {
            // Read what we need, then drop the borrow BEFORE the blocking loop so
            // click handlers (`set_text`, etc.) can re-borrow the state.
            let (display_ok, window, availability) = {
                let state = self.state.borrow();
                (state.display_ok, state.window, state.availability)
            };
            if !display_ok || window.is_null() {
                if availability == GtkAvailability::UnsupportedDisplay {
                    eprintln!("UI_UNSUPPORTED[gtk.display]: no GTK display is available; set JET_UI_HEADLESS=1 only for an explicit headless run");
                }
                return;
            }
            // SAFETY: display is live and `window` is a GTK window handle.
            unsafe {
                if let Ok(ctitle) = CString::new(title) {
                    gtk_window_set_title(window, ctitle.as_ptr());
                }
                gtk_window_present(window);
                let main_loop = g_main_loop_new(std::ptr::null_mut(), 0);
                g_main_loop_run(main_loop);
                g_main_loop_unref(main_loop);
            }
        }
    }

    impl Default for JetGtkBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    impl JetBackend for GtkState {
        fn measure(&mut self, node: &JetUiNode, constraint: JetSizeConstraint) -> JetSize {
            let size = jet_ui_measure_tree(node, constraint);
            self.measured = Some(size);
            size
        }

        fn layout(&mut self, node: &JetUiNode, frame: JetRect) {
            let _ = node;
            self.layout_frame = Some(frame);
        }

        fn paint(&mut self, node: &JetUiNode) {
            let frame = self.layout_frame.unwrap_or(JetRect {
                x: 0.0,
                y: 0.0,
                width: node.width,
                height: node.height,
            });
            jet_ui_paint_tree(node, frame, &mut self.commands);
            self.ensure_init();
            let previous_focus = self
                .focused_index
                .and_then(|index| self.focus_paths.get(index))
                .cloned();
            let mut live = Vec::new();
            let mut focus_nodes = Vec::new();
            let mut focus_paths = Vec::new();
            self.reconcile_node(
                node,
                self.vbox,
                "root",
                &mut live,
                &mut focus_nodes,
                &mut focus_paths,
            );
            for index in (0..self.tree_widgets.len()).rev() {
                if !live.contains(&self.tree_widgets[index].path) {
                    self.remove_tree_widget(index);
                }
            }
            self.focused_index = previous_focus
                .as_ref()
                .and_then(|path| focus_paths.iter().position(|candidate| candidate == path))
                .or_else(|| (!focus_paths.is_empty()).then_some(0));
            self.focus_nodes = focus_nodes;
            self.focus_paths = focus_paths;
            self.focus_current_widget();
        }

        fn on_event(&mut self, event: JetInputEvent) -> JetEventResult {
            if let JetInputEvent::Key { code } = &event {
                if let Some(result) =
                    jet_ui_advance_focus(&self.focus_nodes, &mut self.focused_index, code)
                {
                    self.focus_current_widget();
                    self.last_event = Some(result);
                    return result;
                }
            }
            let result = match &event {
                JetInputEvent::Key { code } if code.is_empty() => JetEventResult::Ignored,
                JetInputEvent::Resize { size } if size.width <= 0.0 || size.height <= 0.0 => {
                    JetEventResult::Ignored
                }
                _ => JetEventResult::Handled,
            };
            self.last_event = Some(result);
            result
        }
    }

    pub fn jet_ui_gtk() -> JetGtkBackend {
        JetGtkBackend::new()
    }
}

pub use jet_gtk::{jet_ui_gtk, JetGtkBackend};
