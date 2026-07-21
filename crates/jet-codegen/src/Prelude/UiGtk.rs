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
    // Retained-mode widget model: `label`/`button` create real GtkWidgets and
    // return an integer handle; `set_text`/`set_size`/`set_color` mutate a live
    // widget; `on_click` wires a button's "clicked" signal to a Jet closure. A
    // reactive effect (`ui.reactive_render`) that calls `set_text` on every
    // signal change is what makes a counter update on the screen — the button's
    // click sets the signal, the effect re-runs, and the GtkLabel's text is
    // updated in place.
    //
    // Headless safety: with `JET_UI_HEADLESS=1` or no display, `gtk_init_check`
    // is false, no widgets are created, and every op is a safe no-op — the
    // program still runs (signals update, terminal output prints) and
    // terminates, so tests are deterministic and a display-less CI never hangs.
    use super::{
        jet_ui_advance_focus, jet_ui_collect_focus, jet_ui_measure_tree, jet_ui_paint_tree,
        JetBackend, JetEventResult, JetInputEvent, JetPaintCmd, JetRect, JetShow, JetSize,
        JetSizeConstraint, JetUiNode,
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
        fn gtk_label_new(text: *const c_char) -> *mut GtkWidget;
        fn gtk_label_set_text(label: *mut GtkWidget, text: *const c_char);
        fn gtk_button_new_with_label(label: *const c_char) -> *mut GtkWidget;
        fn gtk_button_set_label(button: *mut GtkWidget, label: *const c_char);
        fn gtk_widget_set_size_request(widget: *mut GtkWidget, width: c_int, height: c_int);
        fn gtk_widget_add_css_class(widget: *mut GtkWidget, css_class: *const c_char);
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

    struct GtkState {
        // Seam parity (display-free): keeps `GtkBackend` a full `JetBackend` so
        // the null/tui measure→layout→paint path is available on it too.
        measured: Option<JetSize>,
        layout_frame: Option<JetRect>,
        commands: Vec<JetPaintCmd>,
        last_event: Option<JetEventResult>,
        focus_nodes: Vec<JetUiNode>,
        focused_index: Option<usize>,
        // Retained native widgets.
        inited: bool,
        display_ok: bool,
        window: *mut GtkWidget,
        vbox: *mut GtkWidget,
        widgets: Vec<*mut GtkWidget>,
        is_button: Vec<bool>,
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
                    focused_index: None,
                    inited: false,
                    display_ok: false,
                    window: std::ptr::null_mut(),
                    vbox: std::ptr::null_mut(),
                    widgets: Vec::new(),
                    is_button: Vec::new(),
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
            state.focus_nodes = nodes;
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
            let id = state.widgets.len() as i64;
            if state.display_ok {
                let ctext = CString::new(text).unwrap_or_else(|_| CString::new("").unwrap());
                // SAFETY: display is live (ensure_init); pointers are GTK handles.
                unsafe {
                    let widget = if is_button {
                        gtk_button_new_with_label(ctext.as_ptr())
                    } else {
                        gtk_label_new(ctext.as_ptr())
                    };
                    gtk_box_append(state.vbox, widget);
                    state.widgets.push(widget);
                }
            } else {
                state.widgets.push(std::ptr::null_mut());
            }
            state.is_button.push(is_button);
            id
        }

        /// Update a widget's text in place (the reactive counter's live update).
        pub fn set_text(&self, id: i64, text: &str) {
            let state = self.state.borrow();
            let Some(&widget) = state.widgets.get(id as usize) else {
                return;
            };
            if !state.display_ok || widget.is_null() {
                return;
            }
            let ctext = CString::new(text).unwrap_or_else(|_| CString::new("").unwrap());
            let is_button = state.is_button.get(id as usize).copied().unwrap_or(false);
            // SAFETY: display is live and `widget` is a GTK label/button handle.
            unsafe {
                if is_button {
                    gtk_button_set_label(widget, ctext.as_ptr());
                } else {
                    gtk_label_set_text(widget, ctext.as_ptr());
                }
            }
        }

        /// Apply a Px minimum size (D-STYLEUNIT1's `Px` reaching native layout).
        pub fn set_size(&self, id: i64, width: i64, height: i64) {
            let state = self.state.borrow();
            let Some(&widget) = state.widgets.get(id as usize) else {
                return;
            };
            if !state.display_ok || widget.is_null() {
                return;
            }
            // SAFETY: display is live and `widget` is a GTK widget handle.
            unsafe {
                gtk_widget_set_size_request(widget, width as c_int, height as c_int);
            }
        }

        /// Apply a `#RRGGBB` fill via a scoped CSS provider (D-STYLESHAPE1 Color
        /// reaching the native paint pipeline).
        pub fn set_color(&self, id: i64, color: &str) {
            let state = self.state.borrow();
            let Some(&widget) = state.widgets.get(id as usize) else {
                return;
            };
            if !state.display_ok || widget.is_null() {
                return;
            }
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
            let Some(&widget) = state.widgets.get(id as usize) else {
                return;
            };
            if !state.display_ok || widget.is_null() {
                return;
            }
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
            let mut focus = Vec::new();
            jet_ui_collect_focus(node, &mut focus);
            if !focus.is_empty() {
                self.focused_index = Some(0);
                self.focus_nodes = focus;
            }
        }

        fn on_event(&mut self, event: JetInputEvent) -> JetEventResult {
            if let JetInputEvent::Key { code } = &event {
                if let Some(result) =
                    jet_ui_advance_focus(&self.focus_nodes, &mut self.focused_index, code)
                {
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
