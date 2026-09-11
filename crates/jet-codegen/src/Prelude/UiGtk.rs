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
    // The loader below uses `dlopen`/`dlsym`, so ordinary programs do not
    // link-load GTK. Native GTK is opened only when this backend is used and
    // the display path is initialized.
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
        jet_font_shape_with_fallback, jet_font_vetted_bytes, jet_ui_accessibility_attach,
        jet_ui_accessibility_project, jet_ui_advance_focus, jet_ui_bind_tree_clicks,
        jet_ui_constraint, jet_ui_dispatch, jet_ui_measure_tree, jet_ui_node_id,
        jet_ui_paint_tree, jet_ui_rect, JetAriaRole,
        JetBackend, JetEventResult, JetFontFace, JetGlyphRun, JetInputEvent, JetPaintCmd, JetRect,
        JetShow, JetSize, JetSizeConstraint, JetUiAccessibility, JetUiAccessibilityProjection,
        JetUiAccessibilityTarget, JetUiCapability, JetUiCapabilityFacts, JetUiCancellation,
        JetUiClipboardText, JetUiClipboardWrite, JetUiDragEvent, JetUiDragOperation,
        JetUiDragPhase, JetUiDropItem, JetUiFileDialogKind, JetUiFileDialogRequest,
        JetUiFileDialogSelection, JetUiFsAccess, JetUiHost, JetUiHostError, JetUiImeComposition,
        JetUiImeEvent, JetUiImeMode, JetUiImePhase, JetUiNode, JetUiNodeId, JetUiNodeKind,
        JetUiServiceResult,
        JetUiShortcut, JetUiShortcutBinding, JetUiShortcutDispatch, JetUiShortcutRegistry,
        JetUiTextRange,
    };
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_double, c_int, c_void};
    use std::sync::{Arc, LazyLock, Mutex};

    #[allow(non_camel_case_types)]
    type gboolean = c_int;
    #[allow(non_camel_case_types)]
    type gpointer = *mut c_void;
    type GtkWidget = c_void;
    type GtkCssProvider = c_void;
    type GtkEventController = c_void;
    type GtkDropTarget = c_void;
    type GtkAccessible = c_void;
    type GdkDisplay = c_void;
    type GdkClipboard = c_void;
    type GtkFileDialog = c_void;
    type GtkFileFilter = c_void;
    type GListStore = c_void;
    type GListModel = c_void;
    type GFile = c_void;
    type GValue = c_void;
    type GAsyncResult = c_void;
    type GError = c_void;
    type GMainLoop = c_void;
    type GClosure = c_void;
    type GType = usize;
    #[repr(C)]
    struct NativeGError {
        domain: u32,
        code: c_int,
        message: *mut c_char,
    }

    const G_IO_ERROR_CANCELLED: c_int = 19;
    const G_IO_ERROR_QUARK: &[u8] = b"g-io-error-quark\0";
    const GTK_ORIENTATION_VERTICAL: c_int = 1;
    const GTK_STYLE_PROVIDER_PRIORITY_APPLICATION: c_int = 600;
    // GtkAccessibleProperty values are ABI-stable enum entries in GTK4:
    // DESCRIPTION is 1 and LABEL is 4 (gtk/gtkenums.h).
    const GTK_ACCESSIBLE_PROPERTY_DESCRIPTION: c_int = 1;
    const GTK_ACCESSIBLE_PROPERTY_LABEL: c_int = 4;

    // GCallbacks are ABI-polymorphic in GLib; each signal below passes its
    // typed trampoline through the opaque `gpointer` slot.
    type AsyncReadyCallback = extern "C" fn(gpointer, *mut GAsyncResult, gpointer);
    type DestroyCallback = extern "C" fn(gpointer, *mut GClosure);

    type GtkAccessibleUpdateProperty =
        unsafe extern "C" fn(*mut GtkAccessible, c_int, *const c_char, c_int);

    struct Api {
        _handle: *mut c_void,
        gtk_init_check: unsafe extern "C" fn() -> gboolean,
        gtk_window_new: unsafe extern "C" fn() -> *mut GtkWidget,
        gtk_window_set_title: unsafe extern "C" fn(*mut GtkWidget, *const c_char),
        gtk_window_set_default_size: unsafe extern "C" fn(*mut GtkWidget, c_int, c_int),
        gtk_window_set_child: unsafe extern "C" fn(*mut GtkWidget, *mut GtkWidget),
        gtk_window_present: unsafe extern "C" fn(*mut GtkWidget),
        gtk_box_new: unsafe extern "C" fn(c_int, c_int) -> *mut GtkWidget,
        gtk_box_append: unsafe extern "C" fn(*mut GtkWidget, *mut GtkWidget),
        gtk_box_remove: unsafe extern "C" fn(*mut GtkWidget, *mut GtkWidget),
        gtk_label_new: unsafe extern "C" fn(*const c_char) -> *mut GtkWidget,
        gtk_label_set_text: unsafe extern "C" fn(*mut GtkWidget, *const c_char),
        gtk_button_new_with_label: unsafe extern "C" fn(*const c_char) -> *mut GtkWidget,
        gtk_button_set_label: unsafe extern "C" fn(*mut GtkWidget, *const c_char),
        gtk_entry_new: unsafe extern "C" fn() -> *mut GtkWidget,
        gtk_editable_set_text: unsafe extern "C" fn(*mut GtkWidget, *const c_char),
        gtk_widget_set_size_request: unsafe extern "C" fn(*mut GtkWidget, c_int, c_int),
        gtk_widget_add_css_class: unsafe extern "C" fn(*mut GtkWidget, *const c_char),
        gtk_widget_remove_css_class: unsafe extern "C" fn(*mut GtkWidget, *const c_char),
        gtk_widget_grab_focus: unsafe extern "C" fn(*mut GtkWidget) -> gboolean,
        gtk_css_provider_new: unsafe extern "C" fn() -> *mut GtkCssProvider,
        gtk_css_provider_load_from_string:
            unsafe extern "C" fn(*mut GtkCssProvider, *const c_char),
        gtk_style_context_add_provider_for_display:
            unsafe extern "C" fn(*mut GdkDisplay, *mut GtkCssProvider, c_int),
        gdk_display_get_default: unsafe extern "C" fn() -> *mut GdkDisplay,
        gdk_display_get_clipboard:
            unsafe extern "C" fn(*mut GdkDisplay) -> *mut GdkClipboard,
        gdk_clipboard_set_text: unsafe extern "C" fn(*mut GdkClipboard, *const c_char),
        gdk_clipboard_read_text_async:
            unsafe extern "C" fn(*mut GdkClipboard, gpointer, AsyncReadyCallback, gpointer),
        gdk_clipboard_read_text_finish: unsafe extern "C" fn(
            *mut GdkClipboard,
            *mut GAsyncResult,
            *mut *mut GError,
        ) -> *mut c_char,
        g_main_context_iteration: unsafe extern "C" fn(gpointer, gboolean) -> gboolean,
        g_free: unsafe extern "C" fn(gpointer),
        g_signal_connect_data: unsafe extern "C" fn(
            gpointer,
            *const c_char,
            gpointer,
            gpointer,
            Option<DestroyCallback>,
            c_int,
        ) -> u64,
        gtk_file_dialog_new: unsafe extern "C" fn() -> *mut GtkFileDialog,
        gtk_file_dialog_set_title:
            unsafe extern "C" fn(*mut GtkFileDialog, *const c_char),
        gtk_file_dialog_set_modal: unsafe extern "C" fn(*mut GtkFileDialog, gboolean),
        gtk_file_dialog_set_initial_folder:
            unsafe extern "C" fn(*mut GtkFileDialog, *mut GFile),
        gtk_file_dialog_set_filters:
            unsafe extern "C" fn(*mut GtkFileDialog, *mut GListModel),
        gtk_file_dialog_open: unsafe extern "C" fn(
            *mut GtkFileDialog,
            *mut GtkWidget,
            gpointer,
            AsyncReadyCallback,
            gpointer,
        ),
        gtk_file_dialog_open_multiple: unsafe extern "C" fn(
            *mut GtkFileDialog,
            *mut GtkWidget,
            gpointer,
            AsyncReadyCallback,
            gpointer,
        ),
        gtk_file_dialog_save: unsafe extern "C" fn(
            *mut GtkFileDialog,
            *mut GtkWidget,
            gpointer,
            AsyncReadyCallback,
            gpointer,
        ),
        gtk_file_dialog_open_finish:
            unsafe extern "C" fn(*mut GtkFileDialog, *mut GAsyncResult, *mut *mut GError)
                -> *mut GFile,
        gtk_file_dialog_open_multiple_finish:
            unsafe extern "C" fn(*mut GtkFileDialog, *mut GAsyncResult, *mut *mut GError)
                -> *mut GListModel,
        gtk_file_dialog_save_finish:
            unsafe extern "C" fn(*mut GtkFileDialog, *mut GAsyncResult, *mut *mut GError)
                -> *mut GFile,
        gtk_file_filter_new: unsafe extern "C" fn() -> *mut GtkFileFilter,
        gtk_file_filter_get_type: unsafe extern "C" fn() -> GType,
        gtk_file_filter_set_name:
            unsafe extern "C" fn(*mut GtkFileFilter, *const c_char),
        gtk_file_filter_add_pattern:
            unsafe extern "C" fn(*mut GtkFileFilter, *const c_char),
        gtk_file_filter_add_mime_type:
            unsafe extern "C" fn(*mut GtkFileFilter, *const c_char),
        g_type_from_name: unsafe extern "C" fn(*const c_char) -> GType,
        g_list_store_new: unsafe extern "C" fn(GType) -> *mut GListStore,
        g_list_store_append: unsafe extern "C" fn(*mut GListStore, gpointer),
        g_list_model_get_n_items: unsafe extern "C" fn(*mut GListModel) -> u32,
        g_list_model_get_item: unsafe extern "C" fn(*mut GListModel, u32) -> gpointer,
        g_file_new_for_path: unsafe extern "C" fn(*const c_char) -> *mut GFile,
        g_file_get_path: unsafe extern "C" fn(*mut GFile) -> *mut c_char,
        g_error_free: unsafe extern "C" fn(*mut GError),
        g_quark_from_static_string: unsafe extern "C" fn(*const c_char) -> u32,
        g_error_matches: unsafe extern "C" fn(*const GError, u32, c_int) -> gboolean,
        g_main_loop_new: unsafe extern "C" fn(gpointer, gboolean) -> *mut GMainLoop,
        g_main_loop_run: unsafe extern "C" fn(*mut GMainLoop),
        g_main_loop_unref: unsafe extern "C" fn(*mut GMainLoop),
        g_object_unref: unsafe extern "C" fn(gpointer),
        gtk_accessible_update_property: GtkAccessibleUpdateProperty,
        gtk_event_controller_key_new: unsafe extern "C" fn() -> *mut GtkEventController,
        gtk_drop_target_new:
            unsafe extern "C" fn(GType, u32) -> *mut GtkDropTarget,
        gtk_widget_add_controller:
            unsafe extern "C" fn(*mut GtkWidget, *mut GtkEventController),
        gdk_keyval_name: unsafe extern "C" fn(u32) -> *const c_char,
        g_value_get_string: unsafe extern "C" fn(*const GValue) -> *const c_char,
    }

    // The handle and function pointers refer to one process-global, immutable
    // loader result. Keeping the library open is required for every pointer.
    unsafe impl Send for Api {}
    unsafe impl Sync for Api {}

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[link(name = "dl")]
    unsafe extern "C" {}

    #[cfg(unix)]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[cfg(unix)]
    const RTLD_NOW: c_int = 2;

    #[cfg(unix)]
    unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &'static CStr) -> Option<T> {
        let address = dlsym(handle, name.as_ptr());
        (!address.is_null()).then(|| std::mem::transmute_copy(&address))
    }

    #[cfg(unix)]
    fn load() -> Option<Api> {
        const LIBRARIES: [&[u8]; 4] = [
            b"libgtk-4.so.1\0",
            b"libgtk-4.so\0",
            b"libgtk-4.1.dylib\0",
            b"libgtk-4.dylib\0",
        ];
        for library in LIBRARIES {
            // SAFETY: the name is a static NUL-terminated string and the
            // returned handle remains open for the lifetime of the process.
            let handle = unsafe { dlopen(library.as_ptr().cast(), RTLD_NOW) };
            if handle.is_null() {
                continue;
            }
            macro_rules! load_symbol {
                ($name:literal) => {
                    unsafe {
                        symbol(
                            handle,
                            CStr::from_bytes_with_nul_unchecked(
                                concat!($name, "\0").as_bytes(),
                            ),
                        )?
                    }
                };
            }
            return Some(Api {
                _handle: handle,
                gtk_init_check: load_symbol!("gtk_init_check"),
                gtk_window_new: load_symbol!("gtk_window_new"),
                gtk_window_set_title: load_symbol!("gtk_window_set_title"),
                gtk_window_set_default_size: load_symbol!("gtk_window_set_default_size"),
                gtk_window_set_child: load_symbol!("gtk_window_set_child"),
                gtk_window_present: load_symbol!("gtk_window_present"),
                gtk_box_new: load_symbol!("gtk_box_new"),
                gtk_box_append: load_symbol!("gtk_box_append"),
                gtk_box_remove: load_symbol!("gtk_box_remove"),
                gtk_label_new: load_symbol!("gtk_label_new"),
                gtk_label_set_text: load_symbol!("gtk_label_set_text"),
                gtk_button_new_with_label: load_symbol!("gtk_button_new_with_label"),
                gtk_button_set_label: load_symbol!("gtk_button_set_label"),
                gtk_entry_new: load_symbol!("gtk_entry_new"),
                gtk_editable_set_text: load_symbol!("gtk_editable_set_text"),
                gtk_widget_set_size_request: load_symbol!("gtk_widget_set_size_request"),
                gtk_widget_add_css_class: load_symbol!("gtk_widget_add_css_class"),
                gtk_widget_remove_css_class: load_symbol!("gtk_widget_remove_css_class"),
                gtk_widget_grab_focus: load_symbol!("gtk_widget_grab_focus"),
                gtk_css_provider_new: load_symbol!("gtk_css_provider_new"),
                gtk_css_provider_load_from_string:
                    load_symbol!("gtk_css_provider_load_from_string"),
                gtk_style_context_add_provider_for_display:
                    load_symbol!("gtk_style_context_add_provider_for_display"),
                gdk_display_get_default: load_symbol!("gdk_display_get_default"),
                gdk_display_get_clipboard: load_symbol!("gdk_display_get_clipboard"),
                gdk_clipboard_set_text: load_symbol!("gdk_clipboard_set_text"),
                gdk_clipboard_read_text_async: load_symbol!("gdk_clipboard_read_text_async"),
                gdk_clipboard_read_text_finish: load_symbol!("gdk_clipboard_read_text_finish"),
                g_main_context_iteration: load_symbol!("g_main_context_iteration"),
                g_free: load_symbol!("g_free"),
                g_signal_connect_data: load_symbol!("g_signal_connect_data"),
                gtk_file_dialog_new: load_symbol!("gtk_file_dialog_new"),
                gtk_file_dialog_set_title: load_symbol!("gtk_file_dialog_set_title"),
                gtk_file_dialog_set_modal: load_symbol!("gtk_file_dialog_set_modal"),
                gtk_file_dialog_set_initial_folder:
                    load_symbol!("gtk_file_dialog_set_initial_folder"),
                gtk_file_dialog_set_filters: load_symbol!("gtk_file_dialog_set_filters"),
                gtk_file_dialog_open: load_symbol!("gtk_file_dialog_open"),
                gtk_file_dialog_open_multiple: load_symbol!("gtk_file_dialog_open_multiple"),
                gtk_file_dialog_save: load_symbol!("gtk_file_dialog_save"),
                gtk_file_dialog_open_finish: load_symbol!("gtk_file_dialog_open_finish"),
                gtk_file_dialog_open_multiple_finish:
                    load_symbol!("gtk_file_dialog_open_multiple_finish"),
                gtk_file_dialog_save_finish: load_symbol!("gtk_file_dialog_save_finish"),
                gtk_file_filter_new: load_symbol!("gtk_file_filter_new"),
                gtk_file_filter_get_type: load_symbol!("gtk_file_filter_get_type"),
                gtk_file_filter_set_name: load_symbol!("gtk_file_filter_set_name"),
                gtk_file_filter_add_pattern: load_symbol!("gtk_file_filter_add_pattern"),
                gtk_file_filter_add_mime_type: load_symbol!("gtk_file_filter_add_mime_type"),
                g_type_from_name: load_symbol!("g_type_from_name"),
                g_list_store_new: load_symbol!("g_list_store_new"),
                g_list_store_append: load_symbol!("g_list_store_append"),
                g_list_model_get_n_items: load_symbol!("g_list_model_get_n_items"),
                g_list_model_get_item: load_symbol!("g_list_model_get_item"),
                g_file_new_for_path: load_symbol!("g_file_new_for_path"),
                g_file_get_path: load_symbol!("g_file_get_path"),
                g_error_free: load_symbol!("g_error_free"),
                g_quark_from_static_string: load_symbol!("g_quark_from_static_string"),
                g_error_matches: load_symbol!("g_error_matches"),
                g_main_loop_new: load_symbol!("g_main_loop_new"),
                g_main_loop_run: load_symbol!("g_main_loop_run"),
                g_main_loop_unref: load_symbol!("g_main_loop_unref"),
                g_object_unref: load_symbol!("g_object_unref"),
                gtk_accessible_update_property: load_symbol!("gtk_accessible_update_property"),
                gtk_event_controller_key_new: load_symbol!("gtk_event_controller_key_new"),
                gtk_drop_target_new: load_symbol!("gtk_drop_target_new"),
                gtk_widget_add_controller: load_symbol!("gtk_widget_add_controller"),
                gdk_keyval_name: load_symbol!("gdk_keyval_name"),
                g_value_get_string: load_symbol!("g_value_get_string"),
            });
        }
        None
    }

    #[cfg(not(unix))]
    fn load() -> Option<Api> {
        None
    }

    fn gtk_api() -> Option<&'static Api> {
        static API: LazyLock<Option<Api>> = LazyLock::new(load);
        API.as_ref()
    }

    macro_rules! gtk_api_wrapper {
        ($name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty = $default:expr) => {
            fn $name($($arg: $ty),*) -> $ret {
                let Some(api) = gtk_api() else {
                    return $default;
                };
                // SAFETY: `api` contains the symbol with the exact declared GTK ABI.
                unsafe { (api.$name)($($arg),*) }
            }
        };
    }

    gtk_api_wrapper!(gtk_init_check() -> gboolean = 0);
    gtk_api_wrapper!(gtk_window_new() -> *mut GtkWidget = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_window_set_title(window: *mut GtkWidget, title: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_window_set_default_size(window: *mut GtkWidget, width: c_int, height: c_int) -> () = ());
    gtk_api_wrapper!(gtk_window_set_child(window: *mut GtkWidget, child: *mut GtkWidget) -> () = ());
    gtk_api_wrapper!(gtk_window_present(window: *mut GtkWidget) -> () = ());
    gtk_api_wrapper!(gtk_box_new(orientation: c_int, spacing: c_int) -> *mut GtkWidget = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_box_append(box_: *mut GtkWidget, child: *mut GtkWidget) -> () = ());
    gtk_api_wrapper!(gtk_box_remove(box_: *mut GtkWidget, child: *mut GtkWidget) -> () = ());
    gtk_api_wrapper!(gtk_label_new(text: *const c_char) -> *mut GtkWidget = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_label_set_text(label: *mut GtkWidget, text: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_button_new_with_label(label: *const c_char) -> *mut GtkWidget = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_button_set_label(button: *mut GtkWidget, label: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_entry_new() -> *mut GtkWidget = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_editable_set_text(editable: *mut GtkWidget, text: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_widget_set_size_request(widget: *mut GtkWidget, width: c_int, height: c_int) -> () = ());
    gtk_api_wrapper!(gtk_widget_add_css_class(widget: *mut GtkWidget, css_class: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_widget_remove_css_class(widget: *mut GtkWidget, css_class: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_widget_grab_focus(widget: *mut GtkWidget) -> gboolean = 0);
    gtk_api_wrapper!(gtk_css_provider_new() -> *mut GtkCssProvider = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_css_provider_load_from_string(provider: *mut GtkCssProvider, string: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_style_context_add_provider_for_display(display: *mut GdkDisplay, provider: *mut GtkCssProvider, priority: c_int) -> () = ());
    gtk_api_wrapper!(gdk_display_get_default() -> *mut GdkDisplay = std::ptr::null_mut());
    gtk_api_wrapper!(gdk_display_get_clipboard(display: *mut GdkDisplay) -> *mut GdkClipboard = std::ptr::null_mut());
    gtk_api_wrapper!(gdk_clipboard_set_text(clipboard: *mut GdkClipboard, text: *const c_char) -> () = ());
    gtk_api_wrapper!(gdk_clipboard_read_text_async(clipboard: *mut GdkClipboard, cancellable: gpointer, callback: AsyncReadyCallback, user_data: gpointer) -> () = ());
    gtk_api_wrapper!(gdk_clipboard_read_text_finish(clipboard: *mut GdkClipboard, result: *mut GAsyncResult, error: *mut *mut GError) -> *mut c_char = std::ptr::null_mut());
    gtk_api_wrapper!(g_main_context_iteration(context: gpointer, may_block: gboolean) -> gboolean = 0);
    gtk_api_wrapper!(g_free(pointer: gpointer) -> () = ());
    gtk_api_wrapper!(g_signal_connect_data(instance: gpointer, detailed_signal: *const c_char, c_handler: gpointer, data: gpointer, destroy_data: Option<DestroyCallback>, connect_flags: c_int) -> u64 = 0);
    gtk_api_wrapper!(gtk_file_dialog_new() -> *mut GtkFileDialog = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_file_dialog_set_title(dialog: *mut GtkFileDialog, title: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_file_dialog_set_modal(dialog: *mut GtkFileDialog, modal: gboolean) -> () = ());
    gtk_api_wrapper!(gtk_file_dialog_set_initial_folder(dialog: *mut GtkFileDialog, folder: *mut GFile) -> () = ());
    gtk_api_wrapper!(gtk_file_dialog_set_filters(dialog: *mut GtkFileDialog, filters: *mut GListModel) -> () = ());
    gtk_api_wrapper!(gtk_file_dialog_open(dialog: *mut GtkFileDialog, parent: *mut GtkWidget, cancellable: gpointer, callback: AsyncReadyCallback, user_data: gpointer) -> () = ());
    gtk_api_wrapper!(gtk_file_dialog_open_multiple(dialog: *mut GtkFileDialog, parent: *mut GtkWidget, cancellable: gpointer, callback: AsyncReadyCallback, user_data: gpointer) -> () = ());
    gtk_api_wrapper!(gtk_file_dialog_save(dialog: *mut GtkFileDialog, parent: *mut GtkWidget, cancellable: gpointer, callback: AsyncReadyCallback, user_data: gpointer) -> () = ());
    gtk_api_wrapper!(gtk_file_dialog_open_finish(dialog: *mut GtkFileDialog, result: *mut GAsyncResult, error: *mut *mut GError) -> *mut GFile = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_file_dialog_open_multiple_finish(dialog: *mut GtkFileDialog, result: *mut GAsyncResult, error: *mut *mut GError) -> *mut GListModel = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_file_dialog_save_finish(dialog: *mut GtkFileDialog, result: *mut GAsyncResult, error: *mut *mut GError) -> *mut GFile = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_file_filter_new() -> *mut GtkFileFilter = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_file_filter_get_type() -> GType = 0);
    gtk_api_wrapper!(gtk_file_filter_set_name(filter: *mut GtkFileFilter, name: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_file_filter_add_pattern(filter: *mut GtkFileFilter, pattern: *const c_char) -> () = ());
    gtk_api_wrapper!(gtk_file_filter_add_mime_type(filter: *mut GtkFileFilter, mime: *const c_char) -> () = ());
    gtk_api_wrapper!(g_type_from_name(name: *const c_char) -> GType = 0);
    gtk_api_wrapper!(g_list_store_new(item_type: GType) -> *mut GListStore = std::ptr::null_mut());
    gtk_api_wrapper!(g_list_store_append(store: *mut GListStore, item: gpointer) -> () = ());
    gtk_api_wrapper!(g_list_model_get_n_items(model: *mut GListModel) -> u32 = 0);
    gtk_api_wrapper!(g_list_model_get_item(model: *mut GListModel, position: u32) -> gpointer = std::ptr::null_mut());
    gtk_api_wrapper!(g_file_new_for_path(path: *const c_char) -> *mut GFile = std::ptr::null_mut());
    gtk_api_wrapper!(g_file_get_path(file: *mut GFile) -> *mut c_char = std::ptr::null_mut());
    gtk_api_wrapper!(g_error_free(error: *mut GError) -> () = ());
    gtk_api_wrapper!(g_quark_from_static_string(string: *const c_char) -> u32 = 0);
    gtk_api_wrapper!(g_error_matches(error: *const GError, domain: u32, code: c_int) -> gboolean = 0);
    gtk_api_wrapper!(g_main_loop_new(context: gpointer, is_running: gboolean) -> *mut GMainLoop = std::ptr::null_mut());
    gtk_api_wrapper!(g_main_loop_run(loop_: *mut GMainLoop) -> () = ());
    gtk_api_wrapper!(g_main_loop_unref(loop_: *mut GMainLoop) -> () = ());
    gtk_api_wrapper!(g_object_unref(object: gpointer) -> () = ());
    gtk_api_wrapper!(gtk_event_controller_key_new() -> *mut GtkEventController = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_drop_target_new(value_type: GType, actions: u32) -> *mut GtkDropTarget = std::ptr::null_mut());
    gtk_api_wrapper!(gtk_widget_add_controller(widget: *mut GtkWidget, controller: *mut GtkEventController) -> () = ());
    gtk_api_wrapper!(gdk_keyval_name(keyval: u32) -> *const c_char = std::ptr::null());
    gtk_api_wrapper!(g_value_get_string(value: *const GValue) -> *const c_char = std::ptr::null());

    /// Trampoline for a GTK "clicked" signal. Each connection owns one boxed
    /// `Arc<dyn Fn() + Send + Sync>`; the registry retains another `Arc` for path recreation.
    extern "C" fn jet_gtk_click_trampoline(_widget: *mut GtkWidget, data: gpointer) {
        if data.is_null() {
            return;
        }
        // SAFETY: `data` is a boxed `Arc<dyn Fn() + Send + Sync>` owned by this signal
        // connection. GTK runs its destroy notifier before releasing it.
        unsafe {
            let cb = &*(data as *const Arc<dyn Fn() + Send + Sync>);
            cb();
        }
    }
    struct ClipboardRead {
        done: bool,
        text: Option<String>,
    }

    extern "C" fn jet_gtk_clipboard_read_ready(
        source: gpointer,
        result: *mut GAsyncResult,
        data: gpointer,
    ) {
        if data.is_null() {
            return;
        }
        // SAFETY: the callback runs synchronously with the stack value owned
        // by `read_clipboard_text`; the GTK async operation is drained before
        // that value leaves scope.
        let state = unsafe { &mut *(data as *mut ClipboardRead) };
        let mut error = std::ptr::null_mut();
        let text = unsafe {
            gdk_clipboard_read_text_finish(
                source as *mut GdkClipboard,
                result,
                &mut error,
            )
        };
        if !text.is_null() {
            let value = unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned();
            unsafe { g_free(text as gpointer) };
            state.text = Some(value);
        }
        state.done = true;
    }
    #[derive(Clone, Copy)]
    enum FileDialogMode {
        Open,
        OpenMultiple,
        Save,
    }

    struct FileDialogRead {
        done: bool,
        mode: FileDialogMode,
        files: Vec<String>,
        cancelled: bool,
        error: Option<String>,
    }

    unsafe fn gtk_file_path(file: *mut GFile) -> Option<String> {
        if file.is_null() {
            return None;
        }
        let path = g_file_get_path(file);
        let value = (!path.is_null())
            .then(|| CStr::from_ptr(path).to_string_lossy().into_owned());
        if !path.is_null() {
            g_free(path as gpointer);
        }
        g_object_unref(file as gpointer);
        value
    }

    unsafe fn gtk_file_model_paths(model: *mut GListModel) -> Vec<String> {
        if model.is_null() {
            return Vec::new();
        }
        let count = g_list_model_get_n_items(model);
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            let file = g_list_model_get_item(model, index) as *mut GFile;
            if let Some(path) = gtk_file_path(file) {
                paths.push(path);
            }
        }
        g_object_unref(model as gpointer);
        paths
    }

    unsafe fn gtk_file_error(error: *mut GError) -> (bool, String) {
        let domain = g_quark_from_static_string(G_IO_ERROR_QUARK.as_ptr() as *const c_char);
        let cancelled = g_error_matches(error as *const GError, domain, G_IO_ERROR_CANCELLED) != 0;
        if cancelled {
            return (true, String::new());
        }
        let record = &*(error as *const NativeGError);
        let text = if record.message.is_null() {
            "GTK file dialog failed".to_string()
        } else {
            CStr::from_ptr(record.message)
                .to_string_lossy()
                .chars()
                .take(512)
                .collect::<String>()
        };
        (false, format!("GTK file dialog failed: {text}"))
    }
    extern "C" fn jet_gtk_file_dialog_ready(
        source: gpointer,

        result: *mut GAsyncResult,
        data: gpointer,
    ) {
        if data.is_null() {
            return;
        }
        // SAFETY: the callback runs while `run_file_dialog` owns the stack
        // state and drains the GLib context before returning.
        let state = unsafe { &mut *(data as *mut FileDialogRead) };
        let mut error = std::ptr::null_mut();
        let files = unsafe {
            match state.mode {
                FileDialogMode::Open => {
                    let file = gtk_file_dialog_open_finish(
                        source as *mut GtkFileDialog,
                        result,
                        &mut error,
                    );
                    gtk_file_path(file).map(|path| vec![path])
                }
                FileDialogMode::OpenMultiple => Some(gtk_file_model_paths(
                    gtk_file_dialog_open_multiple_finish(
                        source as *mut GtkFileDialog,
                        result,
                        &mut error,
                    ),
                )),
                FileDialogMode::Save => {
                    let file = gtk_file_dialog_save_finish(
                        source as *mut GtkFileDialog,
                        result,
                        &mut error,
                    );
                    gtk_file_path(file).map(|path| vec![path])
                }
            }
        };
        if !error.is_null() {
            let (cancelled, message) = unsafe { gtk_file_error(error) };
            state.cancelled = cancelled;
            if !cancelled {
                state.error = Some(message);
            }
            unsafe { g_error_free(error) };
        }
        state.files = files.unwrap_or_default();
        state.done = true;
    }

    extern "C" fn jet_gtk_drop_callback(data: gpointer, _closure: *mut GClosure) {
        if data.is_null() {
            return;
        }
        // SAFETY: `on_click` allocated exactly one Box at this pointer; GTK
        // invokes this notifier once when the signal/widget is destroyed.
        unsafe {
            drop(Box::from_raw(data as *mut Arc<dyn Fn() + Send + Sync>));
        }
    }
    struct GtkImeBinding {
        state: *mut GtkState,
        target: JetUiNodeId,
        path: String,
    }

    extern "C" fn jet_gtk_binding_drop(data: gpointer, _closure: *mut GClosure) {
        if data.is_null() {
            return;
        }
        // SAFETY: each signal owns one boxed binding and invokes this
        // destroy notifier exactly once.
        unsafe {
            drop(Box::from_raw(data as *mut GtkImeBinding));
        }
    }

    extern "C" fn jet_gtk_insert_text_trampoline(
        _editable: *mut GtkWidget,
        text: *const c_char,
        _length: c_int,
        data: gpointer,
    ) {
        if data.is_null() || text.is_null() {
            return;
        }
        // SAFETY: GTK owns `text` for the duration of this signal callback;
        // the binding remains owned by the widget signal.
        let binding = unsafe { &*(data as *const GtkImeBinding) };
        let enabled = unsafe {
            (*binding.state)
                .ime_enabled
                .get(&binding.path)
                .copied()
                .unwrap_or(false)
        };
        if !enabled {
            return;
        }
        let text = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        let end = text.chars().count();
        let selection = JetUiTextRange::new(0, end).unwrap_or_default();
        unsafe {
            (*binding.state).ime_events.push_back(JetUiImeEvent {
                target: binding.target.clone(),
                phase: JetUiImePhase::Commit,
                composition: Some(JetUiImeComposition {
                    text,
                    selection,
                    marked: None,
                }),
            });
        }
    }

    struct GtkDragBinding {
        state: *mut GtkState,
        target: JetUiNodeId,
    }

    extern "C" fn jet_gtk_drag_binding_drop(data: gpointer, _closure: *mut GClosure) {
        if data.is_null() {
            return;
        }
        // SAFETY: each drag signal owns one boxed binding.
        unsafe {
            drop(Box::from_raw(data as *mut GtkDragBinding));
        }
    }

    fn jet_gtk_queue_drag(
        binding: &GtkDragBinding,
        phase: JetUiDragPhase,
        items: Vec<JetUiDropItem>,
    ) {
        unsafe {
            (*binding.state).drag_events.push_back(JetUiDragEvent {
                target: binding.target.clone(),
                phase,
                operation: JetUiDragOperation::Copy,
                items,
            });
        }
    }

    extern "C" fn jet_gtk_drag_enter_trampoline(
        _target: *mut GtkWidget,
        _x: c_double,
        _y: c_double,
        data: gpointer,
    ) -> u32 {
        if data.is_null() {
            return 0;
        }
        let binding = unsafe { &*(data as *const GtkDragBinding) };
        jet_gtk_queue_drag(binding, JetUiDragPhase::Enter, Vec::new());
        1
    }

    extern "C" fn jet_gtk_drag_motion_trampoline(
        _target: *mut GtkWidget,
        _x: c_double,
        _y: c_double,
        data: gpointer,
    ) -> u32 {
        if data.is_null() {
            return 0;
        }
        let binding = unsafe { &*(data as *const GtkDragBinding) };
        jet_gtk_queue_drag(binding, JetUiDragPhase::Over, Vec::new());
        1
    }

    extern "C" fn jet_gtk_drag_leave_trampoline(_target: *mut GtkWidget, data: gpointer) {
        if data.is_null() {
            return;
        }
        let binding = unsafe { &*(data as *const GtkDragBinding) };
        jet_gtk_queue_drag(binding, JetUiDragPhase::Leave, Vec::new());
    }

    extern "C" fn jet_gtk_drop_trampoline(
        _target: *mut GtkWidget,
        value: *const GValue,
        _x: c_double,
        _y: c_double,
        data: gpointer,
    ) -> gboolean {
        if data.is_null() {
            return 0;
        }
        let binding = unsafe { &*(data as *const GtkDragBinding) };
        let items = if value.is_null() {
            Vec::new()
        } else {
            let text = unsafe { g_value_get_string(value) };
            if text.is_null() {
                Vec::new()
            } else {
                let text = unsafe { CStr::from_ptr(text) }
                    .to_string_lossy()
                    .into_owned();
                if text.starts_with("file://") {
                    vec![JetUiDropItem::Uri(text)]
                } else {
                    vec![JetUiDropItem::Text(text)]
                }
            }
        };
        jet_gtk_queue_drag(binding, JetUiDragPhase::Drop, items);
        1
    }

    extern "C" fn jet_gtk_shortcut_trampoline(
        _controller: *mut GtkWidget,
        keyval: u32,
        _keycode: u32,
        modifiers: u32,
        data: gpointer,
    ) -> gboolean {
        if data.is_null() {
            return 0;
        }
        let state = unsafe { &mut *(data as *mut GtkState) };
        let key_name = unsafe { gdk_keyval_name(keyval) };
        if key_name.is_null() {
            return 0;
        }
        let key = unsafe { CStr::from_ptr(key_name) }
            .to_string_lossy()
            .into_owned();
        let mut modifier_bits = 0u8;
        if modifiers & (1 << 0) != 0 {
            modifier_bits |= 1 << 2;
        }
        if modifiers & (1 << 2) != 0 {
            modifier_bits |= 1;
        }
        if modifiers & (1 << 3) != 0 {
            modifier_bits |= 1 << 1;
        }
        if modifiers & (1 << 26) != 0 {
            modifier_bits |= 1 << 3;
        }
        // `.cmd()` is the logical platform-command modifier. Keep the
        // physical candidate so explicit `.control`/`.meta` bindings still
        // work, then try the logical candidate first (Ctrl on Unix/Windows,
        // Meta on macOS).
        let physical = super::JetUiShortcutModifiers::from_bits(modifier_bits);
        let mut logical_bits = modifier_bits;
        if cfg!(target_os = "macos") && modifier_bits & (1 << 3) != 0 {
            logical_bits = (logical_bits & !(1 << 3)) | (1 << 4);
        } else if !cfg!(target_os = "macos") && modifier_bits & 1 != 0 {
            logical_bits = (logical_bits & !1) | (1 << 4);
        }
        let candidates = [
            super::JetUiShortcutModifiers::from_bits(logical_bits),
            physical,
        ];
        let mut handled_shortcut = None;
        for candidate_modifiers in candidates {
            let Ok(candidate) = JetUiShortcut::new(&key, candidate_modifiers) else {
                continue;
            };
            if !matches!(
                state.shortcuts.dispatch(&candidate),
                JetUiShortcutDispatch::Unhandled
            ) {
                handled_shortcut = Some(candidate);
                break;
            }
        }
        let Some(shortcut) = handled_shortcut else {
            return 0;
        };
        state.trace(&format!("shortcut-dispatch {}", shortcut.key));
        1
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum GtkAvailability {
        Uninitialized,
        Ready,
        GtkUnavailable,
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

    // Reconciled widgets are addressed by a stable tree path; direct widget
    // handles retain the native pointer for the lifetime of the backend.
    enum GtkWidgetHandleTarget {
        Direct(*mut GtkWidget),
        TreePath(String),
    }

    struct GtkWidgetHandle {
        target: GtkWidgetHandleTarget,
        kind: GtkWidgetKind,
    }

    struct GtkClickBinding {
        path: String,
        callback: Arc<dyn Fn() + Send + Sync>,
        connected_widget: *mut GtkWidget,
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
        click_bindings: Vec<GtkClickBinding>,
        tree_widgets: Vec<GtkWidgetRecord>,
        availability: GtkAvailability,
        capabilities: JetUiCapabilityFacts,
        shortcuts: JetUiShortcutRegistry,
        /// Current IME policy by retained tree path. Signal bindings stay
        /// installed across reactive paints and consult this map before
        /// enqueueing a native composition event.
        ime_enabled: std::collections::HashMap<String, bool>,
        ime_events: std::collections::VecDeque<JetUiImeEvent>,
        drag_events: std::collections::VecDeque<JetUiDragEvent>,
        native_controllers: bool,
    }

    // SAFETY: GTK widgets stay on the creating thread; Send/Sync only satisfy
    // `jet_ui_reactive_render`'s closure bound (headless runs never cross threads).
    unsafe impl Send for GtkState {}
    unsafe impl Sync for GtkState {}

    fn gtk_set_accessible_property(
        widget: *mut GtkWidget,
        property: c_int,
        value: Option<&str>,
    ) {
        let value = value.and_then(|text| CString::new(text).ok());
        let value_ptr = value
            .as_ref()
            .map_or(std::ptr::null(), |text| text.as_ptr());
        // GtkAccessible properties are not ordinary GObject properties.
        // Update the interface directly so GTK exports the values through
        // AT-SPI. The dynamically loaded pointer uses the fixed prefix plus
        // the ABI-required integer sentinel; no Rust variadic definition or
        // function-pointer signature cast is needed.
        let Some(api) = gtk_api() else {
            return;
        };
        unsafe {
            (api.gtk_accessible_update_property)(
                widget as *mut GtkAccessible,
                property,
                value_ptr,
                -1i32,
            );
        }
    }

    fn gtk_apply_accessibility(widget: *mut GtkWidget, node: &JetUiNode) {
        if widget.is_null() {
            return;
        }
        let (name, description) = node
            .accessibility
            .as_ref()
            .map_or((None, None), |metadata| {
                (metadata.name.as_deref(), metadata.description.as_deref())
            });
        gtk_set_accessible_property(widget, GTK_ACCESSIBLE_PROPERTY_LABEL, name);
        gtk_set_accessible_property(
            widget,
            GTK_ACCESSIBLE_PROPERTY_DESCRIPTION,
            description,
        );
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
                self.capabilities = JetUiCapabilityFacts::empty();
                self.availability = GtkAvailability::HeadlessOptIn;
                return;
            }
            if gtk_api().is_none() {
                self.display_ok = false;
                self.capabilities = JetUiCapabilityFacts::empty();
                self.availability = GtkAvailability::GtkUnavailable;
                return;
            }
            if gtk_init_check() == 0 {
                self.display_ok = false;
                self.capabilities = JetUiCapabilityFacts::empty();
                self.availability = GtkAvailability::UnsupportedDisplay;
                return;
            }
            self.display_ok = true;
            self.availability = GtkAvailability::Ready;
            self.window = gtk_window_new();
            self.vbox = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
            gtk_window_set_child(self.window, self.vbox);
            gtk_window_set_default_size(self.window, 320, 240);
            if !self.window.is_null() && !self.vbox.is_null() {
                self.capabilities.grant(JetUiCapability::Accessibility);
                self.capabilities.grant(JetUiCapability::FileDialog);
                self.capabilities.grant(JetUiCapability::Ime);
                self.capabilities.grant(JetUiCapability::DragDrop);
                self.capabilities.grant(JetUiCapability::Shortcuts);
                let display = gdk_display_get_default();
                if !display.is_null() && !gdk_display_get_clipboard(display).is_null() {
                    self.capabilities.grant(JetUiCapability::Clipboard);
                }
            }
            self.connect_native_controllers();

        }
        fn connect_native_controllers(&mut self) {
            if self.native_controllers || !self.display_ok || self.window.is_null() {
                return;
            }
            let controller = unsafe { gtk_event_controller_key_new() };
            if controller.is_null() {
                return;
            }
            let signal = CString::new("key-pressed").expect("static GTK signal name");
            // SAFETY: the GTK window owns the controller; the state lives at
            // a stable address behind JetGtkBackend's Arc for its lifetime.
            unsafe {
                g_signal_connect_data(
                    controller as gpointer,
                    signal.as_ptr(),
                    jet_gtk_shortcut_trampoline as gpointer,
                    self as *mut GtkState as gpointer,
                    None,
                    0,
                );
                gtk_widget_add_controller(self.window, controller);
            }
            self.native_controllers = true;
        }

        fn connect_native_input(&mut self, widget: *mut GtkWidget, node: &JetUiNode, path: &str) {
            if !self.display_ok || widget.is_null() {
                return;
            }
            let Ok(identity) = jet_ui_node_id(node, path) else {
                return;
            };
            if matches!(Self::widget_kind(node), GtkWidgetKind::Entry) {
                // GTK's GtkEditable emits this for text committed by native
                // keyboard/IME input; the queue is populated at that signal,
                // not by a synthetic polling fallback.
                let binding = Box::into_raw(Box::new(GtkImeBinding {
                    state: self as *mut GtkState,
                    target: identity.clone(),
                    path: path.to_string(),
                }));
                let signal = CString::new("insert-text").expect("static GTK signal name");
                unsafe {
                    g_signal_connect_data(
                        widget as gpointer,
                        signal.as_ptr(),
                        jet_gtk_insert_text_trampoline as gpointer,
                        binding as gpointer,
                        Some(jet_gtk_binding_drop),
                        0,
                    );
                }
            }
            if node.on_drop.is_some() {
                let type_name = CString::new("gchararray").expect("static GType name");
                let value_type = unsafe { g_type_from_name(type_name.as_ptr()) };
                let controller = unsafe { gtk_drop_target_new(value_type, 7) };
                if controller.is_null() {
                    return;
                }
                unsafe { gtk_widget_add_controller(widget, controller as *mut GtkEventController) };
                let signals: [(&str, gpointer); 4] = [
                    ("enter", jet_gtk_drag_enter_trampoline as gpointer),
                    ("motion", jet_gtk_drag_motion_trampoline as gpointer),
                    ("leave", jet_gtk_drag_leave_trampoline as gpointer),
                    ("drop", jet_gtk_drop_trampoline as gpointer),
                ];
                for (signal_name, callback) in signals {
                    let binding = Box::into_raw(Box::new(GtkDragBinding {
                        state: self as *mut GtkState,
                        target: identity.clone(),
                    }));
                    let signal = CString::new(signal_name).expect("static GTK signal name");
                    unsafe {
                        g_signal_connect_data(
                            controller as gpointer,
                            signal.as_ptr(),
                            callback,
                            binding as gpointer,
                            Some(jet_gtk_drag_binding_drop),
                            0,
                        );
                    }
                }
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
            self.ime_enabled.remove(&record.path);
            for binding in &mut self.click_bindings {
                if binding.path == record.path {
                    binding.connected_widget = std::ptr::null_mut();
                }
            }
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
            if kind == GtkWidgetKind::Entry {
                self.ime_enabled
                    .insert(path.to_string(), node.ime != Some(JetUiImeMode::Disabled));
            }
            let (index, created) = if let Some(index) = self.tree_widgets.iter().position(|record| record.path == path) {
                (index, false)
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
                (self.tree_widgets.len() - 1, true)
            };

            live.push(path.to_string());
            let widget = self.tree_widgets[index].widget;
            if created && kind == GtkWidgetKind::Button {
                // D-WEB-CLICK-PORT1=D / D-UI-EVT-DISP1=E: one click path —
                // portable node slots dispatch by identity; legacy
                // `backend.on_click` bindings still attach on the same widget.
                match jet_ui_node_id(node, path) {
                    Ok(identity) => Self::connect_click_callback(
                        widget,
                        Arc::new(move || jet_ui_dispatch(&identity)),
                    ),
                    Err(error) => self.trace(&format!("click identity rejected: {error:?}")),
                }
                self.attach_click_bindings(path, widget);
            }
            if created && (kind == GtkWidgetKind::Entry || node.on_drop.is_some()) {
                self.connect_native_input(widget, node, path);
            }
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
            gtk_apply_accessibility(widget, node);
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

        fn connect_click_callback(widget: *mut GtkWidget, callback: Arc<dyn Fn() + Send + Sync>) {
            let boxed = Box::into_raw(Box::new(callback));
            let signal = CString::new("clicked").unwrap();
            // SAFETY: `widget` is a live GTK button. The connection owns
            // `boxed`, and GTK invokes the destroy notifier exactly once.
            unsafe {
                g_signal_connect_data(
                    widget as gpointer,
                    signal.as_ptr(),
                    jet_gtk_click_trampoline as gpointer,
                    boxed as gpointer,
                    Some(jet_gtk_drop_callback),
                    0,
                );
            }
        }

        fn attach_click_bindings(&mut self, path: &str, widget: *mut GtkWidget) {
            if widget.is_null() {
                return;
            }
            let mut connected = 0;
            for binding in &mut self.click_bindings {
                if binding.path == path && binding.connected_widget != widget {
                    Self::connect_click_callback(widget, Arc::clone(&binding.callback));
                    binding.connected_widget = widget;
                    connected += 1;
                }
            }
            if connected > 0 {
                self.trace(&format!("click-connect {path} {connected}"));
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
            self.trace("cleanup");
        }
    }

    /// The native GTK4 backend. Constructing it is free; the first `label` /
    /// `button` call opens GTK and the window (when a display exists).
    #[derive(Clone)]
    pub struct JetGtkBackend {
        state: Arc<Mutex<GtkState>>,
    }

    impl JetGtkBackend {
        pub fn new() -> Self {
            JetGtkBackend {
                state: Arc::new(Mutex::new(GtkState {
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
                    click_bindings: Vec::new(),
                    tree_widgets: Vec::new(),
                    availability: GtkAvailability::Uninitialized,
                    capabilities: JetUiCapabilityFacts::empty(),
                    shortcuts: JetUiShortcutRegistry::default(),
                    ime_events: std::collections::VecDeque::new(),
                    ime_enabled: std::collections::HashMap::new(),
                    drag_events: std::collections::VecDeque::new(),
                    native_controllers: false,
                })),
            }
        }

        // ── Seam parity: display-free measure/layout/paint/on_event ──
        pub fn measure_node(&self, node: JetUiNode, constraint: JetSizeConstraint) -> JetSize {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            JetBackend::measure(&mut *state, &node, constraint)
        }
        pub fn layout_node(&self, node: JetUiNode, frame: JetRect) {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            JetBackend::layout(&mut *state, &node, frame);
        }
        pub fn paint_node(&self, node: JetUiNode) {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            JetBackend::paint(&mut *state, &node);
        }
        /// D-UI-MOUNT1=A: measure → layout → paint in one call.
        pub fn mount_node(&self, node: JetUiNode, constraint: JetSizeConstraint) {
            {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                state.commands.clear();
            }
            let size = self.measure_node(node.clone(), constraint);
            self.layout_node(
                node.clone(),
                jet_ui_rect(0.0, 0.0, size.width, size.height),
            );
            self.paint_node(node);
        }
        pub fn mount_node_default(&self, node: JetUiNode) {
            self.mount_node(node, jet_ui_constraint(0.0, 0.0, 320.0, 240.0));
        }
        pub fn dispatch_event(&self, event: JetInputEvent) -> JetEventResult {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            JetBackend::on_event(&mut *state, event)
        }
        pub fn set_focus_group(&self, nodes: Vec<JetUiNode>) {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.focused_index = if nodes.is_empty() { None } else { Some(0) };
            state.focus_paths = nodes
                .iter()
                .filter_map(|node| state.tree_widgets.iter().find(|record| record.label == node.label).map(|record| record.path.clone()))
                .collect();
            state.focus_nodes = nodes;
            state.focus_current_widget();
        }
        pub fn focused_label(&self) -> String {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
        pub fn on_click<F: Fn() + Send + Sync + 'static>(&self, id: i64, handler: F) {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some((widget, GtkWidgetKind::Button)) = state.resolve_widget_handle(id) else {
                state.trace(&format!("handle-miss {id}"));
                return;
            };
            let callback = Arc::new(handler) as Arc<dyn Fn() + Send + Sync>;
            let path = state.widget_handles.get(id as usize).and_then(|handle| {
                if let GtkWidgetHandleTarget::TreePath(path) = &handle.target {
                    Some(path.clone())
                } else {
                    None
                }

            });
            if let Some(path) = path {
                state.click_bindings.push(GtkClickBinding {
                    path: path.clone(),
                    callback,
                    connected_widget: std::ptr::null_mut(),
                });
                state.attach_click_bindings(&path, widget);
            } else {
                GtkState::connect_click_callback(widget, callback);
            }
        }

        /// Present the window and run the GLib main loop until it closes. No-op
        /// without a display (`JET_UI_HEADLESS` / headless CI), so the program
        /// terminates instead of blocking.
        pub fn present(&self, title: &str) {
            // Read what we need, then drop the borrow BEFORE the blocking loop so
            // click handlers (`set_text`, etc.) can re-borrow the state.
            let (display_ok, window, availability) = {
                let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                (state.display_ok, state.window, state.availability)
            };
            if !display_ok || window.is_null() {
                match availability {
                    GtkAvailability::GtkUnavailable => {
                        eprintln!(
                            "UI_UNSUPPORTED[gtk.runtime]: GTK4 runtime is unavailable; install GTK4 or set JET_UI_HEADLESS=1 only for an explicit headless run"
                        );
                    }
                    GtkAvailability::UnsupportedDisplay => {
                        eprintln!(
                            "UI_UNSUPPORTED[gtk.display]: no GTK display is available; set JET_UI_HEADLESS=1 only for an explicit headless run"
                        );
                    }
                    _ => {}
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
    fn gtk_file_dialog_setup(
        dialog: *mut GtkFileDialog,
        parent: *mut GtkWidget,
        request: &JetUiFileDialogRequest,
    ) -> Result<(), JetUiHostError> {
        if dialog.is_null() || parent.is_null() {
            return Err(JetUiHostError::CapabilityUnavailable {
                capability: "UI.FileDialog",
            });
        }
        let title = CString::new(request.title.as_str()).map_err(|_| {
            JetUiHostError::InvalidRequest("file dialog title must not contain NUL".to_string())
        })?;
        unsafe {
            gtk_file_dialog_set_title(dialog, title.as_ptr());
            gtk_file_dialog_set_modal(dialog, 1);
        }

        if let Some(directory) = request.initial_directory.as_ref() {
            let path = CString::new(directory.path()).map_err(|_| {
                JetUiHostError::InvalidRequest(
                    "file dialog initial directory must not contain NUL".to_string(),
                )
            })?;
            let folder = unsafe { g_file_new_for_path(path.as_ptr()) };
            if folder.is_null() {
                return Err(JetUiHostError::InvalidRequest(
                    "GTK rejected the file dialog initial directory".to_string(),
                ));
            }
            unsafe {
                gtk_file_dialog_set_initial_folder(dialog, folder);
                g_object_unref(folder as gpointer);
            }
        }

        if request.filters.is_empty() {
            return Ok(());
        }
        let object_type = unsafe { gtk_file_filter_get_type() };
        if object_type == 0 {
            return Err(JetUiHostError::CapabilityUnavailable {
                capability: "UI.FileDialog",
            });
        }
        let store = unsafe { g_list_store_new(object_type) };
        if store.is_null() {
            return Err(JetUiHostError::CapabilityUnavailable {
                capability: "UI.FileDialog",
            });
        }
        for specification in &request.filters {
            let filter = unsafe { gtk_file_filter_new() };
            if filter.is_null() {
                unsafe { g_object_unref(store as gpointer) };
                return Err(JetUiHostError::CapabilityUnavailable {
                    capability: "UI.FileDialog",
                });
            }
            if let Ok(label) = CString::new(specification.label.as_str()) {
                if !specification.label.is_empty() {
                    unsafe { gtk_file_filter_set_name(filter, label.as_ptr()) };
                }
            }
            for extension in &specification.extensions {
                let extension = extension.trim_start_matches('.');
                let pattern = if extension.starts_with('*') {
                    extension.to_string()
                } else {
                    format!("*.{extension}")
                };
                if let Ok(pattern) = CString::new(pattern) {
                    unsafe { gtk_file_filter_add_pattern(filter, pattern.as_ptr()) };
                }
            }
            for mime in &specification.mime_types {
                if let Ok(mime) = CString::new(mime.as_str()) {
                    unsafe { gtk_file_filter_add_mime_type(filter, mime.as_ptr()) };
                }
            }
            unsafe {
                g_list_store_append(store, filter as gpointer);
                g_object_unref(filter as gpointer);
            }
        }
        unsafe {
            gtk_file_dialog_set_filters(dialog, store as *mut GListModel);
            g_object_unref(store as gpointer);
        }
        Ok(())
    }

    fn gtk_run_file_dialog(
        window: *mut GtkWidget,
        request: &JetUiFileDialogRequest,
    ) -> JetUiServiceResult<JetUiFileDialogSelection> {
        let dialog = unsafe { gtk_file_dialog_new() };
        if dialog.is_null() {
            return Err(JetUiHostError::CapabilityUnavailable {
                capability: "UI.FileDialog",
            });
        }
        if let Err(error) = gtk_file_dialog_setup(dialog, window, request) {
            unsafe { g_object_unref(dialog as gpointer) };
            return Err(error);
        }
        let mode = match (request.kind, request.allow_multiple) {
            (JetUiFileDialogKind::Open, true) => FileDialogMode::OpenMultiple,
            (JetUiFileDialogKind::Open, false) => FileDialogMode::Open,
            (JetUiFileDialogKind::Save, false) => FileDialogMode::Save,
            (JetUiFileDialogKind::Save, true) => {
                unsafe { g_object_unref(dialog as gpointer) };
                return Err(JetUiHostError::InvalidRequest(
                    "save dialogs cannot select multiple files".to_string(),
                ));
            }
        };
        let mut pending = FileDialogRead {
            done: false,
            mode,
            files: Vec::new(),
            cancelled: false,
            error: None,
        };
        unsafe {
            match mode {
                FileDialogMode::Open => gtk_file_dialog_open(
                    dialog,
                    window,
                    std::ptr::null_mut(),
                    jet_gtk_file_dialog_ready,
                    (&mut pending as *mut FileDialogRead).cast(),
                ),
                FileDialogMode::OpenMultiple => gtk_file_dialog_open_multiple(
                    dialog,
                    window,
                    std::ptr::null_mut(),
                    jet_gtk_file_dialog_ready,
                    (&mut pending as *mut FileDialogRead).cast(),
                ),
                FileDialogMode::Save => gtk_file_dialog_save(
                    dialog,
                    window,
                    std::ptr::null_mut(),
                    jet_gtk_file_dialog_ready,
                    (&mut pending as *mut FileDialogRead).cast(),
                ),
            }
            while !pending.done {
                let _ = g_main_context_iteration(std::ptr::null_mut(), 1);
            }
            g_object_unref(dialog as gpointer);
        }
        if let Some(message) = pending.error {
            return Err(JetUiHostError::HostFailure {
                service: "UI.FileDialog",
                message,
            });
        }
        if pending.cancelled || pending.files.is_empty() {
            return Err(JetUiHostError::Cancelled(JetUiCancellation::User));
        }
        let access = match request.kind {
            JetUiFileDialogKind::Open => JetUiFsAccess::Read,
            JetUiFileDialogKind::Save => JetUiFsAccess::Write,
        };
        let files = pending
            .files
            .iter()
            .map(|path| request.grant.scope(path, access))
            .collect::<Result<Vec<_>, _>>();
        match files.and_then(JetUiFileDialogSelection::new) {
            Ok(selection) => Ok(selection),
            Err(error) => Err(error),
        }
    }
    impl JetUiHost for JetGtkBackend {
        fn capability_facts(&self) -> JetUiCapabilityFacts {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            state.capabilities
        }

        fn attach_accessibility(
            &self,
            node: &mut dyn JetUiAccessibilityTarget,
            accessibility: JetUiAccessibility,
        ) -> JetUiServiceResult<()> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            if let Err(error) = state.capabilities.require(JetUiCapability::Accessibility) {
                return Err(error);
            }
            match jet_ui_accessibility_attach(node, accessibility) {
                Ok(()) => Ok(()),
                Err(error) => Err(error),
            }
        }

        fn project_accessibility(
            &self,
            node: &dyn JetUiAccessibilityTarget,
            node_id: JetUiNodeId,
        ) -> JetUiServiceResult<Option<JetUiAccessibilityProjection>> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            if let Err(error) = state.capabilities.require(JetUiCapability::Accessibility) {
                return Err(error);
            }
            Ok(jet_ui_accessibility_project(node, node_id))
        }

        fn open_file(
            &mut self,
            request: JetUiFileDialogRequest,
        ) -> JetUiServiceResult<JetUiFileDialogSelection> {
            if let Err(error) = request.validate() {
                return Err(error);
            }
            let (window, display_ok, capabilities) = {
                let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                state.ensure_init();
                (state.window, state.display_ok, state.capabilities)
            };
            if let Err(error) = capabilities.require(JetUiCapability::FileDialog) {
                return Err(error);
            }
            if !display_ok || window.is_null() {
                return Err(JetUiHostError::Cancelled(JetUiCancellation::Headless));
            }
            gtk_run_file_dialog(window, &request)
        }

        fn save_file(
            &mut self,
            request: JetUiFileDialogRequest,
        ) -> JetUiServiceResult<JetUiFileDialogSelection> {
            if let Err(error) = request.validate() {
                return Err(error);
            }
            let (window, display_ok, capabilities) = {
                let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                state.ensure_init();
                (state.window, state.display_ok, state.capabilities)
            };
            if let Err(error) = capabilities.require(JetUiCapability::FileDialog) {
                return Err(error);
            }
            if !display_ok || window.is_null() {
                return Err(JetUiHostError::Cancelled(JetUiCancellation::Headless));
            }
            gtk_run_file_dialog(window, &request)
        }

        fn read_clipboard_text(&mut self) -> JetUiServiceResult<JetUiClipboardText> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            if let Err(error) = state.capabilities.require(JetUiCapability::Clipboard) {
                return Err(error);
            }
            let display = unsafe { gdk_display_get_default() };
            if display.is_null() {
                return Err(JetUiHostError::CapabilityUnavailable {
                    capability: "UI.Clipboard",
                });
            }
            let clipboard = unsafe { gdk_display_get_clipboard(display) };
            if clipboard.is_null() {
                return Err(JetUiHostError::CapabilityUnavailable {
                    capability: "UI.Clipboard",
                });
            }
            let mut pending = ClipboardRead {
                done: false,
                text: None,
            };
            unsafe {
                gdk_clipboard_read_text_async(
                    clipboard,
                    std::ptr::null_mut(),
                    jet_gtk_clipboard_read_ready,
                    (&mut pending as *mut ClipboardRead).cast(),
                );
                while !pending.done {
                    let _ = g_main_context_iteration(std::ptr::null_mut(), 1);
                }
            }
            match pending.text {
                Some(text) => Ok(JetUiClipboardText {
                    text,
                    selection: None,
                }),
                None => Err(JetUiHostError::CapabilityUnavailable {
                    capability: "UI.Clipboard",
                }),
            }
        }

        fn write_clipboard_text(&mut self, text: String) -> JetUiServiceResult<JetUiClipboardWrite> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            if let Err(error) = state.capabilities.require(JetUiCapability::Clipboard) {
                return Err(error);
            }
            let Ok(text_c) = CString::new(text.as_str()) else {
                return Err(JetUiHostError::InvalidRequest(
                    "clipboard text must not contain NUL".to_string(),
                ));
            };
            let display = unsafe { gdk_display_get_default() };
            if display.is_null() {
                return Err(JetUiHostError::CapabilityUnavailable {
                    capability: "UI.Clipboard",
                });
            }
            let clipboard = unsafe { gdk_display_get_clipboard(display) };
            if clipboard.is_null() {
                return Err(JetUiHostError::CapabilityUnavailable {
                    capability: "UI.Clipboard",
                });
            }
            unsafe { gdk_clipboard_set_text(clipboard, text_c.as_ptr()) };
            Ok(JetUiClipboardWrite {
                characters: text.chars().count(),
            })
        }

        fn poll_ime_event(&mut self) -> JetUiServiceResult<Option<JetUiImeEvent>> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            if let Err(error) = state.capabilities.require(JetUiCapability::Ime) {
                return Err(error);
            }
            Ok(state.ime_events.pop_front())
        }

        fn poll_drag_event(&mut self) -> JetUiServiceResult<Option<JetUiDragEvent>> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            if let Err(error) = state.capabilities.require(JetUiCapability::DragDrop) {
                return Err(error);
            }
            Ok(state.drag_events.pop_front())
        }

        fn register_shortcut(
            &mut self,
            binding: JetUiShortcutBinding,
        ) -> JetUiServiceResult<JetUiShortcutBinding> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            if let Err(error) = state.capabilities.require(JetUiCapability::Shortcuts) {
                return Err(error);
            }
            match state.shortcuts.register(binding) {
                Ok(binding) => Ok(binding),
                Err(error) => Err(error),
            }
        }

        fn dispatch_shortcut(
            &mut self,
            shortcut: JetUiShortcut,
        ) -> JetUiServiceResult<JetUiShortcutDispatch> {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.ensure_init();
            if let Err(error) = state.capabilities.require(JetUiCapability::Shortcuts) {
                return Err(error);
            }
            Ok(state.shortcuts.dispatch(&shortcut))
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
            // D-UI-EVT-DISP1=E: refresh identity→slot bindings after reconcile.
            jet_ui_bind_tree_clicks(node, "root");
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
                    jet_ui_advance_focus(&self.focus_nodes, &mut self.focused_index, code, 0)
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
